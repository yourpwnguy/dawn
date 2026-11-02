//! Theme presets and the resolved visual token set.

use super::*;

/// One of the shipped design presets. A preset only supplies defaults; every
/// token can still be overridden in `[theme]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
  #[default]
  GlassDark,
  GlassLight,
  Minimal,
}

impl ThemePreset {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "glass-dark" | "dark" => Some(Self::GlassDark),
      "glass-light" | "light" => Some(Self::GlassLight),
      "minimal" => Some(Self::Minimal),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::GlassDark => "glass-dark",
      Self::GlassLight => "glass-light",
      Self::Minimal => "minimal",
    }
  }

  /// The background colour used when the user did not set one. This is what
  /// makes the light preset usable with zero config.
  #[must_use]
  pub fn background_color(self) -> Rgb {
    match self {
      Self::GlassDark => Rgb::solid(11, 16, 32),
      Self::GlassLight => Rgb::solid(223, 232, 245),
      Self::Minimal => Rgb::solid(16, 17, 20),
    }
  }

  /// Resolved design tokens for this preset.
  #[must_use]
  pub fn tokens(self) -> ThemeConfig {
    match self {
      Self::GlassDark => ThemeConfig {
        preset: self,
        text: Rgb::solid(242, 246, 255),
        muted: Rgb::solid(169, 180, 200),
        accent: Rgb::solid(125, 177, 255),
        error: Rgb::solid(255, 138, 128),
        tint: Rgb::rgba(16, 23, 38, 204),
        border: Rgb::rgba(255, 255, 255, 64),
        shadow: Rgb::rgba(0, 0, 0, 102),
        focus_ring: Rgb::solid(125, 177, 255),
        blur: BlurConfig {
          refresh: Duration::from_millis(250),
          ..BlurConfig::default()
        },
        corner_radius: 18.0,
        spacing: 1.0,
        noise: true,
        min_contrast: 4.5,
        scanlines: 0.0,
        scanline_gap: 3,
        scanline_ms: 6000.0,
      },
      Self::GlassLight => ThemeConfig {
        preset: self,
        text: Rgb::solid(16, 19, 24),
        muted: Rgb::solid(77, 86, 101),
        accent: Rgb::solid(31, 111, 235),
        error: Rgb::solid(180, 35, 24),
        tint: Rgb::rgba(247, 249, 252, 210),
        border: Rgb::rgba(255, 255, 255, 204),
        shadow: Rgb::rgba(0, 0, 0, 51),
        focus_ring: Rgb::solid(31, 111, 235),
        blur: BlurConfig {
          refresh: Duration::from_millis(250),
          ..BlurConfig::default()
        },
        corner_radius: 18.0,
        spacing: 1.0,
        noise: true,
        min_contrast: 4.5,
        scanlines: 0.0,
        scanline_gap: 3,
        scanline_ms: 6000.0,
      },
      Self::Minimal => ThemeConfig {
        preset: self,
        text: Rgb::solid(245, 245, 245),
        muted: Rgb::solid(160, 160, 160),
        accent: Rgb::solid(158, 203, 255),
        error: Rgb::solid(255, 148, 140),
        tint: Rgb::rgba(23, 24, 28, 242),
        border: Rgb::rgba(255, 255, 255, 34),
        shadow: Rgb::rgba(0, 0, 0, 85),
        focus_ring: Rgb::solid(158, 203, 255),
        blur: BlurConfig {
          enabled: false,
          refresh: Duration::from_millis(500),
          ..BlurConfig::default()
        },
        corner_radius: 10.0,
        spacing: 0.95,
        noise: false,
        min_contrast: 4.5,
        scanlines: 0.0,
        scanline_gap: 3,
        scanline_ms: 6000.0,
      },
    }
  }
}

#[derive(Debug, Clone)]
pub struct ThemeConfig {
  pub preset: ThemePreset,
  pub text: Rgb,
  pub muted: Rgb,
  pub accent: Rgb,
  pub error: Rgb,
  /// Glass tint. The alpha channel is the tint opacity.
  pub tint: Rgb,
  pub border: Rgb,
  pub shadow: Rgb,
  pub focus_ring: Rgb,
  /// Backdrop blur, using Hyprland's `decoration:blur` vocabulary and a CPU
  /// port of its dual Kawase pyramid.
  pub blur: BlurConfig,
  pub corner_radius: f32,
  /// Multiplier for padding and gaps.
  pub spacing: f32,
  pub noise: bool,
  /// Minimum contrast ratio for text against the glass tint.
  pub min_contrast: f32,
  /// CRT scanline strength, 0 to 1. Zero turns the overlay off. The lines are
  /// painted straight onto the wallpaper, before the glass and the form, so
  /// they texture the backdrop instead of the text.
  pub scanlines: f32,
  /// Physical pixel spacing between scanlines, 2 to 12. Smaller values are a
  /// denser, finer CRT; the default reads as a subtle aperture grille.
  pub scanline_gap: u32,
  /// Time for the bright sweep band to travel the whole screen once, in
  /// milliseconds, 1000 to 20000. Reduced motion keeps the static lines and
  /// drops the sweep.
  pub scanline_ms: f32,
}

/// Backdrop blur settings.
///
/// The names and defaults mirror Hyprland's `decoration:blur` block, because
/// that is the look people ask for when they say "blur like my compositor".
/// The CPU implementation uses the same prepare, downsample, upsample, and
/// finish stages; only colour management and xray are not applicable to a
/// single full-screen greeter.
#[derive(Debug, Clone)]
pub struct BlurConfig {
  pub enabled: bool,
  /// Blur radius in logical pixels, Hyprland's `size`.
  pub size: f32,
  /// Number of downsample/upsample pairs, 1 to 8.
  pub passes: u32,
  /// Per-pixel grain amount, Hyprland's `noise`.
  pub noise: f32,
  /// Contrast curve applied before blurring, Hyprland's `contrast`.
  pub contrast: f32,
  /// Brightness applied before and after blurring, Hyprland's `brightness`.
  pub brightness: f32,
  /// Saturation boost during the downsample, Hyprland's `vibrancy`.
  pub vibrancy: f32,
  /// How much dark colours are protected from the vibrancy boost.
  pub vibrancy_darkness: f32,
  /// How often an animated wallpaper refreshes the cached blur.
  pub refresh: Duration,
}

impl Default for BlurConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      size: 8.0,
      passes: 3,
      noise: 0.0117,
      contrast: 0.8916,
      brightness: 0.8172,
      vibrancy: 0.1696,
      vibrancy_darkness: 0.0,
      refresh: Duration::from_millis(300),
    }
  }
}
