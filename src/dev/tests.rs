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
