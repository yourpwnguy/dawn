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
use crate::auth::worker::{self, Event, Mode};
use crate::color::Rgb;
use crate::config::{Config, GpuBackend, GpuConfig, GpuDevice, GpuFallback, TransitionTrigger};
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
use crate::ui::render::{self, Scene};
use crate::ui::text::TextEngine;
use crate::ui::transition::{self, Transition};
use crate::ui::view::{Focus, View};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 800;
/// Motion is capped at roughly 60 frames per second. The login screen does not
/// need more, and a software renderer pays for every extra frame.
const MOTION_FRAME: Duration = Duration::from_millis(16);
const CLOCK_TICK: Duration = Duration::from_secs(1);
/// Hard ceiling on how long a successful auth may hold the window open just to
/// finish the cut sequence. Auth is already done at that point; this only
/// delays the process exit, and a pathological `slam_ms` must not turn a login
/// into a two-minute show. Anything past this exits immediately.
const MAX_SUCCESS_HOLD: Duration = Duration::from_millis(2000);

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
  )
}

/// The dev studio's window entry point.
///
/// There is no `dry_run` parameter: the auth mode is built here as
/// [`AuthMode::DryRun`], and [`run_with`] never derives it from the config.
/// The window is always windowed and always uses the requested size.
pub fn run_dev(
  config: Config,
  sessions: Vec<Session>,
  plugins: Vec<Plugin>,
  warning: Option<String>,
  size: (u32, u32),
  source: Source,
  hud_enabled: bool,
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
      source,
      hud_enabled,
    }),
  )
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
  let app = App::new(config, sessions, plugins, Some(worker), warning, dry_run);

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
    drag,
    regions: Vec::new(),
    cursor: (0.0, 0.0),
    pointer_inside: false,
    frame_pending,
    transition: None,
    burst_at: None,
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
  fn scene<'a>(&'a self, app: &'a App, monitor: Option<&'a str>, drag: &'a Drag) -> Scene<'a> {
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
    }
  }
}

struct Handler {
  app: App,
  background: BackgroundView,
  glass: GlassCache,
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

impl Handler {
  fn init_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowError> {
    let (width, height) = self.initial_size;
    let mut attributes = Window::default_attributes()
      .with_title(self.window_title())
      .with_inner_size(LogicalSize::new(width, height));
    if self.fullscreen {
      attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }

    let window = Arc::new(
      event_loop
        .create_window(attributes)
        .map_err(|error| WindowError::Graphics(error.to_string()))?,
    );
    let size = window.inner_size();
    let presenter = if !self.gpu.enabled {
      tracing::info!("GPU compositor disabled in [gpu]");
      Self::cpu_presenter(&window)?
    } else if self.gpu.fallback == GpuFallback::Cpu {
      tracing::info!("GPU compositor disabled by gpu.fallback = \"cpu\"");
      Self::cpu_presenter(&window)?
    } else {
      match gpu::GpuRenderer::new(
        Arc::clone(&window),
        event_loop.owned_display_handle(),
        (size.width, size.height),
        &self.gpu,
      ) {
        Ok(renderer) => {
          tracing::info!("using GPU compositor");
          Presenter::Gpu(Box::new(renderer))
        }
        Err(error) => {
          tracing::warn!(%error, "GPU compositor unavailable");
          self.gpu_presenter_after_failure(&window, event_loop, size, error)?
        }
      }
    };

    self.monitor = window.current_monitor().and_then(|monitor| monitor.name());
    self.window = Some(window);
    self.presenter = Some(presenter);
    self.ui_dirty = true;
    Ok(())
  }

  fn cpu_presenter(window: &Arc<Window>) -> Result<Presenter, WindowError> {
    let context = softbuffer::Context::new(Arc::clone(window))
      .map_err(|error| WindowError::Graphics(error.to_string()))?;
    let surface = softbuffer::Surface::new(&context, Arc::clone(window))
      .map_err(|error| WindowError::Graphics(error.to_string()))?;
    Ok(Presenter::Cpu(surface))
  }

  /// Applies `[gpu].fallback` after the configured adapter failed.
  ///
  /// The software retry rebuilds a copy with `backend = "software"`,
  /// `device = "cpu"`, and `adapter = "auto"` on purpose. A name or index that
  /// only made sense for the adapter that just failed should not stop the last
  /// chance from trying lavapipe or llvmpipe. `chain` then degrades to
  /// softbuffer; `software` and `none` return an error so main can reach the
  /// terminal fallback.
  fn gpu_presenter_after_failure(
    &self,
    window: &Arc<Window>,
    event_loop: &ActiveEventLoop,
    size: winit::dpi::PhysicalSize<u32>,
    error: String,
  ) -> Result<Presenter, WindowError> {
    if matches!(
      self.gpu.fallback,
      GpuFallback::Chain | GpuFallback::Software
    ) {
      let mut fallback = self.gpu.clone();
      fallback.backend = GpuBackend::Software;
      fallback.device = GpuDevice::Cpu;
      fallback.adapter = "auto".to_string();
      match gpu::GpuRenderer::new(
        Arc::clone(window),
        event_loop.owned_display_handle(),
        (size.width, size.height),
        &fallback,
      ) {
        Ok(renderer) => {
          tracing::info!("using software GPU compositor");
          return Ok(Presenter::Gpu(Box::new(renderer)));
        }
        Err(software_error) => {
          if self.gpu.fallback == GpuFallback::Chain {
            tracing::warn!(
              %software_error,
              "software GPU compositor unavailable, using softbuffer"
            );
            return Self::cpu_presenter(window);
          }
          return Err(WindowError::Graphics(format!(
            "GPU compositor failed ({error}); software retry failed ({software_error})"
          )));
        }
      }
    }
    if self.gpu.fallback == GpuFallback::Cpu {
      // `cpu` is handled before the first attempt; keep this defensive branch
      // so a future call site cannot accidentally try a GPU here.
      tracing::info!("GPU compositor disabled by gpu.fallback = \"cpu\"");
      return Self::cpu_presenter(window);
    }
    Err(WindowError::Graphics(error))
  }

  /// Refreshes the cached output name. `current_monitor` is a round trip on
  /// some backends, so it is not called once per frame.
  fn update_monitor(&mut self) {
    self.monitor = self
      .window
      .as_ref()
      .and_then(|window| window.current_monitor())
      .and_then(|monitor| monitor.name());
  }

  fn request_redraw(&self) {
    if let Some(window) = &self.window {
      window.request_redraw();
    }
  }

  /// Normal runs keep the short historical title. The studio names its source
  /// so two dev windows are distinguishable in a task switcher.
  fn window_title(&self) -> String {
    match &self.dev {
      Some(dev) => format!("dawn dev {}", dev.source.label),
      None => "dawn".to_string(),
    }
  }

  fn maybe_finish(&mut self, event_loop: &ActiveEventLoop) {
    let Some(finish) = self.app.finished() else {
      return;
    };
    // A `success` transition is the only reason a finished login waits. Auth
    // and the session launch are already committed by this point; this hold
    // only delays the process exit, and only for the capped duration. The
    // default `enter` trigger never reaches this branch.
    if finish == Finish::Started && self.transition_holds(Instant::now()) {
      self.request_redraw();
      return;
    }
    self.finish = Some(finish);
    event_loop.exit();
  }

  /// True while a success-triggered sequence is still worth painting. The
  /// `MAX_SUCCESS_HOLD` clamp is the lockout guard: even if a config asks for a
  /// ten-second slam, the greeter leaves for the session after two seconds.
  fn transition_holds(&self, now: Instant) -> bool {
    self.transition.as_ref().is_some_and(|transition| {
      transition.trigger().accepts(TransitionTrigger::Success)
        && !transition.finished(now)
        && transition.elapsed(now) < MAX_SUCCESS_HOLD
    })
  }

  /// Starts the configured sequence for `moment`, if this table accepts that
  /// moment and nothing is already running. The auth request has already left
  /// the UI thread when this is called; nothing here talks to the worker.
  fn start_transition(&mut self, moment: TransitionTrigger, now: Instant) {
    let config = self.app.theme().login.transition.clone();
    let Some(started) = Transition::start(&config, now) else {
      return;
    };
    if !started.trigger().accepts(moment) {
      return;
    }
    if self
      .transition
      .as_ref()
      .is_some_and(|active| !active.finished(now))
    {
      return;
    }
    self.transition = Some(started);
    self.ui_dirty = true;
    self.request_redraw();
  }

  /// Drops a finished sequence and asks for a normal frame. Returns true when
  /// something changed, so the caller can re-check a pending finish.
  fn retire_transition(&mut self, now: Instant) -> bool {
    if self
      .transition
      .as_ref()
      .is_some_and(|transition| transition.finished(now))
    {
      self.transition = None;
      self.ui_dirty = true;
      self.request_redraw();
      return true;
    }
    false
  }

  fn fail(&mut self, event_loop: &ActiveEventLoop, error: WindowError) {
    tracing::error!(%error, "window frontend failed");
    self.error = Some(error);
    event_loop.exit();
  }

  /// Asks the background view for a load and hands it to a thread. The view
  /// deduplicates by size and output, so calling this on every resize is safe.
  fn start_background_load(&mut self) {
    let Some(window) = &self.window else {
      return;
    };
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 {
      return;
    }
    let Some(request) = self
      .background
      .request((size.width, size.height), self.monitor.as_deref())
    else {
      return;
    };
    let proxy = self.proxy.clone();
    let spawned = std::thread::Builder::new()
      .name("dawn-background".to_string())
      .spawn(move || {
        let (generation, result) = request.load();
        let _ = proxy.send_event(UserEvent::Background(generation, result));
      });
    if let Err(error) = spawned {
      self
        .background
        .fail(format!("cannot start the loader thread: {error}"));
      self.ui_dirty = true;
      self.request_redraw();
    }
  }

  fn redraw(&mut self) {
    let Some(window) = self.window.clone() else {
      return;
    };
    let Some(mut presenter) = self.presenter.take() else {
      return;
    };
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 {
      // A zero-sized surface happens during resize. Skipping the frame is
      // correct; winit will ask again once the window has a size.
      self.presenter = Some(presenter);
      return;
    }
    let result = match &mut presenter {
      Presenter::Gpu(renderer) => self.redraw_gpu(renderer, &window, (size.width, size.height)),
      Presenter::Cpu(surface) => self.redraw_cpu(surface, &window, (size.width, size.height)),
    };
    self.presenter = Some(presenter);
    if let Err(error) = result {
      tracing::error!(%error, "cannot present the frame");
      self.error = Some(WindowError::Graphics(error));
    }
  }

  /// Builds the per-frame view and starts the motion clock. Both presentation
  /// paths call this once per redraw so they cannot disagree about boot timing
  /// or widget values.
  fn frame_state(&mut self, window: &Window) -> FrameState {
    let scale = window.scale_factor() as f32;
    let frame_now = Instant::now();
    let hud = match self.dev.as_mut() {
      Some(dev) => {
        dev.fps.tick(frame_now);
        dev.hud()
      }
      None => None,
    };
    let reduced = self.app.theme().motion.reduced || self.app.theme().low_power;
    self.drag.advance(frame_now, reduced);
    let motion = self.app.motion_frame(frame_now, scale);
    let background_color = self
      .background
      .config()
      .color_for_monitor(self.monitor.as_deref());
    let view = self.app.view();
    let zoned = jiff::Zoned::now();
    let warning = self.background.warning().map(str::to_string);
    let size = window.inner_size();
    FrameState {
      view,
      zoned,
      warning,
      started: self.started,
      frame_now,
      scale,
      motion,
      background_color,
      cursor: self.pointer_inside.then_some(self.cursor),
      screen: (size.width as f32, size.height as f32),
      burst_at: self.burst_at,
      hud,
    }
  }

  /// The original full CPU path. It is both the no-GPU fallback and the path
  /// `--screenshot` exercises, so any visual change must keep working here.
  fn redraw_cpu(
    &mut self,
    surface: &mut softbuffer::Surface<Arc<Window>, Arc<Window>>,
    window: &Window,
    size: (u32, u32),
  ) -> Result<(), String> {
    let (Some(width), Some(height)) = (NonZeroU32::new(size.0), NonZeroU32::new(size.1)) else {
      return Ok(());
    };
    let reusable = self
      .frame_canvas
      .as_ref()
      .is_some_and(|canvas| canvas.width() == width.get() && canvas.height() == height.get());
    if !reusable {
      self.frame_canvas = Canvas::new(width.get(), height.get());
    }
    let state = self.frame_state(window);
    let scene = state.scene(&self.app, self.monitor.as_deref(), &self.drag);
    let transition_frame = self
      .transition
      .as_ref()
      .and_then(|transition| transition.frame(state.frame_now));
    let Some(canvas) = self.frame_canvas.as_mut() else {
      return Err("cannot allocate the frame canvas".to_string());
    };
    self.regions = render::paint(
      canvas,
      &mut self.text,
      &mut self.background,
      &mut self.glass,
      &scene,
    );
    // The cut sequence is a post-pass over the finished frame: opaque during
    // the cuts, translucent during the release. The CPU path has nowhere else
    // to put a wallpaper-covering flash.
    if let Some(frame) = &transition_frame {
      transition::paint(canvas, &mut self.text, frame, state.scale);
    }
    self.last_overlay_at = Some(Instant::now());
    self.last_frame_id = self.background.frame_id(state.frame_now);
    // A rebuilt region list can put a different widget under a stationary
    // pointer (a dragged card, a changed field height). Re-evaluate hover so
    // the button transition does not stay stuck on its old target.
    if self.pointer_inside {
      self.drag.hover(&self.regions, Some(self.cursor));
    }

    surface
      .resize(width, height)
      .map_err(|error| error.to_string())?;
    let mut buffer = surface.buffer_mut().map_err(|error| error.to_string())?;
    canvas.to_argb(&mut buffer[..]);
    buffer.present().map_err(|error| error.to_string())
  }

  /// GPU compositor path: upload the wallpaper, rasterise the transparent UI
  /// overlay only when something in it changed, and present one triangle.
  fn redraw_gpu(
    &mut self,
    renderer: &mut gpu::GpuRenderer,
    window: &Window,
    size: (u32, u32),
  ) -> Result<(), String> {
    if size.0 == 0 || size.1 == 0 {
      return Ok(());
    }
    let resized = renderer.resize(size)?;
    let state = self.frame_state(window);
    let scene = state.scene(&self.app, self.monitor.as_deref(), &self.drag);
    let transition_frame = self
      .transition
      .as_ref()
      .and_then(|transition| transition.frame(state.frame_now));
    // An opaque transition frame is a full-screen flash. There is no reason to
    // rasterise the login overlay underneath it, so skip that work and keep
    // the hit regions empty for the duration. Translucent release frames need
    // the real UI because they reveal it.
    let transition_covers = transition_frame
      .as_ref()
      .is_some_and(|frame| frame.fill.a == 255);
    let snapshot = self.background.snapshot(state.frame_now);
    let mut ui_dirty =
      self.ui_dirty || resized || transition_frame.is_some() || self.burst_active(state.frame_now);
    let theme = self.app.theme();

    // Glass panels are the one thing that still needs CPU background pixels.
    // Keep the same refresh cadence as the CPU blur cache instead of repainting
    // the wallpaper on every video frame.
    if theme.blur.enabled || theme.noise {
      let needs_canvas = self
        .blur_canvas
        .as_ref()
        .is_none_or(|canvas| canvas.width() != size.0 || canvas.height() != size.1);
      if needs_canvas {
        self.blur_canvas = Canvas::new(size.0, size.1);
        self.blur_source_at = None;
        ui_dirty = true;
      }
      if theme.blur.enabled {
        let due = self
          .blur_source_at
          .is_none_or(|at| state.frame_now.duration_since(at) >= theme.blur.refresh);
        if due
          && (self.glass.cached_frame() != snapshot.frame_id || !self.glass.has_blur())
          && let Some(canvas) = self.blur_canvas.as_mut()
        {
          self.background.paint_snapshot(canvas, state.frame_now);
          self.blur_source_at = Some(state.frame_now);
        }
      }
      if let Some(canvas) = self.blur_canvas.as_ref() {
        // The GPU renderer owns the blur pipeline, so the cache asks it first.
        // A failure returns `None` and the cache falls back to the CPU worker
        // exactly as before.
        let changed = self.glass.prepare_with(
          canvas,
          snapshot.frame_id,
          state.frame_now,
          theme,
          state.scale,
          snapshot.animated,
          |source, blur, scale| renderer.blur(source, blur, scale).ok(),
        );
        ui_dirty |= changed;
      }
    }

    if ui_dirty {
      let needs_canvas = self
        .ui_canvas
        .as_ref()
        .is_none_or(|canvas| canvas.width() != size.0 || canvas.height() != size.1);
      if needs_canvas {
        self.ui_canvas = Canvas::new(size.0, size.1);
      }
      let Some(canvas) = self.ui_canvas.as_mut() else {
        return Err("cannot allocate the UI overlay".to_string());
      };
      canvas.clear_transparent();
      if transition_covers {
        // The cut covers every pixel, so the overlay is complete without the
        // scanlines or the form. Clearing the regions also makes a stray click
        // during the flash harmless.
        self.regions.clear();
      } else {
        // The GPU path keeps the wallpaper in its own texture, so the CRT
        // overlay goes on the transparent UI layer first. The form then paints
        // over it, which keeps the same wallpaper-under-text ordering as the
        // CPU path.
        render::draw_scanlines(canvas, &scene);
        self.regions = render::paint_overlay(canvas, &mut self.text, &self.glass, &scene);
      }
      if let Some(frame) = &transition_frame {
        transition::paint(canvas, &mut self.text, frame, state.scale);
      }
      self.last_overlay_at = Some(Instant::now());
      renderer.set_ui(canvas.pixmap())?;
      if self.pointer_inside {
        self.drag.hover(&self.regions, Some(self.cursor));
      }
      self.ui_dirty = false;
    }

    renderer.set_background(&snapshot)?;
    renderer.present()?;
    self.last_frame_id = snapshot.frame_id;
    Ok(())
  }

  /// Re-reads the source and swaps the scene, or keeps the last good one.
  ///
  /// This runs on the event loop, but the expensive decode, blur, and video
  /// work does not: [`dev::prepare_reload`] only parses and compares, and a
  /// changed wallpaper is handed to the existing background loader thread.
  fn dev_reload(&mut self) {
    let Some(dev) = self.dev.as_ref() else {
      return;
    };
    let source = dev.source.clone();
    let host_gpu = dev.host_gpu.clone();
    let previous = dev.state.last_good.clone();

    match dev::prepare_reload(&source, &host_gpu, &previous) {
      Ok(reload) => {
        let dev::Reload {
          config,
          plugins,
          warning,
          background,
          rebuild_text,
          cost_ms,
        } = reload;
        let swap_started = Instant::now();
        if rebuild_text {
          self.text = TextEngine::new(&config.ui.font);
        }
        if background.is_reload() {
          // `!decode` means the image path is unchanged, so the loaded frame
          // set stays in place and only the cheap background fields change.
          self.background.reconfigure(&config, !background.decode());
        }
        self.app.reload(config.clone(), plugins, warning);
        let total_ms = cost_ms + swap_started.elapsed().as_secs_f32() * 1000.0;
        if let Some(dev) = self.dev.as_mut() {
          dev.state.success(config, total_ms);
        }
        if background.is_reload() {
          self.start_background_load();
        }
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.request_redraw();
        tracing::info!(cost_ms = total_ms, "dev reload");
      }
      Err(error) => {
        let logged = self
          .dev
          .as_mut()
          .is_some_and(|dev| dev.state.failure(error.clone()));
        if logged {
          tracing::warn!(
            error = %error,
            "dev reload failed, keeping the last good theme"
          );
        }
        // A failed reload still changes the HUD error line, so repaint.
        self.ui_dirty = true;
        self.request_redraw();
      }
    }
  }

  fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
    // Once a login has been committed, the window may still be painting a
    // success transition. Ignore input during that hold so an Escape can never
    // cancel a session that greetd has already accepted.
    if self.app.finished().is_some() {
      return;
    }
    // Studio shortcuts are intercepted before the login key map. Escape is
    // deliberately absent: it stays the normal "clear the field" key.
    if self.dev.is_some() && event.state == ElementState::Pressed {
      match &event.logical_key {
        WinitKey::Named(NamedKey::F1) => {
          if let Some(dev) = self.dev.as_mut()
            && dev.hud_enabled
          {
            dev.hud_visible = !dev.hud_visible;
            self.ui_dirty = true;
            self.request_redraw();
          }
          return;
        }
        WinitKey::Named(NamedKey::F5) => {
          self.dev_reload();
          return;
        }
        WinitKey::Character(value)
          if self.modifiers.control_key() && value.as_str().eq_ignore_ascii_case("q") =>
        {
          self.app.quit();
          self.finish = Some(Finish::Quit);
          event_loop.exit();
          return;
        }
        _ => {}
      }
    }
    let Some(key) = translate_key(&event, self.modifiers) else {
      return;
    };
    self.last_input = Instant::now();
    if self.paused {
      self.paused = false;
      self.background.set_paused(false);
    }
    let submit_before = self.app.submit_epoch();
    self.app.key(key);
    if self.app.submit_epoch() != submit_before {
      self.start_transition(TransitionTrigger::Enter, Instant::now());
    }
    self.ui_dirty = true;
    self.request_redraw();
    self.maybe_finish(event_loop);
  }

  fn refresh_cursor(&self) {
    let Some(window) = &self.window else {
      return;
    };
    let icon = if self.drag.is_dragging() {
      CursorIcon::Grabbing
    } else if self.drag.hover_is_draggable() {
      CursorIcon::Grab
    } else {
      CursorIcon::Default
    };
    window.set_cursor(icon);
  }

  fn pointer_move(&mut self, cursor: (f32, f32)) {
    self.cursor = cursor;
    self.pointer_inside = true;
    self.last_input = Instant::now();
    let Some(window) = &self.window else {
      return;
    };
    let size = window.inner_size();
    if self.drag.is_dragging() {
      if self
        .drag
        .move_to(cursor, (size.width as f32, size.height as f32))
      {
        self.ui_dirty = true;
        self.request_redraw();
      }
    } else if self.drag.hover(&self.regions, Some(cursor)) {
      self.ui_dirty = true;
      self.refresh_cursor();
    }
    // Parallax follows the pointer even when nothing else changed, so a static
    // login screen still needs a fresh overlay frame on every move. Hover
    // already asked for one above, but the guard keeps this path cheap when
    // the effect is off.
    if self.app.theme().motion.parallax > 0.0 {
      self.ui_dirty = true;
      self.request_redraw();
    }
  }

  fn pointer_press(&mut self, event_loop: &ActiveEventLoop) {
    let Some(window) = &self.window else {
      return;
    };
    if self.app.finished().is_some() {
      return;
    }
    let scale = window.scale_factor() as f32;
    self.last_input = Instant::now();
    self.ui_dirty = true;
    if self.drag.begin(&self.regions, self.cursor, scale) {
      self.refresh_cursor();
      self.request_redraw();
      return;
    }
    // A non-draggable field still responds to a click by taking focus.
    let id = self
      .regions
      .iter()
      .rev()
      .find(|region| region.contains(self.cursor))
      .map(|region| region.id.as_str());
    // A click on the submit button is the pointer equivalent of Enter. It
    // goes through the same key policy, so an empty username still moves focus
    // to the first field instead of starting an auth request.
    if id == Some("login.button") {
      let submit_before = self.app.submit_epoch();
      self.app.key(Key::Enter);
      if self.app.submit_epoch() != submit_before {
        self.start_transition(TransitionTrigger::Enter, Instant::now());
      }
      self.request_redraw();
      self.maybe_finish(event_loop);
      return;
    }
    // Media transport buttons are ordinary hit regions. We parse the id rather
    // than comparing strings so a malformed id cannot be mistaken for a field,
    // and an index that no longer has a live handle simply does nothing.
    if let Some((index, control)) = id.and_then(media_control_id) {
      if let Some(handle) = self._media.get(index) {
        // A dropped worker is the only reason this can fail, and the next
        // snapshot wake-up will repaint anyway. The click still belongs to the
        // widget, so we return instead of falling through to field focus.
        let _ = handle.control(control);
        self.ui_dirty = true;
        self.request_redraw();
      }
      return;
    }
    let focused = match id {
      Some("login.username") => self.app.focus_field(Focus::Username),
      Some("login.password") => self.app.focus_field(Focus::Password),
      _ => false,
    };
    if focused {
      self.request_redraw();
    }
  }

  fn pointer_release(&mut self) {
    if self.drag.end() {
      self.drag.save();
      self.refresh_cursor();
      self.ui_dirty = true;
      self.request_redraw();
    }
  }

  fn pointer_leave(&mut self) {
    self.pointer_inside = false;
    if self.drag.hover(&self.regions, None) {
      self.ui_dirty = true;
      self.refresh_cursor();
    }
    if self.drag.end() {
      self.drag.save();
      self.refresh_cursor();
      self.ui_dirty = true;
      self.request_redraw();
    }
  }

  /// True while the submit button needs a frame. A looping effect keeps
  /// asking for 60 fps, but only while the pointer is actually on the button,
  /// so an idle greeter stays cheap.
  fn button_hover_animating(&self, now: Instant) -> bool {
    let button = &self.app.theme().login.button;
    if !button.show || !button.has_motion() {
      return false;
    }
    let duration = Duration::from_millis(button.hover_ms.max(0.0) as u64);
    let looping =
      button.effect != crate::config::ButtonEffect::None && self.drag.is_hovered("login.button");
    looping
      || self
        .drag
        .hover_transition_active("login.button", now, duration)
  }

  /// True while the success burst is still painting. The burst has no mutable
  /// particles: it is a pure function of age, so "active" is just a time
  /// window.
  fn burst_active(&self, now: Instant) -> bool {
    self.burst_at.is_some_and(|at| {
      now.saturating_duration_since(at).as_secs_f32() < crate::config::BURST_LIFETIME
    })
  }

  /// Chooses the next wake-up: the earliest of the clock tick, a wallpaper
  /// frame, and a motion frame. When everything is static this is a plain
  /// one-second wait.
  fn schedule(&self, event_loop: &ActiveEventLoop) {
    let now = Instant::now();
    let mut next = now + CLOCK_TICK;
    if let Some(deadline) = self.background.next_deadline(now) {
      next = next.min(deadline);
    }
    if self.app.motion_active() && !self.paused {
      next = next.min(now + MOTION_FRAME);
    }
    if (self.drag.is_animating() || self.button_hover_animating(now)) && !self.paused {
      next = next.min(now + MOTION_FRAME);
    }
    // The cut sequence is one-shot and short. It drives frames even when the
    // window is unfocused so a success hold cannot depend on input to finish.
    if self
      .transition
      .as_ref()
      .is_some_and(|transition| !transition.finished(now))
    {
      next = next.min(now + MOTION_FRAME);
    }
    if self.burst_active(now) {
      next = next.min(now + MOTION_FRAME);
    }
    if self.app.theme().decor_motion_active() && !self.paused {
      let interval = self.app.theme().motion.decor_frame();
      let due = self
        .last_overlay_at
        .map(|at| at + interval)
        .unwrap_or(now)
        .max(now);
      next = next.min(due);
    }
    event_loop.set_control_flow(ControlFlow::WaitUntil(next));
  }
}

impl ApplicationHandler<UserEvent> for Handler {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    if self.window.is_some() {
      return;
    }
    match self.init_window(event_loop) {
      Ok(()) => {
        self.start_background_load();
        // The boot cut is a one-shot montage intro. It is not tied to auth and
        // cannot hold the process open, because only a success trigger is
        // allowed to delay exit. `resumed` returns early once a window exists,
        // so this fires exactly once.
        self.start_transition(TransitionTrigger::Boot, Instant::now());
        self.request_redraw();
      }
      Err(error) => self.fail(event_loop, error),
    }
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
      UserEvent::Auth(event) => {
        let failed = matches!(event, Event::Failed(_));
        let was_authenticated = self.app.authenticated();
        self.app.handle_auth(event);
        let now = Instant::now();
        if failed {
          // A rejection gets its own optional cut. The form is already back in
          // its typing state; this is decoration on the same frame, never a
          // hold on the auth path.
          self.start_transition(TransitionTrigger::Failure, now);
        }
        if !was_authenticated && self.app.authenticated() {
          if self.app.theme().particles.burst_on == crate::config::BurstTrigger::Success {
            self.burst_at = Some(now);
          }
          self.start_transition(TransitionTrigger::Success, now);
        }
        self.ui_dirty = true;
        self.request_redraw();
        self.maybe_finish(event_loop);
      }
      UserEvent::Background(generation, result) => {
        let accepted = self.background.accept(generation, result, Instant::now());
        if accepted {
          let proxy = self.proxy.clone();
          let pending = Arc::clone(&self.frame_pending);
          self.background.set_waker(Some(Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
              let _ = proxy.send_event(UserEvent::Frame);
            }
          })));
        }
        // A failed load still changes the warning line, so repaint either way.
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.request_redraw();
      }
      UserEvent::Media => {
        self.ui_dirty = true;
        self.request_redraw();
      }
      UserEvent::Frame => {
        self.frame_pending.store(false, Ordering::Release);
        self.request_redraw();
      }
      UserEvent::DevReload => self.dev_reload(),
    }
  }

  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    _window_id: WindowId,
    event: WindowEvent,
  ) {
    match event {
      WindowEvent::CloseRequested => {
        self.app.quit();
        self.finish = Some(Finish::Quit);
        event_loop.exit();
      }
      WindowEvent::Destroyed => {
        self.finish.get_or_insert(Finish::Quit);
        event_loop.exit();
      }
      WindowEvent::Resized(_) => {
        self.update_monitor();
        self.start_background_load();
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.request_redraw();
      }
      WindowEvent::ScaleFactorChanged { .. } => {
        self.update_monitor();
        self.start_background_load();
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.request_redraw();
      }
      WindowEvent::Focused(true) => {
        self.last_input = Instant::now();
        self.ui_dirty = true;
        if self.paused {
          self.paused = false;
          self.background.set_paused(false);
          // Resume visibly: the frozen frame may be seconds behind, and an
          // animated wallpaper would otherwise wait up to one frame delay
          // before the next scheduled repaint catches up.
          self.request_redraw();
        }
      }
      WindowEvent::Focused(false) => {
        // A hidden greeter has no reason to animate a wallpaper. Freezing it
        // also means a compositor switching away does not burn CPU. We do not
        // pause on `Occluded`: X11 without a window manager reports it for a
        // fully mapped greeter, which would freeze the first wallpaper frame
        // forever.
        self.paused = true;
        self.ui_dirty = true;
        self.background.set_paused(true);
        if self.drag.end() {
          self.drag.save();
          self.refresh_cursor();
        }
      }
      WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
      WindowEvent::CursorMoved { position, .. } => {
        self.pointer_move((position.x as f32, position.y as f32));
      }
      WindowEvent::CursorLeft { .. } => self.pointer_leave(),
      WindowEvent::MouseInput {
        state,
        button: MouseButton::Left,
        ..
      } => match state {
        ElementState::Pressed => self.pointer_press(event_loop),
        ElementState::Released => self.pointer_release(),
      },
      WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
        self.handle_key(event_loop, event);
      }
      WindowEvent::RedrawRequested => self.redraw(),
      _ => {}
    }
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    // A drawing failure must stop the loop so main can try the terminal
    // fallback instead of leaving a frozen window on screen.
    if self.error.is_some() {
      event_loop.exit();
      return;
    }
    if self.app.finished().is_some() {
      self.maybe_finish(event_loop);
      if self.finish.is_some() {
        return;
      }
    }

    let now = Instant::now();
    // Advance or retire the cut sequence before scheduling. A finished
    // sequence requests one normal frame so the form, not the last
    // translucent slam, is what stays on screen.
    if self
      .transition
      .as_ref()
      .is_some_and(|transition| !transition.finished(now))
    {
      self.ui_dirty = true;
      self.request_redraw();
    }
    if self.retire_transition(now) {
      self.maybe_finish(event_loop);
      if self.finish.is_some() {
        return;
      }
    }
    if !self.paused
      && !self.idle_pause.is_zero()
      && now.duration_since(self.last_input) >= self.idle_pause
      && self.background.is_animated()
    {
      self.paused = true;
      self.background.set_paused(true);
    }

    // The clock keeps ticking even while the wallpaper is paused: an
    // unfocused greeter on a headless X server is still visible, and a frozen
    // clock looks broken. The wallpaper is safe because paint() and
    // frame_id() render the frame frozen at pause time, so the only cost
    // here is one cheap repaint per visible clock change.
    if self.app.tick(&jiff::Zoned::now()) {
      self.ui_dirty = true;
      self.request_redraw();
    }
    if !self.paused
      && (self.background.fading(now) || self.background.frame_id(now) != self.last_frame_id)
    {
      self.request_redraw();
    }
    let ui_animating =
      self.app.motion_active() || self.drag.is_animating() || self.button_hover_animating(now);
    if ui_animating && !self.paused {
      self.ui_dirty = true;
      self.request_redraw();
    }
    // Decor is ambient: it drifts and breathes rather than tracking input.
    // Rasterising the whole overlay at video rate for a slow petal would waste
    // a core, so decor-only frames use `motion.decor_fps`. Any real UI motion
    // still runs at the full motion frame rate above.
    if self.app.theme().decor_motion_active() && !self.paused {
      let due = self
        .last_overlay_at
        .map(|at| at + self.app.theme().motion.decor_frame())
        .unwrap_or(now);
      if now >= due {
        self.ui_dirty = true;
        self.request_redraw();
      }
    }
    self.schedule(event_loop);
  }
}

/// Maps a winit key press to the app's small key vocabulary. Named keys win
/// over `text`, because winit reports `"\r"` for Enter and we must not insert a
/// carriage return into a field.
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

#[cfg(test)]
mod tests {
  use super::{AuthMode, dev_auth_mode, media_control_id};
  use crate::ui::media::MediaControl;

  #[test]
  fn dev_mode_never_constructs_a_greetd_auth_mode() {
    assert!(matches!(dev_auth_mode(), AuthMode::DryRun));
  }

  #[test]
  fn media_control_ids_map_to_the_right_action() {
    assert_eq!(
      media_control_id("media:0:prev"),
      Some((0, MediaControl::Previous))
    );
    assert_eq!(
      media_control_id("media:12:play"),
      Some((12, MediaControl::PlayPause))
    );
    assert_eq!(
      media_control_id("media:3:next"),
      Some((3, MediaControl::Next))
    );
  }

  #[test]
  fn malformed_media_ids_are_ignored() {
    // Missing pieces, extra pieces, unknown actions, and values that would
    // overflow a usize all have to fall through without panicking.
    for id in [
      "",
      "media",
      "media:",
      "media:0",
      "media::play",
      "media:0:stop",
      "media:0:play:extra",
      "music:0:play",
      "media:-1:play",
      "media:abc:play",
      "media:999999999999999999999999999999:play",
    ] {
      assert_eq!(media_control_id(id), None, "{id:?} should be ignored");
    }
  }

  #[test]
  fn a_parseable_index_is_left_for_the_handle_lookup() {
    // The parser is deliberately not bound to the current handle count. A
    // valid id with a large index is still well formed; `pointer_press` drops
    // it with a checked `.get(index)` rather than treating it as malformed.
    assert_eq!(
      media_control_id("media:42:next"),
      Some((42, MediaControl::Next))
    );
  }
}
