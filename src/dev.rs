//! `dawn dev`: a windowed dry-run theme studio with hot reload.
//!
//! The studio is deliberately built on the normal dry-run path. It never opens
//! a greetd socket, never starts a session, and never grabs the display: the
//! window layer receives an auth mode that is already `DryRun`, so there is no
//! runtime flag a caller could accidentally flip.
//!
//! Reload is split into two halves so the event loop never has to parse while
//! it is painting:
//!
//! 1. [`Source`] names the file or bundle and knows how to re-read it.
//! 2. [`prepare_reload`] reads, parses, validates, and compares the new config
//!    against the last good one. It returns a [`Reload`] the window can apply
//!    in one cheap swap.
//!
//! File watching is best effort. The `notify` watcher is preferred; if it
//! cannot be created or watched, [`poll`] checks modification times every
//! 120 ms instead. Both paths are silent on failure, because a missing config
//! or an unavailable inotify descriptor must only affect the window's scene,
//! never the process exit code or the terminal.
//!
//! Reloads are debounced with [`Debounce`]. Editors often write a temporary
//! file and rename it over the real one, which produces a burst of events; the
//! debouncer turns that burst into one reload after 90 ms of quiet.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher as _};
use tracing::{info, warn};

use crate::cli::DevCommand;
use crate::config::{self, BackgroundConfig, Config, GpuConfig, Issue, LoadedConfig, UiConfig};
use crate::plugin::{self, Plugin};
use crate::theme;
use crate::ui::window;
use std::process::ExitCode;

/// Time a burst of filesystem events must be quiet before one reload runs.
pub const DEBOUNCE: Duration = Duration::from_millis(90);
/// Fallback modification-time poll interval.
pub const POLL_INTERVAL: Duration = Duration::from_millis(120);
/// The default studio window. A greeter normally runs fullscreen, but a studio
/// has to sit next to an editor.
pub const DEFAULT_SIZE: (u32, u32) = (1280, 800);

/// What the watcher should watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchSpec {
  pub root: PathBuf,
  /// True for a bundle tree, false for a single config file's directory.
  pub recursive: bool,
  /// When set, only events touching this file count. The watcher still watches
  /// the parent directory so an editor's write-and-rename cannot orphan it.
  pub filter: Option<PathBuf>,
}

/// Where a dev session reads its config from.
#[derive(Debug, Clone)]
pub struct Source {
  /// Shown in the title and the HUD: a bundle id or a config path.
  pub label: String,
  pub config_path: Option<PathBuf>,
  /// A bundle directory, when the source is a DSB tree.
  pub bundle: Option<PathBuf>,
  pub watch: Option<WatchSpec>,
}

impl Source {
  /// A DSB bundle directory. The caller has already validated it.
  #[must_use]
  pub fn bundle(dir: &Path, label: String) -> Self {
    Self {
      label,
      config_path: Some(dir.join("config.toml")),
      bundle: Some(dir.to_path_buf()),
      watch: Some(WatchSpec {
        root: dir.to_path_buf(),
        recursive: true,
        filter: None,
      }),
    }
  }

  /// A standalone config file. We watch its parent so a temp-and-rename save
  /// still triggers, but only accept events for this file name.
  #[must_use]
  pub fn file(path: &Path) -> Self {
    let watch = path.parent().map(|parent| WatchSpec {
      root: parent.to_path_buf(),
      recursive: false,
      filter: Some(path.to_path_buf()),
    });
    Self {
      label: path.display().to_string(),
      config_path: Some(path.to_path_buf()),
      bundle: None,
      watch,
    }
  }

  /// The config discovery result from a normal run.
  #[must_use]
  pub fn discovered(path: Option<PathBuf>) -> Self {
    match path {
      Some(path) => Self::file(&path),
      None => Self {
        label: "built-in defaults".to_string(),
        config_path: None,
        bundle: None,
        watch: None,
      },
    }
  }

  /// An installed theme that could not be found. It still opens the default
  /// scene so the studio is usable while the author fixes the id, but there is
  /// nothing to watch.
  #[must_use]
  fn missing_theme(id: &str) -> Self {
    Self {
      label: id.to_string(),
      config_path: None,
      bundle: None,
      watch: None,
    }
  }

  /// Re-reads the config. Any field-level issue is a failed reload: the window
  /// keeps the last good scene rather than swapping in a half-default config.
  pub fn load(&self) -> std::result::Result<LoadedConfig, String> {
    let mut loaded = if let Some(dir) = &self.bundle {
      config::load_theme(&dir.join("config.toml"))
    } else if let Some(path) = &self.config_path {
      config::load(Some(path))
    } else {
      config::load(None)
    };

    // A standalone config may select an installed bundle with
    // `[theme].package`, exactly like a normal run. A bundle cannot select
    // another bundle, and `load_theme` already rejects that.
    if let Some(id) = loaded.package.take() {
      match theme::find(&id) {
        Some(dir) => loaded = config::load_theme(&dir.join("config.toml")),
        None => loaded.issues.insert(
          0,
          Issue {
            key: "theme.package".to_string(),
            location: None,
            message: format!("theme {id:?} is not installed; run `dawn list` or `dawn add PATH`"),
          },
        ),
      }
    }

    if let Some(issue) = loaded.issues.first() {
      return Err(issue.to_string());
    }
    Ok(loaded)
  }
}

/// A prepared reload. Everything expensive has already happened; the window
/// only swaps values.
pub struct Reload {
  pub config: Config,
  pub plugins: Vec<Plugin>,
  pub warning: Option<String>,
  pub background: BackgroundChange,
  /// True when the font family or size changed and the text engine must be
  /// rebuilt. Otherwise the existing engine and font database are kept.
  pub rebuild_text: bool,
  /// Parse, validate, and compare cost. Reported in the HUD and the log.
  pub cost_ms: f32,
}

/// What happened to the background section between two configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundChange {
  None,
  /// The background must be re-initialised. `decode` is false when the image
  /// path is identical, so the loaded frame set can be reused.
  Reload {
    decode: bool,
  },
}

impl BackgroundChange {
  #[must_use]
  pub const fn is_reload(self) -> bool {
    matches!(self, Self::Reload { .. })
  }

  #[must_use]
  pub const fn decode(self) -> bool {
    matches!(self, Self::Reload { decode: true })
  }
}

/// Compares the parts of `[background]` that decide whether the current
/// decoded frame set is still valid.
///
/// The rule the studio promises is simple: a different image path is the only
/// reason to decode pixels again. Everything else either needs no wallpaper
/// work (a colour-only theme edit) or can reuse the frames that are already in
/// memory.
#[must_use]
pub fn background_change(previous: &BackgroundConfig, next: &BackgroundConfig) -> BackgroundChange {
  if same_background(previous, next) {
    return BackgroundChange::None;
  }
  BackgroundChange::Reload {
    decode: previous.image != next.image,
  }
}

fn same_background(left: &BackgroundConfig, right: &BackgroundConfig) -> bool {
  left.kind == right.kind
    && left.color == right.color
    && left.gradient == right.gradient
    && left.image == right.image
    && left.fit == right.fit
    && left.fps == right.fps
    && left.memory_limit == right.memory_limit
    && left.idle_pause == right.idle_pause
    && left.monitors == right.monitors
}

/// True when the text engine must be rebuilt. The engine owns the font
/// database and shaping cache, so a colour-only edit must not throw it away.
#[must_use]
pub fn text_engine_changed(previous: &UiConfig, next: &UiConfig) -> bool {
  previous.font != next.font || (previous.font_size - next.font_size).abs() > f32::EPSILON
}

/// Keeps the last good scene and the current error line.
///
/// This is a tiny state machine on purpose. The window never mutates the app on
/// a failed reload, so "keep the last good theme" is true by construction; this
/// type only decides when a new error deserves a log line.
#[derive(Debug)]
pub struct ReloadState {
  pub last_good: Config,
  /// First line of the last failure, or `None` after a success.
  pub error: Option<String>,
  pub last_ms: Option<f32>,
}

impl ReloadState {
  #[must_use]
  pub fn new(config: Config) -> Self {
    Self {
      last_good: config,
      error: None,
      last_ms: None,
    }
  }

  /// Records a successful swap.
  pub fn success(&mut self, config: Config, cost_ms: f32) {
    self.last_good = config;
    self.error = None;
    self.last_ms = Some(cost_ms);
  }

  /// Records a failure and returns true when this is a new distinct error, so
  /// the caller can warn once instead of once per keystroke.
  pub fn failure(&mut self, error: impl Into<String>) -> bool {
    let error = error.into();
    let first = error.lines().next().unwrap_or_default().to_string();
    if self.error.as_deref() == Some(first.as_str()) {
      return false;
    }
    self.error = Some(first);
    true
  }
}

/// Reads, parses, validates, and compares a new config.
///
/// This runs on the event loop, but it is deliberately small: one bounded file
/// read, one TOML parse, the normal validator, and a plugin-directory scan.
/// Wallpaper decode, blur, and video work stay on their existing workers.
pub fn prepare_reload(
  source: &Source,
  host_gpu: &GpuConfig,
  previous: &Config,
) -> std::result::Result<Reload, String> {
  let started = Instant::now();
  let loaded = source.load()?;
  let warning = loaded.summary();
  let mut config = loaded.config;
  // The GPU belongs to the machine, not to the theme. Keep the adapter that
  // the studio booted with so an edit cannot silently switch renderers.
  config.gpu = host_gpu.clone();
  // Dev is a windowed studio by definition, even if the bundle asks for a
  // fullscreen greeter.
  config.ui.fullscreen = false;

  let background = background_change(&previous.background, &config.background);
  let rebuild_text = text_engine_changed(&previous.ui, &config.ui);

  let report = plugin::load_all(&config.plugins, &config.theme);
  for error in &report.errors {
    warn!("plugin: {error}");
  }

  let cost_ms = started.elapsed().as_secs_f32() * 1000.0;
  Ok(Reload {
    config,
    plugins: report.plugins,
    warning,
    background,
    rebuild_text,
    cost_ms,
  })
}

/// A burst coalescer driven by caller-supplied instants.
///
/// Keeping the clock outside makes the rule testable without sleeps: note every
/// event, then ask whether the burst has been quiet for [`DEBOUNCE`].
#[derive(Debug, Default)]
pub struct Debounce {
  last: Option<Instant>,
  pending: bool,
}

impl Debounce {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  /// Records one event at `now`.
  pub fn note(&mut self, now: Instant) {
    self.last = Some(now);
    self.pending = true;
  }

  /// True when the pending burst has been quiet for the debounce window. The
  /// call consumes the burst, so one settled burst produces exactly one reload.
  pub fn settle(&mut self, now: Instant) -> bool {
    if !self.pending {
      return false;
    }
    let Some(last) = self.last else {
      return false;
    };
    if now.saturating_duration_since(last) < DEBOUNCE {
      return false;
    }
    self.pending = false;
    true
  }

  /// How long until the current burst settles. `None` when nothing is pending.
  #[must_use]
  pub fn remaining(&self, now: Instant) -> Option<Duration> {
    if !self.pending {
      return None;
    }
    let last = self.last?;
    Some(DEBOUNCE.saturating_sub(now.saturating_duration_since(last)))
  }
}

/// Live filesystem watcher. Dropping it drops the notify handle, which closes
/// the callback channel and lets the debounce thread exit.
pub struct Watcher {
  _inner: notify::RecommendedWatcher,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Starts the notify watcher. Returns `Err` when the backend or the watch
/// cannot be created; the caller then falls back to [`poll`].
pub fn watch(spec: &WatchSpec, wake: Wake) -> std::result::Result<Watcher, String> {
  let (tx, rx) = mpsc::channel::<()>();
  let filter = spec.filter.clone();
  let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
    let Ok(event) = result else {
      // A watcher error is not fatal. The next event, or the polling fallback
      // after a real failure, still gets a chance to reload.
      return;
    };
    // Reading the config to reload it is itself an OPEN event on Linux. If we
    // forwarded access events the studio would reload forever: read, event,
    // reload, read. Only content and namespace changes are interesting.
    if matches!(event.kind, notify::EventKind::Access(_)) {
      return;
    }
    if let Some(filter) = &filter {
      let matches = event.paths.iter().any(|path| {
        path == filter
          || path.file_name().is_some_and(|name| {
            filter
              .file_name()
              .is_some_and(|wanted| name.eq_ignore_ascii_case(wanted))
          })
      });
      if !matches {
        return;
      }
    }
    let _ = tx.send(());
  })
  .map_err(|error| error.to_string())?;

  let mode = if spec.recursive {
    RecursiveMode::Recursive
  } else {
    RecursiveMode::NonRecursive
  };
  watcher
    .watch(&spec.root, mode)
    .map_err(|error| error.to_string())?;

  let thread = std::thread::Builder::new()
    .name("dawn-dev-watch".to_string())
    .spawn(move || {
      let mut debounce = Debounce::new();
      loop {
        let timeout = debounce.remaining(Instant::now()).unwrap_or(POLL_INTERVAL);
        match rx.recv_timeout(timeout) {
          Ok(()) => debounce.note(Instant::now()),
          Err(RecvTimeoutError::Timeout) => {
            if debounce.settle(Instant::now()) {
              wake();
            }
          }
          Err(RecvTimeoutError::Disconnected) => break,
        }
      }
    });
  if thread.is_err() {
    return Err("cannot start the watcher thread".to_string());
  }
  Ok(Watcher { _inner: watcher })
}

/// Modification-time polling fallback. One thread, one cheap metadata check
/// per interval, no output.
#[derive(Debug)]
pub struct Poller {
  stop: Arc<AtomicBool>,
}

impl Drop for Poller {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
  }
}

/// Starts the 120 ms mtime poller. Used only when [`watch`] fails.
#[must_use]
pub fn poll(spec: &WatchSpec, wake: Wake) -> Poller {
  let stop = Arc::new(AtomicBool::new(false));
  let thread_stop = Arc::clone(&stop);
  let spec = spec.clone();
  let _ = std::thread::Builder::new()
    .name("dawn-dev-poll".to_string())
    .spawn(move || {
      let mut previous = fingerprint(&spec);
      while !thread_stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL_INTERVAL);
        if thread_stop.load(Ordering::Relaxed) {
          break;
        }
        let current = fingerprint(&spec);
        if current != previous {
          previous = current;
          wake();
        }
      }
    });
  Poller { stop }
}

/// A cheap change token for a file or a bundle tree.
fn fingerprint(spec: &WatchSpec) -> Option<(SystemTime, u64)> {
  if let Some(filter) = &spec.filter {
    return std::fs::metadata(filter)
      .ok()
      .and_then(|metadata| metadata.modified().ok())
      .map(|modified| (modified, 0));
  }
  let mut newest: Option<SystemTime> = None;
  for entry in walkdir::WalkDir::new(&spec.root).follow_links(false) {
    let Ok(entry) = entry else {
      continue;
    };
    let Ok(metadata) = entry.metadata() else {
      continue;
    };
    if !metadata.is_file() {
      continue;
    }
    if let Ok(modified) = metadata.modified()
      && newest.is_none_or(|current| modified > current)
    {
      newest = Some(modified);
    }
  }
  newest.map(|modified| (modified, 0))
}

/// A small exponential estimate of presented frames per second.
#[derive(Debug, Default)]
pub struct FpsMeter {
  last: Option<Instant>,
  intervals: [Duration; 8],
  len: usize,
  next: usize,
}

impl FpsMeter {
  /// Records one presented frame.
  pub fn tick(&mut self, now: Instant) {
    if let Some(last) = self.last {
      let interval = now.saturating_duration_since(last);
      if !interval.is_zero() {
        self.intervals[self.next] = interval;
        self.next = (self.next + 1) % self.intervals.len();
        self.len = (self.len + 1).min(self.intervals.len());
      }
    }
    self.last = Some(now);
  }

  /// The current estimate, or `None` until two frames have been seen.
  #[must_use]
  pub fn estimate(&self) -> Option<f32> {
    if self.len == 0 {
      return None;
    }
    let total: Duration = self.intervals.iter().take(self.len).sum();
    let seconds = total.as_secs_f32();
    if seconds <= f32::EPSILON {
      return None;
    }
    Some((self.len as f32 / seconds).clamp(0.0, 999.0))
  }
}

/// Runs the studio. This is the only entry point `main` needs.
pub fn run(command: DevCommand) -> Result<ExitCode> {
  let (source, loaded) = resolve(&command)?;

  for issue in &loaded.issues {
    warn!("{issue}");
  }
  let mut config = loaded.config.clone();
  config.gpu.apply_env();
  config.ui.fullscreen = false;
  let warning = loaded.summary();

  let sessions = crate::session::discover(&config.sessions.dirs);
  let report = plugin::load_all(&config.plugins, &config.theme);
  for error in &report.errors {
    warn!("plugin: {error}");
  }

  info!(
    source = %source.label,
    "dawn dev: dry run, no greetd, no session start"
  );
  let size = command.size.unwrap_or(DEFAULT_SIZE);
  window::run_dev(
    config,
    sessions,
    report.plugins,
    warning,
    size,
    source,
    !command.no_hud,
  )
  .context("dev studio failed")?;
  Ok(ExitCode::SUCCESS)
}

/// Resolves `PATH`, `--theme`, or normal discovery, printing the bundle report
/// for a directory source.
fn resolve(command: &DevCommand) -> Result<(Source, LoadedConfig)> {
  if let Some(path) = command.path.as_deref() {
    if path.is_dir() {
      let check = theme::check_tree(path);
      let revision = check
        .revision
        .map_or_else(|| "unknown".to_string(), |revision| revision.to_string());
      println!("bundle: {} revision {revision}", path.display());
      for warning in &check.warnings {
        println!("warning: {warning}");
      }
      for issue in &check.issues {
        println!("{issue}");
      }
      if !check.ok() {
        anyhow::bail!("bundle {} is not a valid DSB-2 tree", path.display());
      }
      let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
      let source = Source::bundle(path, label);
      let loaded = source.load().map_err(anyhow::Error::msg)?;
      return Ok((source, loaded));
    }
    let source = Source::file(path);
    let loaded = source.load().map_err(anyhow::Error::msg)?;
    return Ok((source, loaded));
  }

  if let Some(id) = command.theme.as_deref() {
    if let Some(dir) = theme::find(id) {
      let source = Source::bundle(&dir, id.to_string());
      let loaded = source.load().map_err(anyhow::Error::msg)?;
      return Ok((source, loaded));
    }
    // Match a normal run: a missing theme is a warning and the built-in
    // defaults stay usable, not a reason to refuse to open a window.
    let mut loaded = config::load(None);
    loaded.issues.insert(
      0,
      Issue {
        key: "theme.package".to_string(),
        location: None,
        message: format!("theme {id:?} is not installed; run `dawn list` or `dawn add PATH`"),
      },
    );
    return Ok((Source::missing_theme(id), loaded));
  }

  let loaded = config::load(None);
  let source = Source::discovered(loaded.path.clone());
  Ok((source, loaded))
}

/// Sessions are only used to make the studio look like the real login form.
#[cfg(test)]
#[must_use]
pub fn sessions(config: &Config) -> Vec<crate::session::Session> {
  crate::session::discover(&config.sessions.dirs)
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use super::{
    BackgroundChange, Debounce, ReloadState, Source, background_change, prepare_reload, resolve,
    text_engine_changed,
  };
  use crate::cli::DevCommand;
  use crate::config::{BackgroundConfig, BackgroundKind, Config, GpuConfig};
  use crate::layout::Fit;
  use std::time::{Duration, Instant};

  fn config_with(background: BackgroundConfig) -> Config {
    Config {
      background,
      ..Config::default()
    }
  }

  #[test]
  fn an_invalid_bundle_is_rejected_before_a_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let command = DevCommand {
      path: Some(dir.path().to_path_buf()),
      theme: None,
      size: None,
      no_hud: true,
    };
    let error = resolve(&command).expect_err("a directory without a manifest must fail");
    assert!(
      error.to_string().contains("not a valid DSB-2 tree"),
      "{error}"
    );
  }

  #[test]
  fn a_bundle_source_watches_the_whole_tree() {
    let source = Source::bundle(std::path::Path::new("/tmp/kinich"), "kinich".to_string());
    let watch = source.watch.expect("bundle watch");
    assert!(watch.recursive);
    assert_eq!(watch.filter, None);
    assert_eq!(watch.root, std::path::PathBuf::from("/tmp/kinich"));
  }

  #[test]
  fn a_config_source_watches_its_parent_for_that_file_only() {
    let source = Source::file(std::path::Path::new("/tmp/theme/config.toml"));
    let watch = source.watch.expect("file watch");
    assert!(!watch.recursive);
    assert_eq!(
      watch.filter,
      Some(std::path::PathBuf::from("/tmp/theme/config.toml"))
    );
    assert_eq!(watch.root, std::path::PathBuf::from("/tmp/theme"));
  }

  #[test]
  fn a_burst_of_events_produces_exactly_one_reload() {
    let mut debounce = Debounce::new();
    let start = Instant::now();
    for index in 0..20 {
      debounce.note(start + Duration::from_millis(index * 3));
    }
    // Still inside the quiet window: nothing may fire yet.
    assert!(!debounce.settle(start + Duration::from_millis(80)));
    // 90 ms after the last event (57 ms + 90 ms) the burst settles once.
    assert!(debounce.settle(start + Duration::from_millis(147)));
    assert!(!debounce.settle(start + Duration::from_millis(400)));
  }

  #[test]
  fn a_new_event_restarts_the_debounce_window() {
    let mut debounce = Debounce::new();
    let start = Instant::now();
    debounce.note(start);
    assert!(!debounce.settle(start + Duration::from_millis(89)));
    debounce.note(start + Duration::from_millis(89));
    assert!(!debounce.settle(start + Duration::from_millis(150)));
    assert!(debounce.settle(start + Duration::from_millis(180)));
  }

  #[test]
  fn an_unchanged_background_is_not_reinitialised() {
    let config = Config::default();
    assert_eq!(
      background_change(&config.background, &config.background),
      BackgroundChange::None
    );
  }

  #[test]
  fn a_new_background_path_requests_a_decode() {
    let old = Config::default().background;
    let mut new = old.clone();
    new.image = Some("/tmp/other.png".into());
    assert_eq!(
      background_change(&old, &new),
      BackgroundChange::Reload { decode: true }
    );
  }

  #[test]
  fn an_fps_only_change_reloads_without_a_decode() {
    let old = Config::default().background;
    let mut new = old.clone();
    new.fps = 24;
    assert_eq!(
      background_change(&old, &new),
      BackgroundChange::Reload { decode: false }
    );
  }

  #[test]
  fn per_monitor_changes_count_as_background_changes() {
    let old = Config::default().background;
    let mut new = old.clone();
    new.monitors.insert(
      "DP-1".to_string(),
      crate::config::BackgroundOverride {
        kind: Some(BackgroundKind::Solid),
        ..Default::default()
      },
    );
    assert_eq!(
      background_change(&old, &new),
      BackgroundChange::Reload { decode: false }
    );
  }

  #[test]
  fn a_font_or_size_change_rebuilds_the_text_engine() {
    let old = Config::default();
    assert!(!text_engine_changed(&old.ui, &old.ui));
    let mut font = old.ui.clone();
    font.font = "Other Font".to_string();
    assert!(text_engine_changed(&old.ui, &font));
    let mut size = old.ui.clone();
    size.font_size += 1.0;
    assert!(text_engine_changed(&old.ui, &size));
  }

  #[test]
  fn a_failed_reload_keeps_the_last_good_config_and_a_later_success_clears_it() {
    let good = Config::default();
    let mut state = ReloadState::new(good.clone());
    assert!(state.failure("theme.accent: invalid colour"));
    assert!(state.error.is_some());
    // The same error is not logged twice.
    assert!(!state.failure("theme.accent: invalid colour"));
    assert_eq!(state.last_good.ui.font, good.ui.font);
    state.success(good.clone(), 3.2);
    assert!(state.error.is_none());
    assert_eq!(state.last_ms, Some(3.2));
  }

  #[test]
  fn a_malformed_config_is_a_failed_reload_and_a_fix_recovers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    let source = Source::file(&path);
    let previous = Config::default();

    std::fs::write(&path, "[theme]\naccent = \"not-a-colour\"\n").expect("write");
    let error = match prepare_reload(&source, &GpuConfig::default(), &previous) {
      Ok(_) => panic!("invalid colour must fail"),
      Err(error) => error,
    };
    let mut state = ReloadState::new(previous.clone());
    assert!(state.failure(error));
    assert_eq!(state.last_good.ui.font, previous.ui.font);

    std::fs::write(&path, "[theme]\naccent = \"#ff0000\"\n").expect("write");
    let reload = prepare_reload(&source, &GpuConfig::default(), &previous).expect("fixed config");
    state.success(reload.config.clone(), reload.cost_ms);
    assert!(state.error.is_none());
    assert_eq!(
      reload.config.theme.accent,
      crate::color::Rgb::solid(255, 0, 0)
    );
  }

  #[test]
  fn a_same_background_section_keeps_loaded_frames() {
    // The pure decision is what the window follows. Same path and same
    // settings means no reload at all, so the existing frame set stays.
    let config = config_with(BackgroundConfig {
      kind: BackgroundKind::Image,
      image: Some("/tmp/one.png".into()),
      fit: Fit::Cover,
      ..Config::default().background
    });
    assert_eq!(
      background_change(&config.background, &config.background.clone()),
      BackgroundChange::None
    );
  }

  #[test]
  fn a_reload_reports_a_measured_cost() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    let mut file = std::fs::File::create(&path).expect("create");
    writeln!(file, "[ui]\nfont = \"Inter\"\nfont_size = 15").expect("write");
    let source = Source::file(&path);
    let reload =
      prepare_reload(&source, &GpuConfig::default(), &Config::default()).expect("valid config");
    assert!(reload.cost_ms >= 0.0);
    // The file explicitly names a different family, so the engine is rebuilt.
    assert!(reload.rebuild_text);
  }
}
