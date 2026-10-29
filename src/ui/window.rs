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
use crate::config::Config;
use crate::plugin::Plugin;
use crate::session::Session;
use crate::ui::background::{BackgroundView, LoadedBackground};
use crate::ui::canvas::Canvas;
use crate::ui::drag::{Drag, HitRegion};
use crate::ui::glass::GlassCache;
use crate::ui::gpu;
use crate::ui::media::{self, MediaHandle};
use crate::ui::motion::MotionFrame;
use crate::ui::render::{self, Scene};
use crate::ui::text::TextEngine;
use crate::ui::view::{Focus, View};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 800;
/// Motion is capped at roughly 60 frames per second. The login screen does not
/// need more, and a software renderer pays for every extra frame.
const MOTION_FRAME: Duration = Duration::from_millis(16);
const CLOCK_TICK: Duration = Duration::from_secs(1);

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
  let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
  let proxy = event_loop.create_proxy();
  let auth_proxy = proxy.clone();
  let mode = if dry_run {
    Mode::DryRun
  } else {
    Mode::Greetd {
      socket: config.auth.socket.clone(),
      timeout: config.auth.timeout,
    }
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
    fullscreen,
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
  };
  event_loop.run_app(&mut handler)?;

  if let Some(error) = handler.error {
    return Err(error);
  }
  handler.finish.ok_or(WindowError::Closed)
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
    }
  }
}

struct Handler {
  app: App,
  background: BackgroundView,
  glass: GlassCache,
  text: TextEngine,
  fullscreen: bool,
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
}

impl Handler {
  fn init_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowError> {
    let mut attributes = Window::default_attributes()
      .with_title("dawn")
      .with_inner_size(LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT));
    if self.fullscreen {
      attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }

    let window = Arc::new(
      event_loop
        .create_window(attributes)
        .map_err(|error| WindowError::Graphics(error.to_string()))?,
    );
    let size = window.inner_size();
    // A developer escape hatch for comparing the two paths and for drivers
    // that report a device but render incorrectly. Normal users should leave
    // it unset.
    let force_cpu = std::env::var_os("DAWN_GPU").is_some_and(|value| value == "0");
    let presenter = if force_cpu {
      tracing::info!("GPU compositor disabled by DAWN_GPU=0");
      Self::cpu_presenter(&window)?
    } else {
      match gpu::GpuRenderer::new(
        Arc::clone(&window),
        event_loop.owned_display_handle(),
        (size.width, size.height),
      ) {
        Ok(renderer) => {
          tracing::info!("using GPU compositor");
          Presenter::Gpu(Box::new(renderer))
        }
        Err(error) => {
          tracing::warn!(%error, "GPU compositor unavailable, using software presentation");
          Self::cpu_presenter(&window)?
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

  fn maybe_finish(&mut self, event_loop: &ActiveEventLoop) {
    if let Some(finish) = self.app.finished() {
      self.finish = Some(finish);
      event_loop.exit();
    }
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
    FrameState {
      view,
      zoned,
      warning,
      started: self.started,
      frame_now,
      scale,
      motion,
      background_color,
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
    let snapshot = self.background.snapshot(state.frame_now);
    let mut ui_dirty = self.ui_dirty || resized;
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
        let changed = self.glass.prepare(
          canvas,
          snapshot.frame_id,
          state.frame_now,
          theme,
          state.scale,
          snapshot.animated,
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
      self.regions = render::paint_overlay(canvas, &mut self.text, &self.glass, &scene);
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

  fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
    let Some(key) = translate_key(&event, self.modifiers) else {
      return;
    };
    self.last_input = Instant::now();
    if self.paused {
      self.paused = false;
      self.background.set_paused(false);
    }
    self.app.key(key);
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
  }

  fn pointer_press(&mut self, event_loop: &ActiveEventLoop) {
    let Some(window) = &self.window else {
      return;
    };
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
      self.app.key(Key::Enter);
      self.request_redraw();
      self.maybe_finish(event_loop);
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
        self.request_redraw();
      }
      Err(error) => self.fail(event_loop, error),
    }
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
      UserEvent::Auth(event) => {
        self.app.handle_auth(event);
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
    if let Some(finish) = self.app.finished() {
      self.finish = Some(finish);
      event_loop.exit();
      return;
    }

    let now = Instant::now();
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
