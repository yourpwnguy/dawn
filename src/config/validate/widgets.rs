//! Widgets validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn clock_style(&mut self, field: Option<&Field>, default: ClockStyle) -> ClockStyle {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "clock.style",
        Some(field),
        "expected \"digital\", \"analog\", \"flip\", or \"segment\"",
      );
      return default;
    };
    ClockStyle::parse(value).unwrap_or_else(|| {
      self.issue(
        "clock.style",
        Some(field),
        format!("unknown style {value:?}; use digital, analog, flip, or segment"),
      );
      default
    })
  }

  pub(super) fn background_kind(
    &mut self,
    field: Option<&Field>,
    default: BackgroundKind,
    key: &str,
  ) -> BackgroundKind {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        key,
        Some(field),
        "expected auto, solid, gradient, image, animated, or video",
      );
      return default;
    };
    BackgroundKind::parse(value).unwrap_or_else(|| {
      self.issue(
        key,
        Some(field),
        format!("unknown kind {value:?}; use auto, solid, gradient, image, animated, or video"),
      );
      default
    })
  }

  pub(super) fn fit(&mut self, field: Option<&Field>, default: Fit, key: &str) -> Fit {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected cover, contain, or fill");
      return default;
    };
    Fit::parse(value).unwrap_or_else(|| {
      self.issue(
        key,
        Some(field),
        format!("unknown fit {value:?}; use cover, contain, or fill"),
      );
      default
    })
  }

  pub(super) fn anchor(&mut self, field: Option<&Field>, key: &str, default: Anchor) -> Anchor {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected an anchor name");
      return default;
    };
    Anchor::parse(value).unwrap_or_else(|| {
      self.issue(
        key,
        Some(field),
        format!(
          "unknown anchor {value:?}; use top-left, top-center, top-right, center, \
           bottom-left, bottom-center, or bottom-right"
        ),
      );
      default
    })
  }

  pub(super) fn offset(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: (i32, i32),
  ) -> (i32, i32) {
    let Some(field) = field else {
      return default;
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(key, Some(field), "expected [x, y]");
      return default;
    };
    if values.len() != 2 {
      self.issue(key, Some(field), "expected exactly two numbers, [x, y]");
      return default;
    }
    let mut pair = [0i32; 2];
    for (index, value) in values.iter().enumerate() {
      let Some(number) = value.as_integer() else {
        self.issue(key, Some(field), "offset components must be whole numbers");
        return default;
      };
      let Ok(number) = i32::try_from(number) else {
        self.issue(key, Some(field), "offset component is out of range");
        return default;
      };
      pair[index] = number;
    }
    if pair.iter().any(|value| !(-2048..=4096).contains(value)) {
      self.issue(
        key,
        Some(field),
        "offset components must be between -2048 and 4096",
      );
      return default;
    }
    (pair[0], pair[1])
  }

  #[allow(clippy::too_many_arguments)]
  pub(super) fn placement(
    &mut self,
    anchor: Option<&Field>,
    offset: Option<&Field>,
    monitor: Option<&Field>,
    rotation: Option<&Field>,
    scope: &'static str,
    default: &Placement,
  ) -> Placement {
    Placement {
      // The key names are static so the issue type stays cheap. The scope is
      // already encoded in the caller's key name below.
      anchor: self.anchor(anchor, placement_key(scope, 0), default.anchor),
      offset: self.offset(offset, placement_key(scope, 1), default.offset),
      monitor: self.optional_text(
        monitor,
        placement_key(scope, 2),
        "expected an output name",
        64,
      ),
      rotation: self.rotation(rotation, placement_key(scope, 3), default.rotation),
    }
  }

  pub(super) fn login_layout(
    &mut self,
    field: Option<&Field>,
    default: crate::config::LoginLayout,
  ) -> crate::config::LoginLayout {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue("login.layout", Some(field), "expected a layout name");
      return default;
    };
    match value {
      "card" => crate::config::LoginLayout::Card,
      "hero" => crate::config::LoginLayout::Hero,
      // A poster is the full-bleed frame without a surface. It shares the hero
      // geometry (a fixed frame with `form_offset`) but the name describes the
      // cardless result an author actually wants.
      "poster" => crate::config::LoginLayout::Poster,
      _ => {
        self.issue(
          "login.layout",
          Some(field),
          format!("unknown layout {value:?}; use card, hero, or poster"),
        );
        default
      }
    }
  }

  /// Builds the Hyprland-style blur block. The old flat `blur_radius` and
  /// `blur_refresh_ms` keys are still accepted as aliases, because a config
  /// written for dawn 0.2 should not start blurring with a different radius
  /// after this change.
  pub(super) fn blur_config(
    &mut self,
    raw: &crate::config::raw::RawBlur,
    legacy_radius: Option<&Field>,
    legacy_refresh: Option<&Field>,
    default: &BlurConfig,
  ) -> BlurConfig {
    let enabled = self.boolean(raw.enabled.as_ref(), "theme.blur.enabled", default.enabled);
    let size = if let Some(field) = raw.size.as_ref() {
      self.range(
        Some(field),
        "theme.blur.size",
        f64::from(default.size),
        0.0,
        64.0,
      ) as f32
    } else {
      self.range(
        legacy_radius,
        "theme.blur_radius",
        f64::from(default.size),
        0.0,
        64.0,
      ) as f32
    };
    let refresh_ms = if let Some(field) = raw.refresh_ms.as_ref() {
      self.integer_range(
        Some(field),
        "theme.blur.refresh_ms",
        default.refresh.as_millis() as u64,
        50,
        5000,
      )
    } else {
      self.integer_range(
        legacy_refresh,
        "theme.blur_refresh_ms",
        default.refresh.as_millis() as u64,
        50,
        5000,
      )
    };
    // Validate the compositor-only switches and intentionally discard them.
    let _ = self.boolean(
      raw.ignore_opacity.as_ref(),
      "theme.blur.ignore_opacity",
      true,
    );
    let _ = self.boolean(
      raw.new_optimizations.as_ref(),
      "theme.blur.new_optimizations",
      true,
    );
    let _ = self.boolean(raw.xray.as_ref(), "theme.blur.xray", false);
    let _ = self.boolean(raw.special.as_ref(), "theme.blur.special", false);
    let _ = self.boolean(raw.popups.as_ref(), "theme.blur.popups", false);
    let _ = self.boolean(
      raw.popup_ignore_alpha.as_ref(),
      "theme.blur.popup_ignore_alpha",
      false,
    );
    BlurConfig {
      enabled,
      size,
      passes: self.integer_range(
        raw.passes.as_ref(),
        "theme.blur.passes",
        u64::from(default.passes),
        1,
        8,
      ) as u32,
      noise: self.range(
        raw.noise.as_ref(),
        "theme.blur.noise",
        f64::from(default.noise),
        0.0,
        1.0,
      ) as f32,
      contrast: self.range(
        raw.contrast.as_ref(),
        "theme.blur.contrast",
        f64::from(default.contrast),
        0.0,
        2.0,
      ) as f32,
      brightness: self.range(
        raw.brightness.as_ref(),
        "theme.blur.brightness",
        f64::from(default.brightness),
        0.0,
        2.0,
      ) as f32,
      vibrancy: self.range(
        raw.vibrancy.as_ref(),
        "theme.blur.vibrancy",
        f64::from(default.vibrancy),
        0.0,
        1.0,
      ) as f32,
      vibrancy_darkness: self.range(
        raw.vibrancy_darkness.as_ref(),
        "theme.blur.vibrancy_darkness",
        f64::from(default.vibrancy_darkness),
        0.0,
        1.0,
      ) as f32,
      refresh: Duration::from_millis(refresh_ms),
    }
  }

  pub(super) fn surface_style(
    &mut self,
    field: Option<&Field>,
    default: SurfaceStyle,
  ) -> SurfaceStyle {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.surface",
        Some(field),
        "expected glass, flat, or none",
      );
      return default;
    };
    match SurfaceStyle::parse(value) {
      Some(style) => style,
      None => {
        self.issue(
          "login.surface",
          Some(field),
          format!("unknown surface {value:?}; use glass, flat, or none"),
        );
        default
      }
    }
  }

  pub(super) fn hover_reveal(
    &mut self,
    field: Option<&Field>,
    default: HoverReveal,
  ) -> HoverReveal {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.button.hover_direction",
        Some(field),
        "expected fade, left, right, up, down, or center",
      );
      return default;
    };
    match HoverReveal::parse(value) {
      Some(reveal) => reveal,
      None => {
        self.issue(
          "login.button.hover_direction",
          Some(field),
          format!("unknown direction {value:?}; use fade, left, right, up, down, or center"),
        );
        default
      }
    }
  }

  pub(super) fn button_effect(
    &mut self,
    field: Option<&Field>,
    default: ButtonEffect,
  ) -> ButtonEffect {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.button.effect",
        Some(field),
        "expected none, sweep, sheen, pulse, glow, or corners",
      );
      return default;
    };
    match ButtonEffect::parse(value) {
      Some(effect) => effect,
      None => {
        self.issue(
          "login.button.effect",
          Some(field),
          format!("unknown effect {value:?}; use none, sweep, sheen, pulse, glow, or corners"),
        );
        default
      }
    }
  }

  pub(super) fn caret_style(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: CaretStyle,
  ) -> CaretStyle {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected bar or block");
      return default;
    };
    match CaretStyle::parse(value) {
      Some(style) => style,
      None => {
        self.issue(
          key,
          Some(field),
          format!("unknown caret style {value:?}; use bar or block"),
        );
        default
      }
    }
  }

  /// Parses the built-in widget draw layer. The two values are deliberately
  /// named after the paint order, not after a z-index: `behind` is the
  /// historical order and `front` puts the widget between decor and the form.
  pub(super) fn widget_layer(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: WidgetLayer,
  ) -> WidgetLayer {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected behind or front");
      return default;
    };
    match WidgetLayer::parse(value) {
      Some(layer) => layer,
      None => {
        self.issue(
          key,
          Some(field),
          format!("unknown widget layer {value:?}; use behind or front"),
        );
        default
      }
    }
  }

  /// Parses one widget plate table. Every key falls back independently, so a
  /// single typo recolours or reshapes one property instead of dropping the
  /// whole backplate. The defaults are the old scrim values.
  pub(super) fn plate(&mut self, raw: &RawWidgetPlate, prefix: &str) -> WidgetPlate {
    let default = WidgetPlate::default();
    let key = |name: &str| format!("{prefix}.{name}");
    let style = match raw.style.as_ref() {
      None => default.style,
      Some(field) => match field.get_ref().as_str() {
        Some(text) => PlateStyle::parse(text).unwrap_or_else(|| {
          self.issue(
            key("style"),
            Some(field),
            format!("unknown plate style {text:?}; use none, tint, solid, gradient, or glass"),
          );
          default.style
        }),
        None => {
          self.issue(key("style"), Some(field), "expected a plate style name");
          default.style
        }
      },
    };
    WidgetPlate {
      style,
      color: self.optional_color(raw.color.as_ref(), &key("color")),
      color_end: self.optional_color(raw.color_end.as_ref(), &key("color_end")),
      gradient_angle: self.range(
        raw.gradient_angle.as_ref(),
        &key("gradient_angle"),
        0.0,
        -360.0,
        360.0,
      ) as f32,
      radius: self.range(
        raw.radius.as_ref(),
        &key("radius"),
        f64::from(default.radius),
        0.0,
        64.0,
      ) as f32,
      border: self.optional_color(raw.border.as_ref(), &key("border")),
      padding: self.range(
        raw.padding.as_ref(),
        &key("padding"),
        f64::from(default.padding),
        0.0,
        64.0,
      ) as f32,
      glow: self.optional_color(raw.glow.as_ref(), &key("glow")),
      glow_size: self.range(raw.glow_size.as_ref(), &key("glow_size"), 0.0, 0.0, 48.0) as f32,
      shadow: self.boolean(raw.shadow.as_ref(), &key("shadow"), false),
      opacity: self.range(
        raw.opacity.as_ref(),
        &key("opacity"),
        f64::from(default.opacity),
        0.0,
        1.0,
      ) as f32,
    }
  }

  /// Parses `show_on` or `hide_on`.
  ///
  /// The lists are small and names are cheap, so duplicates are removed here
  /// rather than at draw time. Unknown names are reported once each and then
  /// skipped: one typo should not hide the whole widget. An empty list is a
  /// valid "always" for `show_on` and a valid "never hide" for `hide_on`.
  pub(super) fn widget_states(&mut self, field: Option<&Field>, key: &str) -> Vec<WidgetState> {
    let Some(field) = field else {
      return Vec::new();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(
        key,
        Some(field),
        "expected an array of state names such as [\"typing\", \"busy\"]",
      );
      return Vec::new();
    };
    if values.len() > 8 {
      self.issue(key, Some(field), "accepts at most 8 states");
    }
    let mut states = Vec::new();
    let mut reported = Vec::new();
    for value in values.iter().take(8) {
      let Some(text) = value.as_str() else {
        self.issue(key, Some(field), "every state must be a string");
        continue;
      };
      match WidgetState::parse(text) {
        Some(state) => {
          if !states.contains(&state) {
            states.push(state);
          }
        }
        None => {
          if !reported.contains(&text) {
            self.issue(
              key,
              Some(field),
              format!("unknown state {text:?}; use idle, typing, busy, error, or authenticated"),
            );
            reported.push(text);
          }
        }
      }
    }
    states
  }

  /// Parses the ordered `system.fields` list.
  ///
  /// We keep the author's order so a theme can put memory first. Unknown names
  /// and duplicates are reported once and skipped, which is the same degrade
  /// one field at a time rule the rest of the config uses. An empty list is
  /// allowed: the sparkline style still has something to draw, and the text
  /// rows simply disappear.
  pub(super) fn system_fields(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: &[SystemField],
  ) -> Vec<SystemField> {
    let Some(field) = field else {
      return default.to_vec();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(
        key,
        Some(field),
        "expected an array of field names such as [\"hostname\", \"kernel\"]",
      );
      return default.to_vec();
    };
    if values.len() > 6 {
      self.issue(key, Some(field), "accepts at most 6 fields");
    }
    let mut fields = Vec::new();
    let mut reported = Vec::new();
    for value in values.iter().take(6) {
      let Some(text) = value.as_str() else {
        self.issue(key, Some(field), "every field must be a string");
        continue;
      };
      match SystemField::parse(text) {
        Some(parsed) => {
          if !fields.contains(&parsed) {
            fields.push(parsed);
          }
        }
        None => {
          if !reported.contains(&text) {
            self.issue(
              key,
              Some(field),
              format!("unknown field {text:?}; use hostname, kernel, uptime, load, or memory"),
            );
            reported.push(text);
          }
        }
      }
    }
    fields
  }

  /// Resolves the whole presentation block for one built-in widget.
  pub(super) fn widget_look(
    &mut self,
    plate: &RawWidgetPlate,
    motion: Option<&crate::config::raw::RawDecorMotion>,
    show_on: Option<&Field>,
    hide_on: Option<&Field>,
    prefix: &str,
  ) -> WidgetLook {
    WidgetLook {
      plate: self.plate(plate, &format!("{prefix}.plate")),
      motion: self.decor_motion(motion, &format!("{prefix}.motion")),
      show_on: self.widget_states(show_on, &format!("{prefix}.show_on")),
      hide_on: self.widget_states(hide_on, &format!("{prefix}.hide_on")),
    }
  }
}
