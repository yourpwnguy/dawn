//! Motion section of the raw config schema.

use super::*;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSplash {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub ms: Option<Field>,
  #[serde(default)]
  pub title: Option<Field>,
  #[serde(default)]
  pub sub: Option<Field>,
  #[serde(default)]
  pub fade_ms: Option<Field>,
  #[serde(default)]
  pub dismiss_on: Option<Field>,
  #[serde(default)]
  pub bg: Option<Field>,
  #[serde(default)]
  pub accent: Option<Field>,
}

/// `[login.transition]`: a decorative cut sequence played around a submit or
/// a successful authentication. Every key is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTransition {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub trigger: Option<Field>,
  #[serde(default)]
  pub cuts: Option<Field>,
  #[serde(default)]
  pub cut_ms: Option<Field>,
  #[serde(default)]
  pub shake: Option<Field>,
  #[serde(default)]
  pub zoom: Option<Field>,
  #[serde(default)]
  pub slam: Option<Field>,
  #[serde(default)]
  pub slam_ms: Option<Field>,
  #[serde(default)]
  pub slam_color: Option<Field>,
  #[serde(default)]
  pub slam_size: Option<Field>,
}

/// `[particles]`: procedural ambient emitters.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawParticles {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub count: Option<Field>,
  #[serde(default)]
  pub sprite: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub color2: Option<Field>,
  #[serde(default)]
  pub drift: Option<Field>,
  #[serde(default)]
  pub speed: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub opacity: Option<Field>,
  #[serde(default)]
  pub layer: Option<Field>,
  #[serde(default)]
  pub burst_on: Option<Field>,
  #[serde(default)]
  pub burst_sprite: Option<Field>,
  #[serde(default)]
  pub burst_count: Option<Field>,
  #[serde(default)]
  pub burst_on_beat: Option<Field>,
}

/// `[ticker]`: the kinetic marquee.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTicker {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub items: Option<Field>,
  #[serde(default)]
  pub separator: Option<Field>,
  #[serde(default)]
  pub anchor: Option<Field>,
  #[serde(default)]
  pub offset: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub speed: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub bold: Option<Field>,
  #[serde(default)]
  pub tracking: Option<Field>,
  #[serde(default)]
  pub layer: Option<Field>,
  #[serde(default)]
  pub session_first: Option<Field>,
  #[serde(default)]
  pub pulse_on_beat: Option<Field>,
  #[serde(default)]
  pub plate: RawWidgetPlate,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMotion {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub reduced: Option<Field>,
  #[serde(default)]
  pub boot_ms: Option<Field>,
  #[serde(default)]
  pub focus_ms: Option<Field>,
  #[serde(default)]
  pub shake_ms: Option<Field>,
  #[serde(default)]
  pub drag_ms: Option<Field>,
  #[serde(default)]
  pub decor_fps: Option<Field>,
  #[serde(default)]
  pub parallax: Option<Field>,
  #[serde(default)]
  pub beat: RawBeat,
  #[serde(default)]
  pub camera: RawCamera,
  #[serde(default)]
  pub hitstop: RawHitstop,
  #[serde(default)]
  pub post: RawPost,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBeat {
  #[serde(default)]
  pub bpm: Option<Field>,
  #[serde(default)]
  pub offset_ms: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawCamera {
  #[serde(default)]
  pub punch: Option<Field>,
  #[serde(default)]
  pub punch_ms: Option<Field>,
  #[serde(default)]
  pub shake: Option<Field>,
  #[serde(default)]
  pub shake_ms: Option<Field>,
  #[serde(default)]
  pub breathing: Option<Field>,
  #[serde(default)]
  pub breath_ms: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawHitstop {
  #[serde(default)]
  pub submit_ms: Option<Field>,
  #[serde(default)]
  pub success_ms: Option<Field>,
  #[serde(default)]
  pub failure_ms: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPost {
  #[serde(default)]
  pub speed_lines: Option<Field>,
  #[serde(default)]
  pub speed_lines_on: Option<Field>,
  #[serde(default)]
  pub letterbox: Option<Field>,
  #[serde(default)]
  pub letterbox_animate_ms: Option<Field>,
  #[serde(default)]
  pub vignette: Option<Field>,
  #[serde(default)]
  pub chromatic: Option<Field>,
  #[serde(default)]
  pub glitch_ms: Option<Field>,
  #[serde(default)]
  pub glitch_on: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPower {
  #[serde(default)]
  pub low_power: Option<Field>,
}
