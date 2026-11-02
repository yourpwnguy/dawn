//! The winit window and presentation.
//!
//! This is the only module that knows a window exists. It translates winit key
//! events into [`Key`], forwards auth events from the worker's event-loop
//! proxy, and presents frames either through the GPU compositor or the
//! softbuffer fallback. All layout, animation, and interaction policy lives
//! elsewhere; keeping it out of here is what makes the login flow testable
//! without a display server.
//!
//! The GPU path rasterises the UI overlay with tiny-skia only when the UI
//! changes, then lets [`gpu`] blend the wallpaper underneath it. A video frame
//! therefore costs one texture upload and one full-screen triangle, not a
//! full CPU repaint. If adapter, device, or surface creation fails, the same
//! window falls back to softbuffer and the original full CPU frame path. A
//! greeter must still show a login form on a machine with no usable driver.
//!
//! Concurrency rule: decoding a wallpaper can take hundreds of milliseconds, so
//! it never runs here. The view paints its gradient immediately, a loader
//! thread returns a decoded frame set through the event proxy, and stale
//! results are dropped by generation. MPRIS polling follows the same pattern on
//! its own thread.
//!
//! Redraw policy: on input, resize, a visible clock change, a wallpaper frame,
//! or an animation frame. An idle static screen waits until the next clock
//! boundary and does not spin.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use crate::app::{App, Finish, Key};
use crate::audio::AudioHandle;
use crate::auth::worker::{self, Event, Mode};
use crate::color::Rgb;
use crate::config::{
  Config, GpuBackend, GpuConfig, GpuDevice, GpuFallback, MotionMoment, TransitionTrigger,
};
use crate::dev::{self, ReloadState, Source};
use crate::plugin::Plugin;
use crate::session::Session;
use crate::ui::background::{BackgroundView, LoadedBackground};
use crate::ui::canvas::Canvas;
use crate::ui::drag::{Drag, HitRegion};
use crate::ui::glass::GlassCache;
use crate::ui::gpu;
use crate::ui::hud::Hud;
use crate::ui::media::{self, MediaControl, MediaHandle};
use crate::ui::motion::MotionFrame;
use crate::ui::render::{self, DecorCache, Scene};
use crate::ui::text::TextEngine;
use crate::ui::transition::{self, Transition};
use crate::ui::view::{Focus, View};

mod app_handler;
mod background_load;
mod dev_reload;
mod finish;
mod input;
mod redraw;
mod schedule;
mod setup;

#[cfg(test)]
mod tests;

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 800;
/// Motion is capped at roughly 60 frames per second. The login screen does not
/// need more, and a software renderer pays for every extra frame.
const MOTION_FRAME: Duration = Duration::from_millis(16);
/// A breathing camera is a slow sine; half rate is visually identical and
/// halves the compositor work on an idle login.
const BREATH_FRAME: Duration = Duration::from_millis(33);
const CLOCK_TICK: Duration = Duration::from_secs(1);
/// Hard ceiling on how long a successful auth may hold the window open just to
/// finish the cut sequence. Auth is already done at that point; this only
/// delays the process exit, and a pathological `slam_ms` must not turn a login
/// into a two-minute show. Anything past this exits immediately.
const MAX_SUCCESS_HOLD: Duration = Duration::from_millis(2000);

/// Hit-stop on the success moment shares the transition's exit budget. A config
/// can ask for 500 ms, but the greeter still leaves for the session within the
/// existing two-second ceiling. Submit and failure holds never gate the exit
/// and keep their full configured length.
fn clamped_hitstop(duration: Duration, success: bool) -> Duration {
  if success {
    duration.min(MAX_SUCCESS_HOLD)
  } else {
    duration
  }
}

#[derive(Debug, thiserror::Error)]
pub enum WindowError {
  #[error("cannot create the event loop: {0}")]
  EventLoop(#[from] winit::error::EventLoopError),
  #[error("cannot start the authentication worker: {0}")]
  Worker(#[from] std::io::Error),
  #[error("graphics setup failed: {0}")]
  Graphics(String),
  #[error("the window closed before a login finished")]
  Closed,
}

/// Events delivered through winit's proxy from worker threads.
enum UserEvent {
  Auth(Event),
  /// A decoded wallpaper plus the generation that requested it.
  Background(u64, Result<LoadedBackground, String>),
  /// A media player's visible state changed; the payload is already in shared
  /// state, so this is purely a wake-up.
  Media,
  /// A live wallpaper published a frame. The window paints immediately instead
  /// of polling for the stream sequence.
  Frame,
  /// The dev watcher settled a burst. The handler re-reads the config on the
  /// event loop; the watcher thread itself never parses anything.
  DevReload,
}

/// Creates the window, runs the event loop, and returns why it stopped.
pub fn run(
  config: Config,
  sessions: Vec<Session>,
  plugins: Vec<Plugin>,
  warning: Option<String>,
  dry_run: bool,
  fullscreen: bool,
  audio: AudioHandle,
) -> Result<Finish, WindowError> {
  let auth = if dry_run {
    AuthMode::DryRun
  } else {
    AuthMode::Greetd {
      socket: config.auth.socket.clone(),
      timeout: config.auth.timeout,
    }
  };
  run_with(
    config,
    sessions,
    plugins,
    warning,
    auth,
    fullscreen,
    (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    None,
    audio,
  )
}

/// The dev studio's window entry point.
///
/// There is no `dry_run` parameter: the auth mode is built here as
/// `AuthMode::DryRun`, and `run_with` never derives it from the config.
/// The window is always windowed and always uses the requested size.
pub fn run_dev(
  config: Config,
  sessions: Vec<Session>,
  plugins: Vec<Plugin>,
  warning: Option<String>,
  size: (u32, u32),
  window: DevWindow,
) -> Result<Finish, WindowError> {
  run_with(
    config,
    sessions,
    plugins,
    warning,
    dev_auth_mode(),
    false,
    size,
    Some(DevStart {
      source: window.source,
      hud_enabled: window.hud,
    }),
    window.audio,
  )
}

/// The parts of a dev window that come from the studio rather than the config.
pub struct DevWindow {
  pub source: Source,
  pub hud: bool,
  pub audio: AudioHandle,
}

/// Kept as a named function so the window test can assert that dev mode never
/// constructs a greetd auth mode.
fn dev_auth_mode() -> AuthMode {
  AuthMode::DryRun
}

/// The only two ways this window may authenticate. Dev passes `DryRun`
/// directly; a normal run chooses once from its `--dry-run` flag.
enum AuthMode {
  Greetd {
    socket: Option<std::path::PathBuf>,
    timeout: Duration,
  },
  DryRun,
}

/// What the dev window needs before the event loop creates its proxy.
struct DevStart {
  source: Source,
  hud_enabled: bool,
}

#[allow(clippy::too_many_arguments)]
fn run_with(
  config: Config,
  sessions: Vec<Session>,
  plugins: Vec<Plugin>,
  warning: Option<String>,
  auth: AuthMode,
  fullscreen: bool,
  initial_size: (u32, u32),
  dev_start: Option<DevStart>,
  audio: AudioHandle,
) -> Result<Finish, WindowError> {
  let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
  let proxy = event_loop.create_proxy();
  let auth_proxy = proxy.clone();
  let dry_run = matches!(&auth, AuthMode::DryRun);
  let mode = match auth {
    AuthMode::DryRun => Mode::DryRun,
    AuthMode::Greetd { socket, timeout } => Mode::Greetd { socket, timeout },
  };
  let worker = worker::spawn(mode, move |event| {
    // If the loop is already gone, the event is meaningless, so ignoring the
    // send error is correct.
    let _ = auth_proxy.send_event(UserEvent::Auth(event));
  })?;

  let idle_pause = config.background.idle_pause;
  let drag = Drag::new(
    config.layout.save.then(|| config.layout.resolved_file()),
    config.motion.drag,
  );
  let background = BackgroundView::new(&config);
  let frame_pending = Arc::new(AtomicBool::new(false));
  let started = Instant::now();
  let text = TextEngine::new(&config.ui.font);
  // `[gpu]` belongs to the presenter, not to the scene, so take it before the
  // config is moved into App. The main entry point has already applied
  // DAWN_GPU to this value.
  let gpu = config.gpu.clone();
  let dev = dev_start.map(|start| DevRuntime::new(start, &config, proxy.clone()));
  let boot = crate::rituals::on_boot(&config.rituals);
  let mut app = App::new(config, sessions, plugins, Some(worker), warning, dry_run);
  app.attach_audio(audio);
  app.audio().play(crate::audio::Event::Boot);
  for event in &boot.events {
    app.audio().play(*event);
  }
  if let Some(note) = boot.note {
    app.set_boot_note(note);
  }

  let mut media = Vec::new();
  for state in app.media_states() {
    let media_proxy = proxy.clone();
    media.push(media::spawn(state, move || {
      let _ = media_proxy.send_event(UserEvent::Media);
    }));
  }

  let mut handler = Handler {
    app,
    background,
    glass: GlassCache::new(),
    decor: DecorCache::new(),
    text,
    gpu,
    fullscreen,
    initial_size,
    window: None,
    presenter: None,
    frame_canvas: None,
    ui_canvas: None,
    blur_canvas: None,
    ui_dirty: true,
    blur_source_at: None,
    started,
    last_overlay_at: None,
    last_frame_at: started,
    next_frame_at: None,
    monitor: None,
    modifiers: ModifiersState::empty(),
    finish: None,
    error: None,
    proxy,
    _media: media,
    idle_pause,
    last_input: Instant::now(),
    paused: false,
    last_frame_id: u64::MAX,
    splash_ui: None,
    splash_was_active: false,
    frame_window: Instant::now(),
    redraw_frames: 0,
    wallpaper_frames: 0,
    drag,
    regions: Vec::new(),
    cursor: (0.0, 0.0),
    pointer_inside: false,
    frame_pending,
    transition: None,
    burst_at: None,
    hitstop_until: None,
    dev,
  };
  event_loop.run_app(&mut handler)?;

  if let Some(error) = handler.error {
    return Err(error);
  }
  handler.finish.ok_or(WindowError::Closed)
}

/// Live dev-studio state owned by the window.
struct DevRuntime {
  source: Source,
  hud_enabled: bool,
  hud_visible: bool,
  state: ReloadState,
  /// The adapter and fallback policy the studio booted with. A theme edit must
  /// not switch renderers underneath the live surface.
  host_gpu: GpuConfig,
  fps: dev::FpsMeter,
  _watcher: Option<dev::Watcher>,
  _poller: Option<dev::Poller>,
}

impl DevRuntime {
  fn new(start: DevStart, config: &Config, proxy: EventLoopProxy<UserEvent>) -> Self {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
      let _ = proxy.send_event(UserEvent::DevReload);
    });
    let (watcher, poller) = match start.source.watch.clone() {
      Some(spec) => match dev::watch(&spec, Arc::clone(&wake)) {
        Ok(watcher) => (Some(watcher), None),
        Err(_) => (None, Some(dev::poll(&spec, wake))),
      },
      None => (None, None),
    };
    Self {
      source: start.source,
      hud_enabled: start.hud_enabled,
      hud_visible: start.hud_enabled,
      state: ReloadState::new(config.clone()),
      host_gpu: config.gpu.clone(),
      fps: dev::FpsMeter::default(),
      _watcher: watcher,
      _poller: poller,
    }
  }

  fn hud(&self) -> Option<Hud> {
    if !self.hud_enabled || !self.hud_visible {
      return None;
    }
    Some(Hud {
      visible: true,
      source: self.source.label.clone(),
      last_ms: self.state.last_ms,
      fps: self.fps.estimate(),
      error: self.state.error.clone(),
    })
  }
}

/// Which presentation path the window ended up with. GPU construction can fail
/// on a boot image with no driver, so the fallback is selected once at startup
/// and then kept for the life of the window.
enum Presenter {
  Gpu(Box<gpu::GpuRenderer>),
  Cpu(softbuffer::Surface<Arc<Window>, Arc<Window>>),
}

/// One frame's inputs that both presentation paths share. Keeping the view,
/// clock, and motion values in one owned struct lets the CPU and GPU paths use
/// the exact same scene construction without duplicating borrow-heavy setup.
struct FrameState {
  view: View,
  zoned: jiff::Zoned,
  warning: Option<String>,
  started: Instant,
  frame_now: Instant,
  scale: f32,
  motion: MotionFrame,
  background_color: Rgb,
  /// Pointer position in physical pixels, or `None` when it is outside.
  cursor: Option<(f32, f32)>,
  /// Window size in physical pixels, used to normalise parallax.
  screen: (f32, f32),
  /// Start of the one-shot success particle burst, if one is running.
  burst_at: Option<Instant>,
  /// Dev HUD for this frame, built only when the studio is visible.
  hud: Option<Hud>,
}

impl FrameState {
  fn scene<'a>(
    &'a self,
    app: &'a App,
    monitor: Option<&'a str>,
    drag: &'a Drag,
    post_gpu: bool,
  ) -> Scene<'a> {
    Scene {
      view: &self.view,
      theme: app.theme(),
      plugins: app.plugins(),
      now: &self.zoned,
      started: self.started,
      frame_now: self.frame_now,
      scale: self.scale,
      motion: self.motion,
      monitor,
      background_color: self.background_color,
      asset_warning: self.warning.as_deref(),
      drag,
      cursor: self.cursor,
      screen: self.screen,
      burst_at: self.burst_at,
      hud: self.hud.as_ref(),
      splash_art: app.splash_art(),
      post_gpu,
    }
  }
}

struct Handler {
  app: App,
  background: BackgroundView,
  glass: GlassCache,
  decor: DecorCache,
  text: TextEngine,
  /// Adapter and fallback policy for the live window. Screenshots never read
  /// this; they always use the CPU renderer.
  gpu: GpuConfig,
  fullscreen: bool,
  /// Initial window size in logical pixels. Normal runs use the historical
  /// 1280x800; the studio can override it with `--size`.
  initial_size: (u32, u32),
  window: Option<Arc<Window>>,
  presenter: Option<Presenter>,
  /// Reused between CPU frames. Allocating a full-screen pixmap at 30 fps was
  /// measurable, and the background paint always covers every pixel.
  frame_canvas: Option<Canvas>,
  /// Transparent UI overlay for the GPU compositor. It is rebuilt only when a
  /// hit region, text run, or animation frame actually changes.
  ui_canvas: Option<Canvas>,
  /// Background snapshot used as the glass blur input. Painting it is the one
  /// full-screen CPU operation left in the GPU path, and only at the configured
  /// blur refresh cadence.
  blur_canvas: Option<Canvas>,
  /// True when the next GPU frame must re-rasterise the UI overlay.
  ui_dirty: bool,
  /// Last time a fresh blur source was painted. Keeps the CPU blur cadence
  /// identical to `theme.blur.refresh` instead of repainting at video rate.
  blur_source_at: Option<Instant>,
  /// Instant the UI started, used to phase decor motion. Stored once so every
  /// frame and both presentation paths sample the same animation clock.
  started: Instant,
  /// Last time the UI overlay was actually rasterised. Decor-only animation
  /// uses this to cap its refresh rate without slowing focus or button motion.
  last_overlay_at: Option<Instant>,
  /// Last animation frame request. The event loop uses this instead of
  /// requesting a redraw from every `about_to_wait`, which would spin at
  /// whatever rate winit can deliver events.
  last_frame_at: Instant,
  /// The next animation frame deadline, or `None` while nothing animates.
  next_frame_at: Option<Instant>,
  monitor: Option<String>,
  modifiers: ModifiersState,
  finish: Option<Finish>,
  error: Option<WindowError>,
  proxy: EventLoopProxy<UserEvent>,
  /// Kept alive so the media workers live exactly as long as the window.
  _media: Vec<MediaHandle>,
  idle_pause: Duration,
  last_input: Instant,
  paused: bool,
  /// Background frame that was last presented. Comparing it in
  /// `about_to_wait` is what actually advances an animation; setting a
  /// control-flow deadline alone only wakes the loop, it does not paint.
  last_frame_id: u64,
  /// Identity of the splash pixmap already uploaded to the GPU UI texture.
  /// `None` means the next covering splash frame uploads it again.
  splash_ui: Option<u64>,
  /// Previous frame's splash state. The transition back to the login form
  /// needs one forced overlay rebuild, even though nothing else moved.
  splash_was_active: bool,
  /// Cadence diagnostics. The UI redraw rate and the unique wallpaper frame
  /// rate are different numbers: a decor animation can redraw the overlay
  /// without a new video frame, and a slow present can skip video frames.
  frame_window: Instant,
  redraw_frames: u32,
  wallpaper_frames: u32,
  /// Drag offsets and the hit regions from the last painted frame.
  drag: Drag,
  regions: Vec<HitRegion>,
  cursor: (f32, f32),
  /// False until the pointer actually enters. Without it the first redraw
  /// would test (0, 0) and could mark a widget hovered before any mouse event.
  pointer_inside: bool,
  /// Coalesces wallpaper wakeups. Without it a slow paint can queue one event
  /// per decoded frame and fall further behind the video.
  frame_pending: Arc<AtomicBool>,
  /// The optional cut sequence. `None` is both "never started" and "finished",
  /// so the normal paint path is the default at every point in time.
  transition: Option<Transition>,
  /// Start of the one-shot success particle burst. Cleared implicitly by age;
  /// keeping the instant rather than a spawn table is what makes the burst
  /// spawn exactly once per success transition.
  burst_at: Option<Instant>,
  /// While this is in the future the window holds the last presented frame and
  /// does not repaint. The event loop still wakes for auth and input, and the
  /// success case is clamped so it can never outlive `MAX_SUCCESS_HOLD`.
  hitstop_until: Option<Instant>,
  /// Present only in `dawn dev`.
  dev: Option<DevRuntime>,
}

/// Parses a media button hit region id.
///
/// The renderer builds these ids as `media:<index>:<action>`, where the index
/// is the position in `App::media_states()`. Anything else returns `None`, so a
/// malformed or hostile id simply falls through to the rest of the click
/// policy. The index is allowed to be any `usize` here; the caller checks it
/// against the live handle list, which is what makes an out-of-range id safely
/// do nothing instead of panicking.
fn media_control_id(id: &str) -> Option<(usize, MediaControl)> {
  let rest = id.strip_prefix("media:")?;
  let (index, action) = rest.split_once(':')?;
  if action.contains(':') {
    return None;
  }
  let index = index.parse::<usize>().ok()?;
  let control = match action {
    "prev" => MediaControl::Previous,
    "play" => MediaControl::PlayPause,
    "next" => MediaControl::Next,
    _ => return None,
  };
  Some((index, control))
}

fn translate_key(event: &KeyEvent, modifiers: ModifiersState) -> Option<Key> {
  if event.state != ElementState::Pressed {
    return None;
  }
  match &event.logical_key {
    WinitKey::Named(NamedKey::Enter) => Some(Key::Enter),
    WinitKey::Named(NamedKey::Tab) => Some(if modifiers.shift_key() {
      Key::BackTab
    } else {
      Key::Tab
    }),
    WinitKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
    WinitKey::Named(NamedKey::Delete) => Some(Key::Delete),
    WinitKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
    WinitKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
    WinitKey::Named(NamedKey::ArrowUp) => Some(Key::Up),
    WinitKey::Named(NamedKey::ArrowDown) => Some(Key::Down),
    WinitKey::Named(NamedKey::Home) => Some(Key::Home),
    WinitKey::Named(NamedKey::End) => Some(Key::End),
    WinitKey::Named(NamedKey::Escape) => Some(Key::Escape),
    WinitKey::Named(NamedKey::Space) => Some(Key::Char(' ')),
    WinitKey::Character(_) => event
      .text
      .as_ref()
      .and_then(|text| text.chars().next())
      .filter(|ch| !ch.is_control())
      .map(Key::Char),
    _ => None,
  }
}
