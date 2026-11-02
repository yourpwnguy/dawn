//! Motion, beat, camera, hit-stop, and post-effect settings.

use super::*;

#[derive(Debug, Clone)]
pub struct MotionConfig {
  pub enabled: bool,
  /// Skip all motion, including background animation, even if enabled.
  pub reduced: bool,
  pub boot: Duration,
  pub focus: Duration,
  pub shake: Duration,
  /// Time constant for the widget drag easing. Zero makes dragging snap.
  pub drag: Duration,
  /// Refresh rate for animated decor only. Ambient ornaments are slow, so
  /// capping their overlay redraws keeps the CPU free for the wallpaper while
  /// focus, typing, and button effects still run at the full motion rate.
  pub decor_fps: u32,
  /// Global multiplier for cursor parallax, 0 to 2. Each decor item also needs
  /// a positive `parallax_depth`, and the effect only runs in a live window
  /// where the pointer position is known. Zero disables it everywhere.
  pub parallax: f32,
  /// Optional musical clock shared by opt-in consumers.
  pub beat: BeatConfig,
  /// Whole-scene camera transform, GPU compositor only.
  pub camera: CameraConfig,
  /// Per-moment frame holds.
  pub hitstop: HitstopConfig,
  /// Full-screen post effects.
  pub post: PostConfig,
}

impl MotionConfig {
  /// The wall-clock gap between two decor-only overlay frames.
  #[must_use]
  pub fn decor_frame(&self) -> Duration {
    Duration::from_millis(1000 / u64::from(self.decor_fps.max(1)))
  }
}

impl Default for MotionConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      reduced: false,
      boot: Duration::from_millis(450),
      focus: Duration::from_millis(160),
      shake: Duration::from_millis(320),
      drag: Duration::from_millis(120),
      // The default is the same cadence as the window. A theme that ships a
      // heavy ornament or targets a low-power panel can drop this to 30.
      decor_fps: 60,
      parallax: 0.0,
      beat: BeatConfig::default(),
      camera: CameraConfig::default(),
      hitstop: HitstopConfig::default(),
      post: PostConfig::default(),
    }
  }
}

/// The shared beat clock. A BPM of zero turns it off entirely, which keeps the
/// default config quiet even though every consumer key exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatConfig {
  /// Beats per minute, 20 to 300. Zero is the documented off value.
  pub bpm: f32,
  /// Delay before the first beat, 0 to 10000 ms. This is what lets a theme
  /// line the pulse up with a musical intro.
  pub offset: Duration,
}

impl Default for BeatConfig {
  fn default() -> Self {
    Self {
      bpm: 0.0,
      offset: Duration::ZERO,
    }
  }
}

impl BeatConfig {
  /// Milliseconds in one beat, or zero when the clock is off.
  #[must_use]
  pub fn period_ms(&self) -> f32 {
    if self.bpm.is_finite() && self.bpm > 0.0 {
      60_000.0 / self.bpm
    } else {
      0.0
    }
  }
}

/// Whole-scene camera motion. The transform only exists on the GPU compositor;
/// the software path ignores it instead of faking a full-frame resample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraConfig {
  /// Extra scale at the instant of a punch, 0 to 0.2.
  pub punch: f32,
  /// Time for the punch to decay back to 1.0, 60 to 600 ms.
  pub punch_ms: f32,
  /// Peak shake amplitude in logical pixels, 0 to 40.
  pub shake: f32,
  /// Time for the shake to decay to zero, 60 to 1000 ms.
  pub shake_ms: f32,
  /// Sinusoidal scale breathing amplitude, 0 to 0.02.
  pub breathing: f32,
  /// Breathing period, 1000 to 10000 ms.
  pub breath_ms: f32,
}

impl Default for CameraConfig {
  fn default() -> Self {
    Self {
      punch: 0.0,
      punch_ms: 180.0,
      shake: 0.0,
      shake_ms: 300.0,
      breathing: 0.0,
      breath_ms: 6000.0,
    }
  }
}

/// Frame holds for the three game-feel moments. A zero duration is off.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HitstopConfig {
  pub submit: Duration,
  pub success: Duration,
  pub failure: Duration,
}

/// A moment that can start a camera, post, or hit-stop effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionMoment {
  Submit,
  Success,
  Failure,
}

/// When the speed-line burst is allowed to fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeedLinesOn {
  Beat,
  #[default]
  Submit,
  Both,
}

impl SpeedLinesOn {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "beat" => Some(Self::Beat),
      "submit" => Some(Self::Submit),
      "both" => Some(Self::Both),
      _ => None,
    }
  }

  #[must_use]
  pub fn accepts(self, moment: MotionMoment) -> bool {
    match moment {
      MotionMoment::Submit => matches!(self, Self::Submit | Self::Both),
      MotionMoment::Success | MotionMoment::Failure => false,
      // Beat is not a `MotionMoment`; callers ask `beat()` instead.
    }
  }

  #[must_use]
  pub fn beat(self) -> bool {
    matches!(self, Self::Beat | Self::Both)
  }
}

/// When the glitch slices are allowed to fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlitchOn {
  #[default]
  Failure,
  Submit,
  Both,
}

impl GlitchOn {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "failure" => Some(Self::Failure),
      "submit" => Some(Self::Submit),
      "both" => Some(Self::Both),
      _ => None,
    }
  }

  #[must_use]
  pub fn accepts(self, moment: MotionMoment) -> bool {
    match moment {
      MotionMoment::Submit => matches!(self, Self::Submit | Self::Both),
      MotionMoment::Failure => matches!(self, Self::Failure | Self::Both),
      MotionMoment::Success => false,
    }
  }
}

/// Full-screen post effects. Every field defaults to a no-op.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostConfig {
  /// Speed-line strength, 0 to 1.
  pub speed_lines: f32,
  pub speed_lines_on: SpeedLinesOn,
  /// Letterbox bar height as a fraction of the screen height, 0 to 0.2.
  pub letterbox: f32,
  /// Time the bars take to slide in on boot or out on success, 0 to 2000 ms.
  pub letterbox_animate_ms: f32,
  /// Static radial darkening, 0 to 1.
  pub vignette: f32,
  /// RGB channel offset strength, 0 to 1. GPU only.
  pub chromatic: f32,
  /// Glitch slice window, 0 to 600 ms. Zero is off.
  pub glitch_ms: f32,
  pub glitch_on: GlitchOn,
}

impl Default for PostConfig {
  fn default() -> Self {
    Self {
      speed_lines: 0.0,
      speed_lines_on: SpeedLinesOn::Submit,
      letterbox: 0.0,
      letterbox_animate_ms: 400.0,
      vignette: 0.0,
      chromatic: 0.0,
      glitch_ms: 0.0,
      glitch_on: GlitchOn::Failure,
    }
  }
}

impl PostConfig {
  /// How long a speed-line punch stays on screen. The intensity stretches the
  /// window from 400 to 900 ms, so a stronger effect also reads longer.
  #[must_use]
  pub fn speed_window_ms(&self) -> f32 {
    let intensity = if self.speed_lines.is_finite() {
      self.speed_lines.clamp(0.0, 1.0)
    } else {
      0.0
    };
    400.0 + 500.0 * intensity
  }
}
