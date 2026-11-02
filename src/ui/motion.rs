//! Small, restrained motion for the login screen.
//!
//! The greeter is not a game. Motion exists to make the form feel deliberate:
//! the card slides in once, the focus ring settles quickly, and a failed login
//! shakes once. Everything is driven by `Instant` and can be disabled with
//! `[motion] reduced = true` or `[power] low_power = true`.
//!
//! The ENCORE keys add a shared beat clock, a whole-scene camera, hit-stop
//! timing, and post effects. All of them default to a no-op. The beat clock is
//! a pure function of the boot instant, and every consumer has to opt in with
//! its own key, so turning the clock on by itself changes nothing.

use std::time::{Duration, Instant};

use crate::config::{BeatConfig, CameraConfig, MotionConfig, MotionMoment, PostConfig};
use crate::ui::anim::{ease_out_cubic, progress};

/// The whole-scene transform handed to the GPU compositor. The software path
/// deliberately ignores it: resampling a finished CPU frame would cost more
/// than the effect is worth and would blur text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFrame {
  /// Uniform scale around the screen centre. 1.0 is no zoom.
  pub scale: f32,
  /// Horizontal shake in logical pixels.
  pub dx: f32,
  /// Vertical shake in logical pixels.
  pub dy: f32,
}

impl CameraFrame {
  /// The neutral transform. This is what a config without `[motion.camera]`
  /// and every screenshot gets.
  #[must_use]
  pub fn identity() -> Self {
    Self {
      scale: 1.0,
      dx: 0.0,
      dy: 0.0,
    }
  }

  #[must_use]
  pub fn is_identity(&self) -> bool {
    (self.scale - 1.0).abs() <= f32::EPSILON
      && self.dx.abs() <= f32::EPSILON
      && self.dy.abs() <= f32::EPSILON
  }
}

/// The post-effect values for one frame. All zeros mean "draw nothing extra".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostFrame {
  /// Current speed-line intensity, already decayed over its window.
  pub speed_lines: f32,
  /// Current letterbox bar height as a fraction of the screen height.
  pub letterbox: f32,
  pub vignette: f32,
  /// Chromatic aberration strength. The software path ignores this field.
  pub chromatic: f32,
  /// Glitch strength, 0 to 1. Zero means the window is closed.
  pub glitch: f32,
  /// Frame-varying seed for the shader's slice displacements.
  pub glitch_seed: f32,
}

impl PostFrame {
  #[must_use]
  pub fn settled() -> Self {
    Self {
      speed_lines: 0.0,
      letterbox: 0.0,
      vignette: 0.0,
      chromatic: 0.0,
      glitch: 0.0,
      glitch_seed: 0.0,
    }
  }
}

/// The values the renderer needs for one animated frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionFrame {
  pub card_dx: f32,
  pub card_dy: f32,
  /// Opacity of the card tint and shadow during the boot fade.
  pub opacity: f32,
  /// 0.0 right after a focus change, 1.0 when settled.
  pub focus_t: f32,
  pub settled: bool,
  /// Beat phase in 0..1. Zero when the beat clock is off or not started.
  pub beat_phase: f32,
  /// Index of the current beat since the boot instant.
  pub beat_index: u64,
  /// Length of one beat in milliseconds, or zero when the clock is off.
  pub beat_ms: f32,
  pub camera: CameraFrame,
  pub post: PostFrame,
}

impl MotionFrame {
  /// Screenshot and reduced-motion state: no offsets, fully opaque, settled,
  /// and every ENCORE effect at its neutral value.
  #[must_use]
  pub fn settled() -> Self {
    Self {
      card_dx: 0.0,
      card_dy: 0.0,
      opacity: 1.0,
      focus_t: 1.0,
      settled: true,
      beat_phase: 0.0,
      beat_index: 0,
      beat_ms: 0.0,
      camera: CameraFrame::identity(),
      post: PostFrame::settled(),
    }
  }
}

/// Phase of the shared beat clock at `elapsed_ms`. Offset is subtracted before
/// the modulo, so "beat zero" happens at boot plus the configured offset.
#[must_use]
pub fn beat_phase_at(beat: &BeatConfig, elapsed_ms: f64) -> f32 {
  let period = f64::from(beat.period_ms());
  if period <= 0.0 {
    return 0.0;
  }
  let shifted = elapsed_ms - beat.offset.as_secs_f64() * 1000.0;
  (shifted.rem_euclid(period) / period) as f32
}

/// Whole beats since the boot instant. Negative time clamps to zero so a test
/// that asks about a moment before boot never wraps into a huge index.
#[must_use]
pub fn beat_index_at(beat: &BeatConfig, elapsed_ms: f64) -> u64 {
  let period = f64::from(beat.period_ms());
  if period <= 0.0 {
    return 0;
  }
  let shifted = elapsed_ms - beat.offset.as_secs_f64() * 1000.0;
  if shifted < 0.0 {
    0
  } else {
    (shifted / period).floor() as u64
  }
}

/// Decor scale multiplier on a beat. The squared falloff gives a fast pop that
/// settles before the next beat, and the hard cap is 1.06.
#[must_use]
pub fn decor_beat_scale(phase: f32) -> f32 {
  let phase = phase.clamp(0.0, 1.0);
  1.0 + 0.06 * (1.0 - phase).powi(2)
}

/// Ticker speed multiplier on a beat. Linear decay, capped at 1.25.
#[must_use]
pub fn ticker_beat_speed(phase: f32) -> f32 {
  let phase = phase.clamp(0.0, 1.0);
  1.0 + 0.25 * (1.0 - phase)
}

/// Pure camera math so tests never have to sleep. `punch_age` and `shake_age`
/// are `None` when that trigger has not fired.
#[must_use]
pub fn camera_sample(
  camera: &CameraConfig,
  breathing_secs: f32,
  punch_age: Option<Duration>,
  shake_age: Option<Duration>,
) -> CameraFrame {
  let punch_decay = punch_age.map_or(0.0, |age| decay(age, camera.punch_ms));
  let shake_decay = shake_age.map_or(0.0, |age| decay(age, camera.shake_ms));
  let breath_period = (camera.breath_ms / 1000.0).max(0.001);
  let breath = if camera.breathing > 0.0 && breathing_secs.is_finite() {
    (breathing_secs * std::f32::consts::TAU / breath_period).sin() * camera.breathing
  } else {
    0.0
  };
  let scale = 1.0 + camera.punch * punch_decay.powi(2) + breath;
  let amplitude = camera.shake * shake_decay.powi(2);
  let seconds = shake_age.map_or(0.0, |age| age.as_secs_f32());
  let dx = (seconds * std::f32::consts::TAU * 12.0).sin() * amplitude;
  let dy = (seconds * std::f32::consts::TAU * 15.0).cos() * amplitude * 0.7;
  CameraFrame {
    scale: if scale.is_finite() {
      scale.max(0.01)
    } else {
      1.0
    },
    dx: if dx.is_finite() { dx } else { 0.0 },
    dy: if dy.is_finite() { dy } else { 0.0 },
  }
}

/// 1.0 at age zero, falling linearly to 0.0 at `ms`. A zero or negative
/// duration is already finished, which is how "off" stays a no-op.
fn decay(age: Duration, ms: f32) -> f32 {
  if !ms.is_finite() || ms <= 0.0 {
    return 0.0;
  }
  (1.0 - (age.as_secs_f32() * 1000.0 / ms).clamp(0.0, 1.0)).max(0.0)
}

pub struct Motion {
  config: MotionConfig,
  reduced: bool,
  boot_start: Option<Instant>,
  focus_start: Option<Instant>,
  shake_start: Option<Instant>,
  camera_punch_start: Option<Instant>,
  camera_shake_start: Option<Instant>,
  success_at: Option<Instant>,
  speed_start: Option<Instant>,
  glitch_start: Option<Instant>,
}

impl Motion {
  #[must_use]
  pub fn new(config: &MotionConfig, low_power: bool) -> Self {
    Self {
      config: config.clone(),
      reduced: config.reduced || low_power,
      boot_start: None,
      focus_start: None,
      shake_start: None,
      camera_punch_start: None,
      camera_shake_start: None,
      success_at: None,
      speed_start: None,
      glitch_start: None,
    }
  }

  /// Records that the focused field changed. The next frame animates from 0.
  pub fn notify_focus(&mut self, now: Instant) {
    if self.reduced || !self.config.enabled || self.config.focus.is_zero() {
      return;
    }
    self.focus_start = Some(now);
  }

  /// Records a failed login. The card shakes once, then stops.
  pub fn notify_error(&mut self, now: Instant) {
    if self.reduced || !self.config.enabled || self.config.shake.is_zero() {
      return;
    }
    self.shake_start = Some(now);
  }

  /// Starts the camera, post, and hit-stop triggers that belong to `moment`.
  ///
  /// This is called from the window after the relevant app event, never from
  /// the auth worker. Nothing here can delay a request or a handoff.
  pub fn notify_moment(&mut self, moment: MotionMoment, now: Instant) {
    if self.reduced || !self.config.enabled {
      return;
    }
    let camera = &self.config.camera;
    let post = &self.config.post;
    match moment {
      MotionMoment::Submit => {
        if camera.punch > 0.0 || camera.shake > 0.0 {
          self.camera_punch_start = (camera.punch > 0.0).then_some(now);
          self.camera_shake_start = (camera.shake > 0.0).then_some(now);
        }
        if post.speed_lines > 0.0 && post.speed_lines_on.accepts(moment) {
          self.speed_start = Some(now);
        }
        if post.glitch_ms > 0.0 && post.glitch_on.accepts(moment) {
          self.glitch_start = Some(now);
        }
      }
      MotionMoment::Success => {
        self.success_at = Some(now);
        if camera.punch > 0.0 {
          self.camera_punch_start = Some(now);
        }
      }
      MotionMoment::Failure => {
        if camera.shake > 0.0 {
          self.camera_shake_start = Some(now);
        }
        if post.glitch_ms > 0.0 && post.glitch_on.accepts(moment) {
          self.glitch_start = Some(now);
        }
      }
    }
  }

  fn boot_elapsed_ms(&self, now: Instant) -> f64 {
    self.boot_start.map_or(0.0, |start| {
      now.saturating_duration_since(start).as_secs_f64() * 1000.0
    })
  }

  /// Beat phase in 0..1. Before the first presented frame the clock reads as
  /// phase zero, which keeps the very first frame deterministic.
  #[must_use]
  pub fn beat_phase(&self, now: Instant) -> f32 {
    beat_phase_at(&self.config.beat, self.boot_elapsed_ms(now))
  }

  /// Whole beats since boot.
  #[must_use]
  pub fn beat_index(&self, now: Instant) -> u64 {
    beat_index_at(&self.config.beat, self.boot_elapsed_ms(now))
  }

  #[must_use]
  pub fn beat_period_ms(&self) -> f32 {
    self.config.beat.period_ms()
  }

  /// The most recent beat instant, used by the particle burst. `None` when the
  /// clock is off or the first beat has not happened yet.
  #[must_use]
  pub fn beat_instant(&self, now: Instant) -> Option<Instant> {
    let period_ms = self.beat_period_ms();
    if self.boot_start.is_none() || period_ms <= 0.0 {
      return None;
    }
    let elapsed_ms = self.boot_elapsed_ms(now);
    if elapsed_ms < self.config.beat.offset.as_secs_f64() * 1000.0 {
      return None;
    }
    Some(now - Duration::from_secs_f32(self.beat_phase(now) * period_ms / 1000.0))
  }

  /// Computes this frame's values. The boot animation starts on the first
  /// call, which is the first presented frame rather than process start.
  pub fn frame(&mut self, now: Instant, scale: f32) -> MotionFrame {
    if self.reduced || !self.config.enabled {
      return MotionFrame::settled();
    }
    let boot_start = *self.boot_start.get_or_insert(now);
    let boot_t = progress(now, boot_start, self.config.boot);
    let focus_t = self
      .focus_start
      .map_or(1.0, |start| progress(now, start, self.config.focus));
    let shake_t = self
      .shake_start
      .map_or(1.0, |start| progress(now, start, self.config.shake));

    let boot_ease = ease_out_cubic(boot_t);
    let card_dy = (1.0 - boot_ease) * 18.0 * scale;
    let card_dx = if shake_t < 1.0 {
      let decay = (1.0 - shake_t).powi(2);
      (shake_t * std::f32::consts::TAU * 3.0).sin() * 9.0 * scale * decay
    } else {
      0.0
    };
    let opacity = 0.35 + 0.65 * boot_ease;
    let settled = boot_t >= 1.0 && focus_t >= 1.0 && shake_t >= 1.0;
    if settled {
      self.focus_start = None;
      self.shake_start = None;
    }

    let beat_phase = beat_phase_at(&self.config.beat, self.boot_elapsed_ms(now));
    let beat_ms = self.config.beat.period_ms();
    let breathing_secs = now.saturating_duration_since(boot_start).as_secs_f32();
    let camera = camera_sample(
      &self.config.camera,
      breathing_secs,
      self
        .camera_punch_start
        .map(|at| now.saturating_duration_since(at)),
      self
        .camera_shake_start
        .map(|at| now.saturating_duration_since(at)),
    );
    let post = self.post_frame(now, beat_phase, beat_ms);

    MotionFrame {
      card_dx,
      card_dy,
      opacity,
      focus_t,
      settled,
      beat_phase,
      beat_index: beat_index_at(&self.config.beat, self.boot_elapsed_ms(now)),
      beat_ms,
      camera,
      post,
    }
  }

  /// Resolves the post-effect values. Every branch returns a neutral value
  /// when its key is zero, so default configs produce exact identity output.
  fn post_frame(&self, now: Instant, beat_phase: f32, beat_ms: f32) -> PostFrame {
    let post = &self.config.post;
    PostFrame {
      speed_lines: self.speed_lines(now, beat_phase, beat_ms, post),
      letterbox: self.letterbox(now, post),
      vignette: if post.vignette.is_finite() {
        post.vignette.clamp(0.0, 1.0)
      } else {
        0.0
      },
      chromatic: if post.chromatic.is_finite() {
        post.chromatic.clamp(0.0, 1.0)
      } else {
        0.0
      },
      ..self.glitch(now, post)
    }
  }

  fn speed_lines(&self, now: Instant, beat_phase: f32, beat_ms: f32, post: &PostConfig) -> f32 {
    let base = post.speed_lines;
    if !base.is_finite() || base <= 0.0 {
      return 0.0;
    }
    let window_ms = f64::from(post.speed_window_ms());
    let mut intensity: f32 = 0.0;
    if post.speed_lines_on.accepts(MotionMoment::Submit)
      && let Some(start) = self.speed_start
    {
      let age_ms = now.saturating_duration_since(start).as_secs_f64() * 1000.0;
      if age_ms < window_ms {
        intensity = base * (1.0 - age_ms / window_ms) as f32;
      }
    }
    if post.speed_lines_on.beat() && beat_ms > 0.0 {
      let age_ms = f64::from(beat_phase * beat_ms);
      if age_ms < window_ms {
        intensity = intensity.max(base * (1.0 - age_ms / window_ms) as f32);
      }
    }
    intensity.clamp(0.0, 1.0)
  }

  fn letterbox(&self, now: Instant, post: &PostConfig) -> f32 {
    let amount = post.letterbox;
    if !amount.is_finite() || amount <= 0.0 {
      return 0.0;
    }
    let animate = Duration::from_secs_f32((post.letterbox_animate_ms / 1000.0).max(0.0));
    let boot_t = self
      .boot_start
      .map_or(1.0, |start| ease_out_cubic(progress(now, start, animate)));
    let out_t = self
      .success_at
      .map_or(0.0, |start| ease_out_cubic(progress(now, start, animate)));
    (amount * boot_t * (1.0 - out_t)).clamp(0.0, 0.2)
  }

  fn glitch(&self, now: Instant, post: &PostConfig) -> PostFrame {
    let duration = post.glitch_ms;
    if !duration.is_finite() || duration <= 0.0 {
      return PostFrame::settled();
    }
    let Some(start) = self.glitch_start else {
      return PostFrame::settled();
    };
    let age_ms = now.saturating_duration_since(start).as_secs_f32() * 1000.0;
    if age_ms >= duration {
      return PostFrame::settled();
    }
    PostFrame {
      glitch: 1.0,
      // A new seed every rendered frame, quantised so a slow loop does not
      // shimmer faster than the effect means to.
      glitch_seed: (age_ms / 16.0).floor(),
      ..PostFrame::settled()
    }
  }

  /// True while the window should request animation frames.
  pub fn active(&self) -> bool {
    if self.reduced || !self.config.enabled {
      return false;
    }
    self.config.camera.breathing > 0.0 || self.camera_active() || self.overlay_active()
  }

  /// True while a fast frame is required. Camera punch/shake and the short
  /// post-effect windows are fast. Beat pulses, ticker scroll, and ambient
  /// particles are deliberately not: they read fine at half rate and cost a
  /// full text re-shape on every frame.
  pub fn fast_active(&self) -> bool {
    if self.reduced || !self.config.enabled {
      return false;
    }
    let now = Instant::now();
    let speed = self.config.post.speed_lines > 0.0
      && self.speed_start.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0
          < self.config.post.speed_window_ms()
      });
    let glitch = self.config.post.glitch_ms > 0.0
      && self.glitch_start.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0 < self.config.post.glitch_ms
      });
    let bars = self.config.post.letterbox > 0.0
      && (self.success_at.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0
          < self.config.post.letterbox_animate_ms
      }) || self.boot_start.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0
          < self.config.post.letterbox_animate_ms
      }));
    self.camera_active()
      || speed
      || glitch
      || bars
      || self.boot_start.is_none()
      || self
        .focus_start
        .is_some_and(|start| start.elapsed() < self.config.focus)
      || self
        .shake_start
        .is_some_and(|start| start.elapsed() < self.config.shake)
      || self
        .boot_start
        .is_some_and(|start| start.elapsed() < self.config.boot)
  }

  /// True while the CPU overlay pixels can change. Camera-only motion is
  /// deliberately excluded: it is a GPU transform, so a breathing camera must
  /// not force a full text re-rasterisation every frame.
  pub fn overlay_active(&self) -> bool {
    if self.reduced || !self.config.enabled {
      return false;
    }
    let now = Instant::now();
    let speed_active = self.config.post.speed_lines > 0.0
      && (self.config.post.speed_lines_on.beat()
        || self.speed_start.is_some_and(|start| {
          now.saturating_duration_since(start).as_secs_f32() * 1000.0
            < self.config.post.speed_window_ms()
        }));
    let glitch_active = self.config.post.glitch_ms > 0.0
      && self.glitch_start.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0 < self.config.post.glitch_ms
      });
    let bars_active = self.config.post.letterbox > 0.0
      && (self.success_at.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0
          < self.config.post.letterbox_animate_ms
      }) || self.boot_start.is_some_and(|start| {
        now.saturating_duration_since(start).as_secs_f32() * 1000.0
          < self.config.post.letterbox_animate_ms
      }));
    self.config.beat.bpm > 0.0
      || speed_active
      || glitch_active
      || bars_active
      || self.boot_start.is_none()
      || self
        .focus_start
        .is_some_and(|start| start.elapsed() < self.config.focus)
      || self
        .shake_start
        .is_some_and(|start| start.elapsed() < self.config.shake)
      || self
        .boot_start
        .is_some_and(|start| start.elapsed() < self.config.boot)
  }

  fn camera_active(&self) -> bool {
    let now = Instant::now();
    self.camera_punch_start.is_some_and(|start| {
      now.saturating_duration_since(start).as_secs_f32() * 1000.0 < self.config.camera.punch_ms
    }) || self.camera_shake_start.is_some_and(|start| {
      now.saturating_duration_since(start).as_secs_f32() * 1000.0 < self.config.camera.shake_ms
    })
  }
}

#[cfg(test)]
mod tests;
