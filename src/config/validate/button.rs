//! Button validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn button_config(
    &mut self,
    raw: &crate::config::raw::RawButton,
    default: &crate::config::ButtonConfig,
  ) -> crate::config::ButtonConfig {
    crate::config::ButtonConfig {
      show: self.boolean(raw.show.as_ref(), "login.button.show", default.show),
      text: self
        .string_value(
          raw.text.as_ref(),
          "login.button.text",
          "expected button text",
          64,
        )
        .unwrap_or_else(|| default.text.clone()),
      gap: self.range(
        raw.gap.as_ref(),
        "login.button.gap",
        f64::from(default.gap),
        0.0,
        160.0,
      ) as f32,
      height: self.range(
        raw.height.as_ref(),
        "login.button.height",
        f64::from(default.height),
        20.0,
        160.0,
      ) as f32,
      radius: self.range(
        raw.radius.as_ref(),
        "login.button.radius",
        f64::from(default.radius),
        0.0,
        64.0,
      ) as f32,
      font_size: self.range(
        raw.font_size.as_ref(),
        "login.button.font_size",
        f64::from(default.font_size),
        0.0,
        96.0,
      ) as f32,
      background: self.optional_color(raw.background.as_ref(), "login.button.background"),
      background_end: self
        .optional_color(raw.background_end.as_ref(), "login.button.background_end"),
      gradient_angle: self.range(
        raw.gradient_angle.as_ref(),
        "login.button.gradient_angle",
        f64::from(default.gradient_angle),
        -360.0,
        360.0,
      ) as f32,
      text_color: self.optional_color(raw.text_color.as_ref(), "login.button.text_color"),
      bracket: self.optional_color(raw.bracket.as_ref(), "login.button.bracket"),
      bracket_gap: self.range(
        raw.bracket_gap.as_ref(),
        "login.button.bracket_gap",
        f64::from(default.bracket_gap),
        0.0,
        64.0,
      ) as f32,
      bold: self.boolean(raw.bold.as_ref(), "login.button.bold", default.bold),
      tracking: self.range(
        raw.tracking.as_ref(),
        "login.button.tracking",
        f64::from(default.tracking),
        0.0,
        0.5,
      ) as f32,
      hover_background: self.optional_color(
        raw.hover_background.as_ref(),
        "login.button.hover_background",
      ),
      hover_background_end: self.optional_color(
        raw.hover_background_end.as_ref(),
        "login.button.hover_background_end",
      ),
      hover_text_color: self.optional_color(
        raw.hover_text_color.as_ref(),
        "login.button.hover_text_color",
      ),
      hover_direction: self.hover_reveal(raw.hover_direction.as_ref(), default.hover_direction),
      hover_ms: self.range(
        raw.hover_ms.as_ref(),
        "login.button.hover_ms",
        f64::from(default.hover_ms),
        0.0,
        5000.0,
      ) as f32,
      hover_scale: self.range(
        raw.hover_scale.as_ref(),
        "login.button.hover_scale",
        f64::from(default.hover_scale),
        0.5,
        2.0,
      ) as f32,
      hover_lift: self.range(
        raw.hover_lift.as_ref(),
        "login.button.hover_lift",
        f64::from(default.hover_lift),
        0.0,
        64.0,
      ) as f32,
      glow: self.optional_color(raw.glow.as_ref(), "login.button.glow"),
      glow_size: self.range(
        raw.glow_size.as_ref(),
        "login.button.glow_size",
        f64::from(default.glow_size),
        0.0,
        64.0,
      ) as f32,
      effect: self.button_effect(raw.effect.as_ref(), default.effect),
      effect_color: self.optional_color(raw.effect_color.as_ref(), "login.button.effect_color"),
      effect_ms: self.range(
        raw.effect_ms.as_ref(),
        "login.button.effect_ms",
        f64::from(default.effect_ms),
        100.0,
        20000.0,
      ) as f32,
      effect_angle: self.range(
        raw.effect_angle.as_ref(),
        "login.button.effect_angle",
        f64::from(default.effect_angle),
        -360.0,
        360.0,
      ) as f32,
      effect_strength: self.range(
        raw.effect_strength.as_ref(),
        "login.button.effect_strength",
        f64::from(default.effect_strength),
        0.0,
        1.0,
      ) as f32,
      rotation: self.rotation(
        raw.rotation.as_ref(),
        "login.button.rotation",
        default.rotation,
      ),
    }
  }
}
