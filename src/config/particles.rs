//! Ambient and burst particle configuration.

use super::*;

/// The procedural sprite an ambient particle uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParticleSprite {
  /// A radial ember blob with a flickering hot core.
  #[default]
  Flame,
  /// Two mirrored triangle wings that flap.
  Butterfly,
  /// A plain soft circle.
  Dot,
  /// A thin vertical light streak. Ambient use reads as a stage spotlight
  /// falling through the frame; burst use gives the success moment a diving,
  /// rain-like energy without another sprite atlas.
  Streak,
}

impl ParticleSprite {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "flame" | "ember" => Some(Self::Flame),
      "butterfly" | "moth" => Some(Self::Butterfly),
      "dot" | "sphere" => Some(Self::Dot),
      "streak" | "ray" => Some(Self::Streak),
      _ => None,
    }
  }
}

/// What starts the one-shot particle burst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BurstTrigger {
  /// No burst. This is the default and leaves the ambient emitter alone.
  #[default]
  None,
  /// Fire once when the success transition starts.
  Success,
}

impl BurstTrigger {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "none" | "off" => Some(Self::None),
      "success" => Some(Self::Success),
      _ => None,
    }
  }
}

/// Which way the particles travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParticleDrift {
  #[default]
  Up,
  Down,
  /// A gentle sine wander with no net travel.
  Float,
}

impl ParticleDrift {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "up" => Some(Self::Up),
      "down" => Some(Self::Down),
      "float" => Some(Self::Float),
      _ => None,
    }
  }
}

/// Whether the emitter paints under the UI or over everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParticleLayer {
  /// Over the wallpaper, under the login form and widgets.
  #[default]
  Behind,
  /// Topmost, after the form and warnings.
  Front,
}

impl ParticleLayer {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "behind" => Some(Self::Behind),
      "front" => Some(Self::Front),
      _ => None,
    }
  }
}

/// Ambient, procedurally drawn emitters.
///
/// The sprites are generated at draw time, so a theme never needs image files
/// for them. Their positions are derived from the particle index and the
/// session clock, which makes every run deterministic and keeps a screenshot
/// stable. Disabled by default: an existing config renders exactly as before.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticlesConfig {
  pub enabled: bool,
  /// Number of live particles, capped at [`MAX_PARTICLES`] at load time.
  pub count: u32,
  pub sprite: ParticleSprite,
  /// Flame core and the first butterfly wing.
  pub color: Rgb,
  /// The second butterfly wing.
  pub color2: Rgb,
  pub drift: ParticleDrift,
  /// Base travel speed in logical pixels per second.
  pub speed: f32,
  /// Base sprite size in logical pixels.
  pub size: f32,
  /// Overall opacity multiplier, 0 to 1.
  pub opacity: f32,
  pub layer: ParticleLayer,
  /// One-shot burst source. Off by default, so the ambient field is unchanged.
  pub burst_on: BurstTrigger,
  /// Sprite used by the burst. Independent of the ambient `sprite`.
  pub burst_sprite: ParticleSprite,
  /// Number of sprites in the burst, capped at [`MAX_BURST_PARTICLES`].
  pub burst_count: u32,
  /// Fire the burst once per `[motion.beat]` beat. Off by default, and it
  /// still needs `enabled = true` because the burst paints through the normal
  /// emitter path.
  pub burst_on_beat: bool,
}

/// Hard cap for `[particles].burst_count`. The burst is one shot, but it still
/// paints every frame until it dies, so the same budget rule applies.
pub const MAX_BURST_PARTICLES: u32 = 64;

/// Seconds a burst sprite lives. After this the sample function returns
/// `None`, which is how the window knows the burst is over without keeping a
/// mutable spawn table.
pub const BURST_LIFETIME: f32 = 1.8;

/// Duration of the ticker flash when the selected session changes. It is one
/// beat of the montage, not a sustained highlight.
pub const SESSION_FLASH_MS: f32 = 80.0;

/// Hard cap for `[particles].count`, enforced by validation and again by the
/// emitter so a hand-built config cannot turn the greeter into a space heater.
pub const MAX_PARTICLES: u32 = 96;

impl Default for ParticlesConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      count: 48,
      sprite: ParticleSprite::Flame,
      color: Rgb::solid(0xFF, 0x7A, 0x3C),
      color2: Rgb::solid(0xD4, 0x18, 0x30),
      drift: ParticleDrift::Up,
      speed: 24.0,
      size: 6.0,
      opacity: 0.55,
      layer: ParticleLayer::Behind,
      burst_on: BurstTrigger::None,
      burst_sprite: ParticleSprite::Butterfly,
      burst_count: 24,
      burst_on_beat: false,
    }
  }
}
