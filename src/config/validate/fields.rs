//! Fields validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn icon_name(&mut self, field: Option<&Field>, key: &str, default: &str) -> String {
    let Some(field) = field else {
      return default.to_string();
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected user, lock, or none");
      return default.to_string();
    };
    match value {
      "" | "none" => String::new(),
      "user" | "lock" => value.to_string(),
      _ => {
        self.issue(key, Some(field), "expected user, lock, or none");
        default.to_string()
      }
    }
  }

  pub(super) fn field_config(
    &mut self,
    shared: &RawField,
    own: &RawField,
    prefix: &str,
    default: &FieldConfig,
  ) -> FieldConfig {
    let key = |name: &str| format!("{prefix}.{name}");
    let text =
      |this: &mut Self, own: Option<&Field>, shared: Option<&Field>, name: &str, fallback: &str| {
        this
          .string_value(own.or(shared), &key(name), "expected a short string", 128)
          .unwrap_or_else(|| fallback.to_string())
      };
    FieldConfig {
      label: text(
        self,
        own.label.as_ref(),
        shared.label.as_ref(),
        "label",
        &default.label,
      ),
      label_color: self.optional_color(
        own.label_color.as_ref().or(shared.label_color.as_ref()),
        &key("label_color"),
      ),
      label_size: self.range(
        own.label_size.as_ref().or(shared.label_size.as_ref()),
        &key("label_size"),
        f64::from(default.label_size),
        0.0,
        64.0,
      ) as f32,
      label_tracking: self.range(
        own
          .label_tracking
          .as_ref()
          .or(shared.label_tracking.as_ref()),
        &key("label_tracking"),
        f64::from(default.label_tracking),
        0.0,
        0.5,
      ) as f32,
      placeholder: text(
        self,
        own.placeholder.as_ref(),
        shared.placeholder.as_ref(),
        "placeholder",
        &default.placeholder,
      ),
      font_size: self.range(
        own.font_size.as_ref().or(shared.font_size.as_ref()),
        &key("font_size"),
        f64::from(default.font_size),
        0.0,
        96.0,
      ) as f32,
      height: self.range(
        own.height.as_ref().or(shared.height.as_ref()),
        &key("height"),
        f64::from(default.height),
        0.0,
        160.0,
      ) as f32,
      radius: self.range(
        own.radius.as_ref().or(shared.radius.as_ref()),
        &key("radius"),
        f64::from(default.radius),
        0.0,
        64.0,
      ) as f32,
      padding: self.range(
        own.padding.as_ref().or(shared.padding.as_ref()),
        &key("padding"),
        f64::from(default.padding),
        0.0,
        64.0,
      ) as f32,
      background: self.optional_color(
        own.background.as_ref().or(shared.background.as_ref()),
        &key("background"),
      ),
      background_end: self.optional_color(
        own
          .background_end
          .as_ref()
          .or(shared.background_end.as_ref()),
        &key("background_end"),
      ),
      border: self.optional_color(
        own.border.as_ref().or(shared.border.as_ref()),
        &key("border"),
      ),
      underline: self.optional_color(
        own.underline.as_ref().or(shared.underline.as_ref()),
        &key("underline"),
      ),
      underline_size: self.range(
        own
          .underline_size
          .as_ref()
          .or(shared.underline_size.as_ref()),
        &key("underline_size"),
        f64::from(default.underline_size),
        0.0,
        12.0,
      ) as f32,
      bracket: self.optional_color(
        own.bracket.as_ref().or(shared.bracket.as_ref()),
        &key("bracket"),
      ),
      bracket_gap: self.range(
        own.bracket_gap.as_ref().or(shared.bracket_gap.as_ref()),
        &key("bracket_gap"),
        f64::from(default.bracket_gap),
        0.0,
        64.0,
      ) as f32,
      text: self.optional_color(own.text.as_ref().or(shared.text.as_ref()), &key("text")),
      placeholder_color: self.optional_color(
        own
          .placeholder_color
          .as_ref()
          .or(shared.placeholder_color.as_ref()),
        &key("placeholder_color"),
      ),
      focus: self.optional_color(own.focus.as_ref().or(shared.focus.as_ref()), &key("focus")),
      caret: self.optional_color(own.caret.as_ref().or(shared.caret.as_ref()), &key("caret")),
      caret_width: self.range(
        own.caret_width.as_ref().or(shared.caret_width.as_ref()),
        &key("caret_width"),
        f64::from(default.caret_width),
        0.0,
        16.0,
      ) as f32,
      caret_style: self.caret_style(
        own.caret_style.as_ref().or(shared.caret_style.as_ref()),
        &key("caret_style"),
        default.caret_style,
      ),
      icon: self.icon_name(
        own.icon.as_ref().or(shared.icon.as_ref()),
        &key("icon"),
        &default.icon,
      ),
      icon_size: self.range(
        own.icon_size.as_ref().or(shared.icon_size.as_ref()),
        &key("icon_size"),
        f64::from(default.icon_size),
        0.0,
        64.0,
      ) as f32,
      icon_gap: self.range(
        own.icon_gap.as_ref().or(shared.icon_gap.as_ref()),
        &key("icon_gap"),
        f64::from(default.icon_gap),
        0.0,
        64.0,
      ) as f32,
      icon_color: self.optional_color(
        own.icon_color.as_ref().or(shared.icon_color.as_ref()),
        &key("icon_color"),
      ),
      rotation: self.rotation(
        own.rotation.as_ref().or(shared.rotation.as_ref()),
        &key("rotation"),
        default.rotation,
      ),
      mask: text(
        self,
        own.mask.as_ref(),
        shared.mask.as_ref(),
        "mask",
        &default.mask,
      ),
      key_pop_ms: self.range(
        own.key_pop_ms.as_ref().or(shared.key_pop_ms.as_ref()),
        &key("key_pop_ms"),
        f64::from(default.key_pop_ms),
        0.0,
        2000.0,
      ) as f32,
      key_pop_scale: self.range(
        own.key_pop_scale.as_ref().or(shared.key_pop_scale.as_ref()),
        &key("key_pop_scale"),
        f64::from(default.key_pop_scale),
        1.0,
        2.5,
      ) as f32,
      key_pop_color: self.optional_color(
        own.key_pop_color.as_ref().or(shared.key_pop_color.as_ref()),
        &key("key_pop_color"),
      ),
    }
  }
}
