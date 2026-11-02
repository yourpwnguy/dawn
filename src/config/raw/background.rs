//! Background section of the raw config schema.

use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBackground {
  #[serde(default)]
  pub kind: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub gradient: Option<Field>,
  #[serde(default)]
  pub image: Option<Field>,
  #[serde(default)]
  pub audio: Option<Field>,
  #[serde(default)]
  pub fit: Option<Field>,
  #[serde(default)]
  pub fps: Option<Field>,
  #[serde(default)]
  pub memory_limit_mb: Option<Field>,
  #[serde(default)]
  pub idle_pause_secs: Option<Field>,
  #[serde(default)]
  pub monitors: BTreeMap<String, RawBackgroundOverride>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBackgroundOverride {
  #[serde(default)]
  pub kind: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub image: Option<Field>,
  #[serde(default)]
  pub fit: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTheme {
  /// Select a complete theme bundle from the theme store. The bundle's own
  /// config replaces this file; it is a pointer, not an overlay.
  #[serde(default)]
  pub package: Option<Field>,
  #[serde(default)]
  pub preset: Option<Field>,
  #[serde(default)]
  pub text: Option<Field>,
  #[serde(default)]
  pub muted: Option<Field>,
  #[serde(default)]
  pub accent: Option<Field>,
  #[serde(default)]
  pub error: Option<Field>,
  /// Legacy name for the glass tint.
  #[serde(default)]
  pub card: Option<Field>,
  #[serde(default)]
  pub tint: Option<Field>,
  #[serde(default)]
  pub border: Option<Field>,
  #[serde(default)]
  pub shadow: Option<Field>,
  #[serde(default)]
  pub focus_ring: Option<Field>,
  /// Legacy alias for `[theme.blur].size`.
  #[serde(default)]
  pub blur_radius: Option<Field>,
  /// Legacy alias for `[theme.blur].refresh_ms`.
  #[serde(default)]
  pub blur_refresh_ms: Option<Field>,
  /// Accepted for old configs; the dual Kawase pyramid does not use a single
  /// downsample factor.
  #[serde(default)]
  pub blur_downsample: Option<Field>,
  #[serde(default)]
  pub blur: RawBlur,
  #[serde(default)]
  pub corner_radius: Option<Field>,
  #[serde(default)]
  pub spacing: Option<Field>,
  #[serde(default)]
  pub noise: Option<Field>,
  #[serde(default)]
  pub min_contrast: Option<Field>,
  /// CRT scanline strength, 0 to 1.
  #[serde(default)]
  pub scanlines: Option<Field>,
  /// Physical pixel spacing between scanlines, 2 to 12.
  #[serde(default)]
  pub scanline_gap: Option<Field>,
  /// Sweep period in milliseconds, 1000 to 20000.
  #[serde(default)]
  pub scanline_ms: Option<Field>,
}

/// Hyprland's `decoration:blur` block, minus the compositor-only switches.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBlur {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub passes: Option<Field>,
  #[serde(default)]
  pub noise: Option<Field>,
  #[serde(default)]
  pub contrast: Option<Field>,
  #[serde(default)]
  pub brightness: Option<Field>,
  #[serde(default)]
  pub vibrancy: Option<Field>,
  #[serde(default)]
  pub vibrancy_darkness: Option<Field>,
  #[serde(default)]
  pub refresh_ms: Option<Field>,
  // Compositor-only switches accepted so a Hyprland blur block can be pasted
  // without a wall of warnings. A single full-screen greeter is always opaque,
  // so none of these change its pixels.
  #[serde(default)]
  pub ignore_opacity: Option<Field>,
  #[serde(default)]
  pub new_optimizations: Option<Field>,
  #[serde(default)]
  pub xray: Option<Field>,
  #[serde(default)]
  pub special: Option<Field>,
  #[serde(default)]
  pub popups: Option<Field>,
  #[serde(default)]
  pub popup_ignore_alpha: Option<Field>,
}
