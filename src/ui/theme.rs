//! Resolved design tokens derived from config.
//!
//! The renderer never reads `ThemeConfig` directly. It gets a [`Theme`] with
//! every colour already checked for legibility against the glass tint, so a
//! theme change cannot accidentally make the password prompt unreadable. The
//! preset supplies sensible values; `[theme]` overrides them field by field.

use std::path::Path;

use tiny_skia::Pixmap;
use tracing::warn;

use crate::color::Rgb;
use crate::config::{BlurConfig, Config, DecorSpec, LoginConfig, MotionConfig};
use crate::layout::Anchor;
use crate::ui::image::{DecodeLimits, load_native};

/// A cosmetic image or text run inside the login card.
#[derive(Debug, Clone)]
pub struct Decor {
  pub image: Option<Pixmap>,
  pub text: String,
  pub size: f32,
  pub color: Rgb,
  pub bold: bool,
  pub tracking: f32,
  pub width: f32,
  pub opacity: f32,
  pub anchor: Anchor,
  pub offset: (i32, i32),
  pub rotation: f32,
  pub motion: crate::config::DecorMotion,
}

#[derive(Debug, Clone)]
pub struct Theme {
  pub text: Rgb,
  pub muted: Rgb,
  pub accent: Rgb,
  pub error: Rgb,
  /// Glass tint. Alpha is the tint opacity.
  pub tint: Rgb,
  pub border: Rgb,
  pub shadow: Rgb,
  pub focus_ring: Rgb,
  pub min_contrast: f32,
  pub font_size: f32,
  pub card_width: f32,
  pub greeting: String,
  /// The full login layout. Kept here so the renderer and the app read the
  /// same resolved values without threading the whole `Config` around.
  pub login: LoginConfig,
  /// Decorative images and text placed inside the login card. Images are
  /// decoded once at startup; a missing file is skipped with a warning.
  pub decor: Vec<Decor>,
  pub blur: BlurConfig,
  pub corner_radius: f32,
  pub spacing: f32,
  pub noise: bool,
  pub motion: MotionConfig,
  pub low_power: bool,
}

impl Theme {
  /// The colour the glass tint is composited over. It is the config background
  /// colour, which is a conservative stand-in for the actual wallpaper.
  #[must_use]
  pub fn panel_backdrop(&self, background: Rgb) -> Rgb {
    self.tint.over(background)
  }

  /// Primary text, adjusted only when the configured colour would fail the
  /// contrast target against the glass tint.
  #[must_use]
  pub fn primary_on(&self, background: Rgb) -> Rgb {
    self
      .text
      .ensure_contrast(self.panel_backdrop(background), self.min_contrast)
  }

  /// Secondary text still has to meet the configured target; the preset
  /// colours are chosen with enough headroom that this rarely rewrites them.
  #[must_use]
  pub fn muted_on(&self, background: Rgb) -> Rgb {
    self
      .muted
      .ensure_contrast(self.panel_backdrop(background), self.min_contrast)
  }

  /// Error text is information the user needs to read, so it gets the same
  /// target as primary text.
  #[must_use]
  pub fn error_on(&self, background: Rgb) -> Rgb {
    self
      .error
      .ensure_contrast(self.panel_backdrop(background), self.min_contrast)
  }

  /// The accent is used for borders and highlights, not body text.
  #[must_use]
  pub fn accent_on(&self, background: Rgb) -> Rgb {
    self
      .accent
      .ensure_contrast(self.panel_backdrop(background), self.min_contrast)
  }

  /// Focus rings use their own token so a theme can make focus louder than
  /// the accent used for links and selections, but they still have to clear
  /// the contrast target against the glass.
  #[must_use]
  pub fn focus_on(&self, background: Rgb) -> Rgb {
    self
      .focus_ring
      .ensure_contrast(self.panel_backdrop(background), self.min_contrast)
  }
  /// True when at least one decor item is animated and motion is allowed.
  /// The window uses this to keep requesting animation frames; reduced motion
  /// and low-power mode freeze the ornaments along with the rest of the UI.
  #[must_use]
  pub fn decor_motion_active(&self) -> bool {
    !self.motion.reduced && !self.low_power && self.decor.iter().any(|item| item.motion.active())
  }
}

impl From<&Config> for Theme {
  fn from(config: &Config) -> Self {
    let decor = config
      .login
      .decor
      .iter()
      .filter_map(|spec| decor_from_spec(spec, config))
      .collect();
    Self {
      decor,
      text: config.theme.text,
      muted: config.theme.muted,
      accent: config.theme.accent,
      error: config.theme.error,
      tint: config.theme.tint,
      border: config.theme.border,
      shadow: config.theme.shadow,
      focus_ring: config.theme.focus_ring,
      min_contrast: config.theme.min_contrast,
      font_size: config.ui.font_size,
      card_width: config.login.width,
      greeting: config.login.greeting.clone(),
      login: config.login.clone(),
      blur: config.theme.blur.clone(),
      corner_radius: config.theme.corner_radius,
      spacing: config.theme.spacing,
      noise: config.theme.noise,
      motion: config.motion.clone(),
      low_power: config.power.low_power,
    }
  }
}

/// Loads a decor image once. The renderer scales it to the configured width,
/// so keeping the native size avoids a second resample at every paint.
fn load_decor_image(path: &Path) -> Option<Pixmap> {
  match load_native(path, DecodeLimits::default()) {
    Ok(pixmap) => Some(pixmap),
    Err(error) => {
      warn!(path = %path.display(), %error, "cannot load a login decor image");
      None
    }
  }
}

fn decor_from_spec(spec: &DecorSpec, config: &Config) -> Option<Decor> {
  match spec {
    DecorSpec::Text {
      text,
      size,
      color,
      bold,
      tracking,
      anchor,
      offset,
      rotation,
      motion,
    } => Some(Decor {
      image: None,
      text: text.clone(),
      size: *size,
      color: color.unwrap_or(config.theme.text),
      bold: *bold,
      tracking: *tracking,
      width: 0.0,
      opacity: 1.0,
      anchor: *anchor,
      offset: *offset,
      rotation: *rotation,
      motion: *motion,
    }),
    DecorSpec::Image {
      path,
      width,
      anchor,
      offset,
      opacity,
      rotation,
      motion,
    } => {
      let image = load_decor_image(path)?;
      Some(Decor {
        image: Some(image),
        text: String::new(),
        size: 0.0,
        color: config.theme.text,
        bold: false,
        tracking: 0.0,
        width: *width,
        opacity: *opacity,
        anchor: *anchor,
        offset: *offset,
        rotation: *rotation,
        motion: *motion,
      })
    }
  }
}

#[cfg(test)]
mod tests {
  use super::Theme;
  use crate::config::{Config, ThemePreset};

  fn theme(preset: ThemePreset) -> Theme {
    let config = Config {
      theme: preset.tokens(),
      ..Config::default()
    };
    Theme::from(&config)
  }

  #[test]
  fn all_presets_keep_body_text_legible() {
    for preset in [
      ThemePreset::GlassDark,
      ThemePreset::GlassLight,
      ThemePreset::Minimal,
    ] {
      let theme = theme(preset);
      let background = preset.background_color();
      let panel = theme.panel_backdrop(background);
      assert!(
        theme.primary_on(background).contrast_ratio(panel) >= 4.5,
        "{preset:?} primary text failed"
      );
      assert!(
        theme.muted_on(background).contrast_ratio(panel) >= theme.min_contrast,
        "{preset:?} muted text failed"
      );
      assert!(
        theme.focus_on(background).contrast_ratio(panel) >= theme.min_contrast,
        "{preset:?} focus ring failed"
      );
    }
  }
}
