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
use crate::config::{
  BlurConfig, ClockStyle, Config, DecorSpec, LoginConfig, MotionConfig, SecondHand, WidgetLayer,
  WidgetLook,
};
use crate::layout::Anchor;
use crate::ui::image::{DecodeLimits, load_native};

/// A cosmetic image, text run, or plate inside the login card.
#[derive(Debug, Clone)]
pub struct Decor {
  pub image: Option<Pixmap>,
  pub text: String,
  pub size: f32,
  pub color: Rgb,
  pub bold: bool,
  pub tracking: f32,
  pub width: f32,
  /// Panel height. Zero for text and images.
  pub height: f32,
  pub opacity: f32,
  pub anchor: Anchor,
  pub offset: (i32, i32),
  pub rotation: f32,
  pub motion: crate::config::DecorMotion,
  /// True for a `kind = "panel"` item. Panels have no image or text, so an
  /// explicit flag keeps the renderer from guessing based on empty strings.
  pub panel: bool,
  /// Optional second panel colour. When set, the plate is a linear gradient
  /// from `color` to `color_end` at `gradient_angle`.
  pub color_end: Option<Rgb>,
  pub gradient_angle: f32,
  pub radius: f32,
  pub border: Option<Rgb>,
  /// Optional text outline. The renderer draws the same run eight times around
  /// a small circle before the fill pass.
  pub stroke: Option<Rgb>,
  pub stroke_width: f32,
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
  /// CRT scanline strength, 0 to 1. See `ThemeConfig::scanlines`.
  pub scanlines: f32,
  /// Physical pixel spacing between scanlines.
  pub scanline_gap: u32,
  /// Sweep period in milliseconds. The static lines ignore it; the moving band
  /// uses it as its cycle time.
  pub scanline_ms: f32,
  pub font_size: f32,
  pub card_width: f32,
  pub greeting: String,
  /// The full login layout. Kept here so the renderer and the app read the
  /// same resolved values without threading the whole `Config` around.
  pub login: LoginConfig,
  /// Decorative images, text runs, and panels placed inside the login card.
  /// Images are decoded once at startup; a missing file is skipped with a
  /// warning.
  pub decor: Vec<Decor>,
  pub blur: BlurConfig,
  pub corner_radius: f32,
  pub spacing: f32,
  pub noise: bool,
  pub motion: MotionConfig,
  pub low_power: bool,
  /// Draw order for the built-in clock. The renderer reads this instead of the
  /// whole clock config, so the theme stays a bag of resolved values.
  pub clock_layer: WidgetLayer,
  /// Draw order for the built-in media widget. See [`Self::clock_layer`].
  pub media_layer: WidgetLayer,
  /// Resolved plate, motion, and state rules for the built-in clock.
  pub clock_look: WidgetLook,
  /// Resolved plate, motion, and state rules for the built-in media widget.
  pub media_look: WidgetLook,
  /// True when the clock face changes between whole seconds: a blinking colon
  /// or a sweeping analog second hand. Those need animation frames rather than
  /// the once-per-second clock tick, otherwise the effect looks like a
  /// slideshow.
  pub clock_face_animating: bool,
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
  /// True when at least one decor item or built-in widget is animated and
  /// motion is allowed. The window uses this to keep requesting animation
  /// frames; reduced motion and low-power mode freeze the ornaments and the
  /// widget plates along with the rest of the UI.
  #[must_use]
  pub fn decor_motion_active(&self) -> bool {
    !self.motion.reduced
      && !self.low_power
      && (self.decor.iter().any(|item| item.motion.active())
        || self.clock_look.motion.active()
        || self.media_look.motion.active()
        || self.clock_face_animating
        || self.scanlines > 0.0)
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
      scanlines: config.theme.scanlines,
      scanline_gap: config.theme.scanline_gap,
      scanline_ms: config.theme.scanline_ms,
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
      clock_layer: config.clock.layer,
      media_layer: config.media.layer,
      clock_look: config.clock.look.clone(),
      media_look: config.media.presentation.clone(),
      clock_face_animating: config.clock.enabled
        && (config.clock.colon_blink_ms > 0.0
          || (config.clock.style == ClockStyle::Analog
            && config.clock.second_hand == SecondHand::Sweep)),
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
      stroke,
      stroke_width,
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
      height: 0.0,
      opacity: 1.0,
      anchor: *anchor,
      offset: *offset,
      rotation: *rotation,
      motion: *motion,
      panel: false,
      color_end: None,
      gradient_angle: 0.0,
      radius: 0.0,
      border: None,
      stroke: *stroke,
      stroke_width: *stroke_width,
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
        height: 0.0,
        opacity: *opacity,
        anchor: *anchor,
        offset: *offset,
        rotation: *rotation,
        motion: *motion,
        panel: false,
        color_end: None,
        gradient_angle: 0.0,
        radius: 0.0,
        border: None,
        stroke: None,
        stroke_width: 0.0,
      })
    }
    DecorSpec::Panel {
      width,
      height,
      color,
      color_end,
      gradient_angle,
      radius,
      border,
      opacity,
      anchor,
      offset,
      rotation,
      motion,
    } => Some(Decor {
      image: None,
      text: String::new(),
      size: 0.0,
      // A panel without a colour is just a piece of the card tint. That keeps
      // a quick `kind = "panel"` useful without forcing a hex value.
      color: color.unwrap_or(config.theme.tint),
      bold: false,
      tracking: 0.0,
      width: *width,
      height: *height,
      opacity: *opacity,
      anchor: *anchor,
      offset: *offset,
      rotation: *rotation,
      motion: *motion,
      panel: true,
      color_end: *color_end,
      gradient_angle: *gradient_angle,
      radius: *radius,
      border: *border,
      stroke: None,
      stroke_width: 0.0,
    }),
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
