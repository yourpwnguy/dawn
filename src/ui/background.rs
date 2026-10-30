//! The background engine: solid, gradient, still image, or animated wallpaper.
//!
//! Decoding is the expensive part and it never runs on the UI thread. The
//! window shows the configured gradient immediately, starts a loader thread,
//! and crossfades to the wallpaper when the frame set arrives. Frames are
//! decoded once, fitted to the output size, and cached as ready-to-blit
//! pixmaps. Animated playback is driven by the file's delays and capped by the
//! configured frame rate.
//!
//! A broken wallpaper is never fatal. The loader returns a readable error, the
//! view keeps its gradient, and the error is painted near the bottom of the
//! screen alongside config warnings.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use tracing::warn;

use crate::color::Rgb;
use crate::config::{BackgroundConfig, BackgroundKind, Config};
use crate::ui::canvas::Canvas;
use crate::ui::image::{self, DecodeLimits};
use crate::ui::stream::{FrameStream, LiveSource};
use crate::ui::video::{self, VideoStream};

const FADE: Duration = Duration::from_millis(320);
const LOAD_LIMITS: DecodeLimits = DecodeLimits {
  max_file_bytes: 64 * 1024 * 1024,
  max_dimension: 8192,
  max_pixels: 32 * 1024 * 1024,
  max_alloc: 256 * 1024 * 1024,
};

/// What the loader produced. `Frames` is used for both still and animated
/// images; a still image simply has one entry.
#[derive(Debug)]
pub enum Content {
  Solid(Rgb),
  Gradient(Rgb),
  Frames {
    /// Frames are shared so a renderer snapshot can hold one without copying
    /// a full-screen pixmap. The GPU path relies on that: it snapshots the
    /// background every frame but only uploads when the `Arc` actually
    /// changes.
    frames: Vec<Arc<Pixmap>>,
    delays: Vec<Duration>,
    cumulative: Vec<Duration>,
  },
  /// A live GIF, WebP, or video decoder. Only the current frame is resident,
  /// so long or full-HD animations do not need a predecoded frame cache.
  Stream(Arc<dyn LiveSource>),
}

/// What the current background is made of. The GPU shader uses this instead of
/// guessing whether a texture is bound, and the blur snapshot uses it to paint
/// the same placeholder the CPU path would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
  Image,
  Solid(Rgb),
  Gradient(Rgb),
}

/// A borrowing-free view of one background frame. `current` and `previous`
/// are shared frame handles, so taking a snapshot at 30 or 60 fps does not
/// copy a single pixel. `opacity` is the crossfade weight of `current` over
/// `previous`, exactly like `BackgroundView::paint`.
#[derive(Debug, Clone)]
pub struct BackgroundSnapshot {
  pub current: Option<Arc<Pixmap>>,
  pub previous: Option<Arc<Pixmap>>,
  pub opacity: f32,
  pub kind: SnapshotKind,
  pub frame_id: u64,
  pub animated: bool,
}

#[derive(Debug)]
pub struct LoadedBackground {
  pub content: Content,
  pub warning: Option<String>,
}

impl LoadedBackground {
  /// Decodes the configured wallpaper for one output. The returned frames are
  /// already the size of the output, so painting is a single blit.
  pub fn load(
    config: &BackgroundConfig,
    size: (u32, u32),
    monitor: Option<&str>,
  ) -> Result<Self, String> {
    let config = config.for_monitor(monitor);
    match config.kind {
      BackgroundKind::Solid => Ok(Self {
        content: Content::Solid(config.color),
        warning: None,
      }),
      BackgroundKind::Gradient => Ok(Self {
        content: Content::Gradient(config.color),
        warning: None,
      }),
      BackgroundKind::Auto if config.image.is_none() => Ok(Self {
        content: if config.gradient {
          Content::Gradient(config.color)
        } else {
          Content::Solid(config.color)
        },
        warning: None,
      }),
      BackgroundKind::Auto | BackgroundKind::Image | BackgroundKind::Animated => {
        let Some(path) = config.image.as_deref() else {
          return Err(
            "background.image is empty; set a PNG/JPEG/GIF/WebP/MP4 path or remove background.kind"
              .to_string(),
          );
        };
        let video = video::is_video_path(path);
        let format = if video {
          None
        } else {
          image::animation_format(path)?
        };
        let wants_animation = config.kind == BackgroundKind::Animated
          || (config.kind == BackgroundKind::Auto && (format.is_some() || video));

        if wants_animation {
          if video {
            let stream = VideoStream::load(
              path,
              size,
              config.fit,
              config.color,
              config.fps,
              config.memory_limit,
              LOAD_LIMITS,
            )?;
            return Ok(Self {
              content: Content::Stream(Arc::new(stream)),
              warning: None,
            });
          }
          if let Some(format) = format {
            let stream = FrameStream::load(
              path,
              format,
              size,
              config.fit,
              config.color,
              config.fps,
              config.memory_limit,
              LOAD_LIMITS,
            )?;
            return Ok(Self {
              content: Content::Stream(Arc::new(stream)),
              warning: None,
            });
          }
          // kind = "animated" on a still file is a config mistake, not a
          // reason to fail. Show the still and say what happened.
          let frame = image::load_static(path, LOAD_LIMITS, size, config.fit, config.color)?;
          return Ok(Self {
            content: Content::Frames {
              frames: vec![Arc::new(frame)],
              delays: Vec::new(),
              cumulative: Vec::new(),
            },
            warning: Some(
              "background.kind = \"animated\" but the file has one frame; showing it still"
                .to_string(),
            ),
          });
        }

        if video {
          let frame = VideoStream::first_frame(
            path,
            size,
            config.fit,
            config.color,
            config.fps,
            config.memory_limit,
          )?;
          return Ok(Self {
            content: Content::Frames {
              frames: vec![Arc::new(frame)],
              delays: Vec::new(),
              cumulative: Vec::new(),
            },
            warning: Some(
              "background.kind = \"image\" shows only the first frame of the video".to_string(),
            ),
          });
        }

        let frame = image::load_static(path, LOAD_LIMITS, size, config.fit, config.color)?;
        let warning = (config.kind == BackgroundKind::Image && format.is_some())
          .then(|| "background.kind = \"image\" shows only the first frame".to_string());
        Ok(Self {
          content: Content::Frames {
            frames: vec![Arc::new(frame)],
            delays: Vec::new(),
            cumulative: Vec::new(),
          },
          warning,
        })
      }
    }
  }

  #[must_use]
  pub fn is_animated(&self) -> bool {
    match &self.content {
      Content::Frames { frames, .. } => frames.len() > 1,
      Content::Stream(_) => true,
      _ => false,
    }
  }

  #[must_use]
  pub fn frame_count(&self) -> usize {
    match &self.content {
      Content::Frames { frames, .. } => frames.len(),
      Content::Stream(_) => 1,
      _ => 1,
    }
  }

  fn frame_at(&self, elapsed: Duration) -> (Option<&Arc<Pixmap>>, usize) {
    match &self.content {
      Content::Frames {
        frames, cumulative, ..
      } => {
        if frames.len() <= 1 {
          return (frames.first(), 0);
        }
        let total = cumulative.last().copied().unwrap_or_default();
        if total.is_zero() {
          return (frames.first(), 0);
        }
        let position = wrap(elapsed, total);
        let index = cumulative
          .partition_point(|end| *end <= position)
          .min(frames.len() - 1);
        (frames.get(index), index)
      }
      _ => (None, 0),
    }
  }
}

/// A request owns everything the loader thread needs. It is `Send`, which is
/// the point: no `Pixmap`, config, or window handle crosses the wire mid-load.
#[derive(Debug)]
pub struct BackgroundRequest {
  pub generation: u64,
  config: BackgroundConfig,
  size: (u32, u32),
  monitor: Option<String>,
}

impl BackgroundRequest {
  /// Runs on the loader thread.
  pub fn load(self) -> (u64, Result<LoadedBackground, String>) {
    let result = LoadedBackground::load(&self.config, self.size, self.monitor.as_deref());
    (self.generation, result)
  }
}

/// The live background view. It owns the placeholder, the current frame set,
/// and the crossfade state. All timing uses `Instant` so the window can decide
/// exactly when the next frame is due.
pub struct BackgroundView {
  config: BackgroundConfig,
  placeholder: Rgb,
  gradient: bool,
  loaded: Option<LoadedBackground>,
  previous: Option<Arc<Pixmap>>,
  fade_start: Option<Instant>,
  frame_start: Instant,
  generation: u64,
  requested: Option<((u32, u32), Option<String>)>,
  accepted: Option<((u32, u32), Option<String>)>,
  loading: bool,
  paused: bool,
  paused_since: Option<Instant>,
  low_power: bool,
  warning: Option<String>,
}

impl BackgroundView {
  #[must_use]
  pub fn new(config: &Config) -> Self {
    Self {
      config: config.background.clone(),
      placeholder: config.background.color,
      gradient: config.background.gradient,
      loaded: None,
      previous: None,
      fade_start: None,
      frame_start: Instant::now(),
      generation: 0,
      requested: None,
      accepted: None,
      loading: false,
      paused: false,
      paused_since: None,
      low_power: config.power.low_power || config.motion.reduced,
      warning: None,
    }
  }

  /// Starts a load for this size and output. Returns `None` when the same
  /// load is already in flight, so a resize storm cannot spawn a thread per
  /// event.
  pub fn request(&mut self, size: (u32, u32), monitor: Option<&str>) -> Option<BackgroundRequest> {
    if size.0 == 0 || size.1 == 0 {
      return None;
    }
    let key = (size, monitor.map(str::to_string));
    if self.loading && self.requested.as_ref() == Some(&key) {
      return None;
    }
    // Already showing this exact size and output. Without this check every
    // redraw would start a fresh loader thread.
    if !self.loading && self.accepted.as_ref() == Some(&key) {
      return None;
    }
    self.generation = self.generation.wrapping_add(1);
    self.requested = Some(key);
    self.loading = true;
    Some(BackgroundRequest {
      generation: self.generation,
      config: self.config.clone(),
      size,
      monitor: monitor.map(str::to_string),
    })
  }

  /// Applies a loader result. Stale generations (from a window that was
  /// resized while a load was running) are ignored.
  pub fn accept(
    &mut self,
    generation: u64,
    result: Result<LoadedBackground, String>,
    now: Instant,
  ) -> bool {
    if generation != self.generation {
      return false;
    }
    self.loading = false;
    self.accepted = self.requested.take();
    match result {
      Ok(loaded) => {
        if let Some(warning) = &loaded.warning {
          warn!(%warning, "background warning");
        }
        self.warning = loaded.warning.clone();
        let had_previous = self.loaded.is_some();
        if let Some(current) = &self.loaded {
          match &current.content {
            Content::Stream(stream) => {
              self.previous = stream.current();
            }
            _ => {
              let (frame, _) = current.frame_at(now.duration_since(self.frame_start));
              self.previous = frame.cloned();
            }
          }
        }
        self.loaded = Some(loaded);
        self.frame_start = now;
        // A live stream must start frozen when low power is on.
        if self.low_power
          && let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|l| &l.content)
        {
          stream.set_paused(true);
        }
        // Only crossfade when replacing something. The very first wallpaper
        // should appear fully opaque: a greeter must not depend on a later
        // animation tick to become visible (an unfocused window would sit on
        // the placeholder forever). Requiring a captured previous frame also
        // keeps the CPU and GPU paths identical: neither fades from a
        // placeholder that the GPU cannot sample.
        self.fade_start = if had_previous && self.previous.is_some() {
          Some(now)
        } else {
          None
        };
        true
      }
      Err(error) => {
        warn!(%error, "cannot load background, keeping the gradient");
        self.warning = Some(format!("Background: {error}"));
        false
      }
    }
  }

  /// A load failure that happened before the event loop could deliver it, as
  /// in the screenshot path or a thread spawn failure.
  pub fn fail(&mut self, message: impl Into<String>) {
    let message = message.into();
    self.loading = false;
    // Remember the failed key too. A corrupt file is not going to fix itself
    // while the greeter is on screen, and retrying on every redraw would spawn
    // a thread per frame.
    self.accepted = self.requested.take();
    // The window path logs failures in `accept`; this is the equivalent for a
    // synchronous load. Without it a `--screenshot` run would show the warning
    // banner but print nothing to stderr, which is easy to miss in CI.
    warn!(%message, "cannot load background, keeping the gradient");
    self.warning = Some(format!("Background: {message}"));
  }

  #[must_use]
  pub fn warning(&self) -> Option<&str> {
    self.warning.as_deref()
  }

  /// True when the current wallpaper has more than one frame, which tells the
  /// glass cache it may refresh on an interval instead of once.
  #[must_use]
  pub fn is_animated(&self) -> bool {
    self
      .loaded
      .as_ref()
      .is_some_and(LoadedBackground::is_animated)
  }

  /// Installs the frame callback on the current live stream. The window calls
  /// this after accepting a wallpaper so every published frame wakes the event
  /// loop instead of relying on a 16 ms poll.
  pub fn set_waker(&mut self, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
    if let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|loaded| &loaded.content) {
      stream.set_waker(waker);
    }
  }

  pub fn set_paused(&mut self, paused: bool) {
    if paused == self.paused {
      return;
    }
    if let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|l| &l.content) {
      stream.set_paused(paused || self.low_power);
    }
    if paused {
      self.paused = true;
      self.paused_since = Some(Instant::now());
      return;
    }
    // Resume where the animation stopped. Shifting the start instant by the
    // paused duration avoids a jump back to frame zero (and avoids pretending
    // the whole pause was part of the animation).
    self.paused = false;
    if let Some(since) = self.paused_since.take() {
      let paused_for = Instant::now().saturating_duration_since(since);
      self.frame_start += paused_for;
      if let Some(fade) = self.fade_start {
        self.fade_start = Some(fade + paused_for);
      }
    }
  }

  /// The instant used to choose an animation frame.
  ///
  /// Low power freezes at the load instant rather than at a live clock, so a
  /// repaint caused by the ticking clock cannot walk the wallpaper forward one
  /// frame per second. Pausing freezes at the pause instant for the same
  /// reason. Without this, "freeze on a still frame" would look true at first
  /// and then drift as soon as the greeter repainted for any other reason.
  fn animation_now(&self, now: Instant) -> Instant {
    if self.low_power {
      self.frame_start
    } else {
      self.paint_time(now)
    }
  }

  /// Paints the placeholder and, when ready, the current wallpaper frame. The
  /// first 320 ms of a new wallpaper is a crossfade from the previous frame.
  pub fn paint(&mut self, canvas: &mut Canvas, now: Instant) {
    let now = self.animation_now(now);
    match &self.loaded {
      None => self.paint_placeholder(canvas),
      Some(loaded) => {
        if let Some(previous) = &self.previous {
          canvas.image_cover(previous);
        } else {
          self.paint_placeholder(canvas);
        }
        let opacity = match self.fade_start {
          Some(start) if !self.low_power => {
            (now.duration_since(start).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
          }
          _ => 1.0,
        };
        if opacity >= 1.0 {
          self.fade_start = None;
        }
        let elapsed = now.duration_since(self.frame_start);
        match &loaded.content {
          Content::Solid(color) => canvas.clear(*color),
          Content::Gradient(color) => draw_gradient(canvas, *color),
          Content::Frames { .. } => {
            let (frame, _) = loaded.frame_at(elapsed);
            if let Some(frame) = frame {
              canvas.image_cover_alpha(frame, opacity);
            }
          }
          Content::Stream(stream) => {
            if self.low_power || self.paused {
              stream.set_paused(true);
            }
            if let Some(frame) = stream.current() {
              canvas.image_cover_alpha(&frame, opacity);
            }
          }
        }
      }
    }
  }

  /// A borrowing-free view of the frame that [`Self::paint`] would draw right
  /// now. The GPU renderer uses it to upload only the current and previous
  /// frames, and to get the crossfade weight and placeholder kind without
  /// painting a full-screen CPU pixmap.
  ///
  /// `frame_id` follows the same pause and low-power rules as
  /// [`Self::frame_id`], so an idle greeter keeps a stable snapshot.
  #[must_use]
  pub fn snapshot(&self, now: Instant) -> BackgroundSnapshot {
    let now = self.animation_now(now);
    let frame_id = self.frame_id(now);
    let placeholder = if self.gradient {
      SnapshotKind::Gradient(self.placeholder)
    } else {
      SnapshotKind::Solid(self.placeholder)
    };
    let Some(loaded) = &self.loaded else {
      return BackgroundSnapshot {
        current: None,
        previous: None,
        opacity: 1.0,
        kind: placeholder,
        frame_id,
        animated: false,
      };
    };
    let elapsed = now.duration_since(self.frame_start);
    let (current, mut kind) = match &loaded.content {
      Content::Solid(color) => (None, SnapshotKind::Solid(*color)),
      Content::Gradient(color) => (None, SnapshotKind::Gradient(*color)),
      Content::Frames { .. } => {
        let (frame, _) = loaded.frame_at(elapsed);
        (frame.cloned(), SnapshotKind::Image)
      }
      Content::Stream(stream) => (stream.current(), SnapshotKind::Image),
    };
    // A live source can be between its load and the first published frame.
    // Falling back to the placeholder is what `paint` does too.
    if kind == SnapshotKind::Image && current.is_none() {
      kind = placeholder;
    }
    let opacity = match self.fade_start {
      Some(start) if !self.low_power => {
        (now.duration_since(start).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
      }
      _ => 1.0,
    };
    BackgroundSnapshot {
      current,
      // `paint` keeps drawing the old frame for the whole transition, but the
      // GPU only needs it while the new frame is still fading in.
      previous: if opacity < 1.0 {
        self.previous.clone()
      } else {
        None
      },
      opacity,
      kind,
      frame_id,
      animated: loaded.is_animated(),
    }
  }

  /// Paints a snapshot for a CPU consumer such as the glass blur cache. This
  /// has no side effects: unlike [`Self::paint`] it never clears the fade
  /// state, so the GPU crossfade and the CPU fallback cannot fight over it.
  pub fn paint_snapshot(&self, canvas: &mut Canvas, now: Instant) {
    let snapshot = self.snapshot(now);
    if let Some(previous) = &snapshot.previous {
      canvas.image_cover(previous);
    }
    match snapshot.kind {
      SnapshotKind::Solid(color) => canvas.clear(color),
      SnapshotKind::Gradient(color) => draw_gradient(canvas, color),
      SnapshotKind::Image => {
        if snapshot.previous.is_none() {
          self.paint_placeholder(canvas);
        }
        if let Some(current) = &snapshot.current {
          canvas.image_cover_alpha(current, snapshot.opacity);
        }
      }
    }
  }

  fn paint_placeholder(&self, canvas: &mut Canvas) {
    if self.gradient {
      draw_gradient(canvas, self.placeholder);
    } else {
      canvas.clear(self.placeholder);
    }
  }

  /// The next instant the window should wake for a wallpaper frame or fade.
  #[must_use]
  pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
    if self.low_power || self.paused {
      return None;
    }
    if let Some(start) = self.fade_start {
      let end = start + FADE;
      if now < end {
        return Some(now + Duration::from_millis(16));
      }
    }
    let loaded = self.loaded.as_ref()?;
    if let Content::Stream(_) = &loaded.content {
      // The stream wakes the window when a frame is published, so there is
      // nothing to poll. Keeping a 16 ms deadline here would burn a wakeup per
      // frame even when the video is slower than 60 fps.
      return None;
    }
    let Content::Frames { delays, .. } = &loaded.content else {
      return None;
    };
    if delays.len() <= 1 {
      return None;
    }
    let elapsed = now.duration_since(self.frame_start);
    let total: Duration = delays.iter().copied().sum();
    if total.is_zero() {
      return None;
    }
    let mut position = wrap(elapsed, total);
    for delay in delays {
      if position < *delay {
        return Some(now + (*delay - position));
      }
      position = position.saturating_sub(*delay);
    }
    Some(now + *delays.first().unwrap_or(&Duration::from_millis(33)))
  }

  /// The instant the next paint should treat as "now". While paused this is
  /// the moment the pause began, so repaints driven by the clock tick (the
  /// clock keeps ticking on an unfocused but visible greeter) render the
  /// frozen frame instead of silently advancing the animation.
  #[must_use]
  pub fn paint_time(&self, now: Instant) -> Instant {
    if self.paused {
      self.paused_since.unwrap_or(now)
    } else {
      now
    }
  }

  /// A stable identifier for the current frame, used by the blur cache and
  /// the window's change detection. Frozen while paused and in low-power mode,
  /// matching `paint`, so a repaint neither re-blurs nor signals a frame change
  /// when the wallpaper is not supposed to move.
  #[must_use]
  pub fn frame_id(&self, now: Instant) -> u64 {
    // An unloaded wallpaper shows the placeholder, and that is not the same
    // identity as the wallpaper that will load a moment later. The generation
    // does not change across the transition (the request already bumped it and
    // a still image keeps frame index 0 forever), so the blur cache computed
    // from the placeholder would stick around after the real pixels arrive.
    // Sentinel `u64::MAX` means "next change wakes the caches"; glass itself
    // uses it as its not-prepared marker, so the two sides agree.
    if self.loaded.is_none() {
      return u64::MAX;
    }
    if let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|l| &l.content) {
      return self
        .generation
        .wrapping_mul(1_000_003)
        .wrapping_add(stream.sequence());
    }
    let now = self.animation_now(now);
    let index = self.loaded.as_ref().map_or(0, |loaded| {
      loaded.frame_at(now.duration_since(self.frame_start)).1
    });
    self
      .generation
      .wrapping_mul(1024)
      .wrapping_add(index as u64)
  }

  /// True while the crossfade is running.
  #[must_use]
  pub fn fading(&self, now: Instant) -> bool {
    self
      .fade_start
      .is_some_and(|start| now.duration_since(start) < FADE)
  }

  /// Blocks until a wallpaper is ready. Only the screenshot path uses this;
  /// the live window must always start with the placeholder.
  pub fn load_blocking(&mut self, size: (u32, u32), monitor: Option<&str>, now: Instant) -> bool {
    self.generation = self.generation.wrapping_add(1);
    let generation = self.generation;
    match LoadedBackground::load(&self.config, size, monitor) {
      Ok(loaded) => self.accept(generation, Ok(loaded), now),
      Err(error) => {
        self.fail(error);
        false
      }
    }
  }

  /// The configured background, for screenshots and diagnostics.
  #[must_use]
  pub fn config(&self) -> &BackgroundConfig {
    &self.config
  }
}

fn draw_gradient(canvas: &mut Canvas, color: Rgb) {
  canvas.vertical_gradient(color, shade(color, 0.72));
}

/// Darkens a colour by a factor, keeping alpha. Used for the default gradient
/// so a theme only has to specify one background colour.
fn shade(color: Rgb, factor: f32) -> Rgb {
  let channel = |value: u8| (f32::from(value) * factor).round().clamp(0.0, 255.0) as u8;
  Rgb::rgba(
    channel(color.r),
    channel(color.g),
    channel(color.b),
    color.a,
  )
}

/// Duration does not implement remainder, so wrap nanosecond counts. All
/// wallpaper durations are far below u64 nanoseconds.
fn wrap(value: Duration, modulus: Duration) -> Duration {
  if modulus.is_zero() {
    return Duration::ZERO;
  }
  Duration::from_nanos((value.as_nanos() % modulus.as_nanos()) as u64)
}

#[cfg(test)]
fn cumulative(delays: &[Duration]) -> Vec<Duration> {
  let mut total = Duration::ZERO;
  delays
    .iter()
    .map(|delay| {
      total += *delay;
      total
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::{BackgroundView, Content, LoadedBackground, cumulative};
  use crate::config::{BackgroundConfig, BackgroundKind, Config};
  use std::sync::Arc;
  use std::time::{Duration, Instant};

  fn config(kind: BackgroundKind) -> BackgroundConfig {
    let mut config = BackgroundConfig {
      kind,
      ..Config::default().background
    };
    config.color = crate::color::Rgb::solid(10, 20, 30);
    config
  }

  #[test]
  fn solid_and_gradient_do_not_need_files() {
    let solid = LoadedBackground::load(&config(BackgroundKind::Solid), (320, 200), None).unwrap();
    assert!(matches!(solid.content, Content::Solid(_)));
    let gradient =
      LoadedBackground::load(&config(BackgroundKind::Gradient), (320, 200), None).unwrap();
    assert!(matches!(gradient.content, Content::Gradient(_)));
  }

  #[test]
  fn missing_animation_still_yields_a_placeholder_view() {
    let mut config = Config::default();
    config.background = BackgroundConfig {
      kind: BackgroundKind::Image,
      ..config.background
    };
    let mut view = BackgroundView::new(&config);
    let now = Instant::now();
    assert!(!view.load_blocking((320, 200), None, now));
    assert!(view.warning().is_some());
  }

  #[test]
  fn cumulative_delays_are_monotonic() {
    let values = cumulative(&[Duration::from_millis(10), Duration::from_millis(20)]);
    assert_eq!(
      values,
      [Duration::from_millis(10), Duration::from_millis(30)]
    );
  }

  #[test]
  fn an_unloaded_wallpaper_is_not_the_loaded_identity() {
    // The frame id gates the glass blur cache. Before the loader returns, the
    // window blurs the navy placeholder; the id stays there forever (a still
    // background keeps frame index 0 and the generation was already bumped by
    // the request), so the cache never re-blurred and the plates stayed navy
    // on screen. Unloaded must return the sentinel; loaded must move off it.
    let mut view = BackgroundView::new(&Config {
      background: config(BackgroundKind::Gradient),
      ..Config::default()
    });
    assert_eq!(
      view.frame_id(Instant::now()),
      u64::MAX,
      "unloaded id must be the sentinel"
    );
    let request = view.request((320, 200), None).unwrap();
    let (generation, result) = request.load();
    assert!(view.accept(generation, result, Instant::now()));
    assert_ne!(
      view.frame_id(Instant::now()),
      u64::MAX,
      "loaded background kept the placeholder id, so the blur cache would never refresh"
    );
  }

  #[test]
  fn a_stale_generation_is_ignored() {
    let base = Config {
      background: config(BackgroundKind::Gradient),
      ..Config::default()
    };
    let mut view = BackgroundView::new(&base);
    let request = view.request((320, 200), None).unwrap();
    let (generation, result) = request.load();
    assert!(view.accept(generation, result, Instant::now()));
    assert!(!view.accept(
      generation + 10,
      Ok(LoadedBackground::load(&config(BackgroundKind::Gradient), (320, 200), None).unwrap()),
      Instant::now()
    ));
  }

  #[test]
  fn pausing_freezes_the_frame_and_resume_continues_from_it() {
    // Two synthetic frames with 50 ms delays, injected directly because the
    // loader path is covered elsewhere. While paused, the frame id must not
    // move no matter how much wall-clock passes; on resume it continues from
    // the frozen point instead of jumping to the live position.
    let mut view = BackgroundView::new(&Config {
      background: config(BackgroundKind::Animated),
      ..Config::default()
    });
    let frame = tiny_skia::Pixmap::new(4, 4).unwrap();
    view.loaded = Some(LoadedBackground {
      content: Content::Frames {
        cumulative: cumulative(&[Duration::from_millis(50), Duration::from_millis(100)]),
        delays: vec![Duration::from_millis(50), Duration::from_millis(50)],
        frames: vec![Arc::new(frame.clone()), Arc::new(frame)],
      },
      warning: None,
    });

    view.set_paused(true);
    let frozen = view.frame_id(Instant::now());
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(
      view.frame_id(Instant::now()),
      frozen,
      "paused frame id moved"
    );

    view.set_paused(false);
    assert_eq!(
      view.frame_id(Instant::now()),
      frozen,
      "resume jumped the animation"
    );
    std::thread::sleep(Duration::from_millis(120));
    assert_ne!(
      view.frame_id(Instant::now()),
      frozen,
      "resume did not advance"
    );
  }

  #[test]
  fn paint_snapshot_matches_paint_for_every_background_kind() {
    // The GPU path paints the wallpaper from `snapshot`/`paint_snapshot` while
    // the CPU and screenshot paths use `paint`. They must agree pixel for
    // pixel, or the live window would drift from `--screenshot`.
    use crate::ui::canvas::Canvas;
    use tiny_skia::Color;

    let fill = |red: u8, green: u8, blue: u8| {
      let mut pixmap = tiny_skia::Pixmap::new(4, 4).unwrap();
      pixmap.fill(Color::from_rgba8(red, green, blue, 255));
      Arc::new(pixmap)
    };

    let compare = |view: &mut BackgroundView, now: Instant| {
      let mut painted = Canvas::new(4, 4).unwrap();
      let mut snapshot = Canvas::new(4, 4).unwrap();
      view.paint(&mut painted, now);
      view.paint_snapshot(&mut snapshot, now);
      assert_eq!(
        painted.pixmap().data(),
        snapshot.pixmap().data(),
        "snapshot painting diverged from paint"
      );
    };

    // Placeholder, solid, and gradient need no frame set at all.
    for kind in [
      BackgroundKind::Solid,
      BackgroundKind::Gradient,
      BackgroundKind::Auto,
    ] {
      let mut view = BackgroundView::new(&Config {
        background: config(kind),
        ..Config::default()
      });
      compare(&mut view, Instant::now());
    }

    let mut view = BackgroundView::new(&Config {
      background: config(BackgroundKind::Animated),
      ..Config::default()
    });
    view.loaded = Some(LoadedBackground {
      content: Content::Frames {
        cumulative: cumulative(&[Duration::from_millis(50), Duration::from_millis(100)]),
        delays: vec![Duration::from_millis(50), Duration::from_millis(50)],
        frames: vec![fill(220, 40, 40), fill(40, 180, 80)],
      },
      warning: None,
    });
    view.frame_start = Instant::now();
    let start = view.frame_start;
    compare(&mut view, start);

    // A half-finished crossfade must also match: this is the path that mixes
    // the previous wallpaper with the new one.
    view.previous = Some(fill(20, 20, 200));
    view.fade_start = Some(start - Duration::from_millis(160));
    compare(&mut view, start);
  }

  #[test]
  fn low_power_freezes_on_one_frame() {
    // Low-power mode is documented as freezing the wallpaper on a still
    // frame. The clock keeps repainting, so the frame id must stay at the
    // load instant even while wall-clock time moves well past a frame delay.
    let mut config = Config {
      background: config(BackgroundKind::Animated),
      ..Config::default()
    };
    config.power.low_power = true;
    let mut view = BackgroundView::new(&config);
    let frame = tiny_skia::Pixmap::new(4, 4).unwrap();
    view.loaded = Some(LoadedBackground {
      content: Content::Frames {
        cumulative: cumulative(&[Duration::from_millis(50), Duration::from_millis(100)]),
        delays: vec![Duration::from_millis(50), Duration::from_millis(50)],
        frames: vec![Arc::new(frame.clone()), Arc::new(frame)],
      },
      warning: None,
    });

    let frozen = view.frame_id(Instant::now());
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(
      view.frame_id(Instant::now()),
      frozen,
      "low-power frame id moved"
    );
    assert!(view.next_deadline(Instant::now()).is_none());
  }
}
