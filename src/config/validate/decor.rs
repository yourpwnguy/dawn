//! Decor validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn decor(
    &mut self,
    raw: &[crate::config::raw::RawDecor],
    base: Option<&Path>,
  ) -> Vec<crate::config::DecorSpec> {
    const MAX_DECOR: usize = 32;
    if raw.len() > MAX_DECOR {
      self.issue(
        "login.decor",
        None,
        format!("at most {MAX_DECOR} decor items are supported"),
      );
    }
    let mut items = Vec::new();
    for (index, item) in raw.iter().take(MAX_DECOR).enumerate() {
      let prefix = format!("login.decor[{index}]");
      let kind = self
        .string_value(
          item.kind.as_ref(),
          &format!("{prefix}.kind"),
          "expected text, image, or panel",
          16,
        )
        .unwrap_or_default();
      match kind.as_str() {
        "text" => {
          let text = self
            .string_value(
              item.text.as_ref(),
              &format!("{prefix}.text"),
              "expected decor text",
              256,
            )
            .unwrap_or_default();
          if text.is_empty() {
            continue;
          }
          let stroke = self.optional_color(item.stroke.as_ref(), &format!("{prefix}.stroke"));
          let stroke_width = self.integer_range(
            item.stroke_width.as_ref(),
            &format!("{prefix}.stroke_width"),
            0,
            0,
            12,
          ) as f32;
          // A stroke colour without a width would look like a no-op in the
          // config. Give it a visible two-pixel default; `stroke_width` can
          // still raise it up to 12.
          let stroke_width = if stroke.is_some() && stroke_width <= 0.0 {
            2.0
          } else {
            stroke_width
          };
          items.push(crate::config::DecorSpec::Text {
            text,
            size: self.range(
              item.size.as_ref(),
              &format!("{prefix}.size"),
              14.0,
              6.0,
              200.0,
            ) as f32,
            color: self.optional_color(item.color.as_ref(), &format!("{prefix}.color")),
            bold: self.boolean(item.bold.as_ref(), &format!("{prefix}.bold"), false),
            tracking: self.range(
              item.tracking.as_ref(),
              &format!("{prefix}.tracking"),
              0.0,
              0.0,
              0.5,
            ) as f32,
            stroke,
            stroke_width,
            anchor: self.anchor(
              item.anchor.as_ref(),
              &format!("{prefix}.anchor"),
              Anchor::TopLeft,
            ),
            offset: self.offset(item.offset.as_ref(), &format!("{prefix}.offset"), (0, 0)),
            rotation: self.rotation(item.rotation.as_ref(), &format!("{prefix}.rotation"), 0.0),
            motion: self.decor_motion(item.motion.as_ref(), &format!("{prefix}.motion")),
            pulse_on_beat: self.boolean(
              item.pulse_on_beat.as_ref(),
              &format!("{prefix}.pulse_on_beat"),
              false,
            ),
          });
        }
        "image" => {
          let Some(path) = self.image(item.image.as_ref(), &format!("{prefix}.image"), base) else {
            continue;
          };
          items.push(crate::config::DecorSpec::Image {
            path,
            width: self.range(
              item.width.as_ref(),
              &format!("{prefix}.width"),
              64.0,
              4.0,
              2000.0,
            ) as f32,
            anchor: self.anchor(
              item.anchor.as_ref(),
              &format!("{prefix}.anchor"),
              Anchor::TopLeft,
            ),
            offset: self.offset(item.offset.as_ref(), &format!("{prefix}.offset"), (0, 0)),
            opacity: self.range(
              item.opacity.as_ref(),
              &format!("{prefix}.opacity"),
              1.0,
              0.0,
              1.0,
            ) as f32,
            rotation: self.rotation(item.rotation.as_ref(), &format!("{prefix}.rotation"), 0.0),
            motion: self.decor_motion(item.motion.as_ref(), &format!("{prefix}.motion")),
            pulse_on_beat: self.boolean(
              item.pulse_on_beat.as_ref(),
              &format!("{prefix}.pulse_on_beat"),
              false,
            ),
          });
        }
        "panel" => {
          // A panel is a plate, not an image or a text run. Accept either key
          // so an item can be switched from one kind to another without the
          // parser rejecting it, but tell the author exactly once that the
          // payload is being ignored and draw a bare plate.
          if item.text.is_some() || item.image.is_some() {
            self.issue(
              format!("{prefix}.kind"),
              item.kind.as_ref(),
              "panel ignores text and image; drawing a bare panel",
            );
          }
          items.push(crate::config::DecorSpec::Panel {
            width: self.panel_size(item.width.as_ref(), &format!("{prefix}.width"), 320),
            height: self.panel_size(item.height.as_ref(), &format!("{prefix}.height"), 120),
            color: self.optional_color(item.color.as_ref(), &format!("{prefix}.color")),
            color_end: self.optional_color(item.color_end.as_ref(), &format!("{prefix}.color_end")),
            gradient_angle: self.range(
              item.gradient_angle.as_ref(),
              &format!("{prefix}.gradient_angle"),
              0.0,
              -360.0,
              360.0,
            ) as f32,
            radius: self.range(
              item.radius.as_ref(),
              &format!("{prefix}.radius"),
              0.0,
              0.0,
              64.0,
            ) as f32,
            border: self.optional_color(item.border.as_ref(), &format!("{prefix}.border")),
            glass: self.boolean(item.glass.as_ref(), &format!("{prefix}.glass"), false),
            opacity: self.range(
              item.opacity.as_ref(),
              &format!("{prefix}.opacity"),
              1.0,
              0.0,
              1.0,
            ) as f32,
            anchor: self.anchor(
              item.anchor.as_ref(),
              &format!("{prefix}.anchor"),
              Anchor::TopLeft,
            ),
            offset: self.offset(item.offset.as_ref(), &format!("{prefix}.offset"), (0, 0)),
            rotation: self.rotation(item.rotation.as_ref(), &format!("{prefix}.rotation"), 0.0),
            motion: self.decor_motion(item.motion.as_ref(), &format!("{prefix}.motion")),
            pulse_on_beat: self.boolean(
              item.pulse_on_beat.as_ref(),
              &format!("{prefix}.pulse_on_beat"),
              false,
            ),
          });
        }
        _ => {
          self.issue(
            format!("{prefix}.kind"),
            item.kind.as_ref(),
            "expected text, image, or panel",
          );
        }
      }
    }
    items
  }

  /// Parses one decor item's optional motion table. Periods have usable
  /// defaults even when the config only sets an amplitude, so a sparse table
  /// like `float = [8, 4]` still works.
  pub(super) fn decor_motion(
    &mut self,
    raw: Option<&crate::config::raw::RawDecorMotion>,
    prefix: &str,
  ) -> DecorMotion {
    let default = DecorMotion::default();
    let Some(raw) = raw else {
      return default;
    };
    let key = |name: &str| format!("{prefix}.{name}");
    let motion = DecorMotion {
      spin: self.range(
        raw.spin.as_ref(),
        &key("spin"),
        f64::from(default.spin),
        -720.0,
        720.0,
      ) as f32,
      sway: self.range(
        raw.sway.as_ref(),
        &key("sway"),
        f64::from(default.sway),
        0.0,
        45.0,
      ) as f32,
      sway_ms: self.range(
        raw.sway_ms.as_ref(),
        &key("sway_ms"),
        f64::from(default.sway_ms),
        200.0,
        20000.0,
      ) as f32,
      float: self.offset(raw.float.as_ref(), &key("float"), default.float),
      float_ms: self.range(
        raw.float_ms.as_ref(),
        &key("float_ms"),
        f64::from(default.float_ms),
        200.0,
        30000.0,
      ) as f32,
      pulse: self.range(
        raw.pulse.as_ref(),
        &key("pulse"),
        f64::from(default.pulse),
        0.0,
        0.5,
      ) as f32,
      pulse_ms: self.range(
        raw.pulse_ms.as_ref(),
        &key("pulse_ms"),
        f64::from(default.pulse_ms),
        200.0,
        20000.0,
      ) as f32,
      fade: self.range(
        raw.fade.as_ref(),
        &key("fade"),
        f64::from(default.fade),
        0.0,
        1.0,
      ) as f32,
      fade_ms: self.range(
        raw.fade_ms.as_ref(),
        &key("fade_ms"),
        f64::from(default.fade_ms),
        200.0,
        20000.0,
      ) as f32,
      phase: self.range(
        raw.phase.as_ref(),
        &key("phase"),
        f64::from(default.phase),
        0.0,
        1.0,
      ) as f32,
      typer_ms: self.range(
        raw.typer_ms.as_ref(),
        &key("typer_ms"),
        f64::from(default.typer_ms),
        0.0,
        20000.0,
      ) as f32,
      blink_ms: self.range(
        raw.blink_ms.as_ref(),
        &key("blink_ms"),
        f64::from(default.blink_ms),
        0.0,
        20000.0,
      ) as f32,
      ticker_ms: self.range(
        raw.ticker_ms.as_ref(),
        &key("ticker_ms"),
        f64::from(default.ticker_ms),
        0.0,
        120000.0,
      ) as f32,
      ticker_width: self.integer_range(
        raw.ticker_width.as_ref(),
        &key("ticker_width"),
        default.ticker_width as u64,
        0,
        4000,
      ) as i32,
      ticker_gap: self.integer_range(
        raw.ticker_gap.as_ref(),
        &key("ticker_gap"),
        default.ticker_gap as u64,
        0,
        2000,
      ) as i32,
      parallax_depth: self.range(
        raw.parallax_depth.as_ref(),
        &key("parallax_depth"),
        f64::from(default.parallax_depth),
        0.0,
        1.0,
      ) as f32,
      enter: self.choice(
        raw.enter.as_ref(),
        &key("enter"),
        default.enter,
        EnterStyle::parse,
        "none, fade, slide, or scale",
      ),
      enter_ms: self.range(
        raw.enter_ms.as_ref(),
        &key("enter_ms"),
        f64::from(default.enter_ms),
        0.0,
        20000.0,
      ) as f32,
      enter_delay_ms: self.range(
        raw.enter_delay_ms.as_ref(),
        &key("enter_delay_ms"),
        f64::from(default.enter_delay_ms),
        0.0,
        20000.0,
      ) as f32,
      enter_from: self.choice(
        raw.enter_from.as_ref(),
        &key("enter_from"),
        default.enter_from,
        EnterFrom::parse,
        "left, right, top, or bottom",
      ),
      wave: self.range(
        raw.wave.as_ref(),
        &key("wave"),
        f64::from(default.wave),
        0.0,
        40.0,
      ) as f32,
      wave_ms: self.range(
        raw.wave_ms.as_ref(),
        &key("wave_ms"),
        f64::from(default.wave_ms),
        200.0,
        20000.0,
      ) as f32,
      wave_step: self.range(
        raw.wave_step.as_ref(),
        &key("wave_step"),
        f64::from(default.wave_step),
        0.0,
        1.0,
      ) as f32,
      scramble_ms: self.range(
        raw.scramble_ms.as_ref(),
        &key("scramble_ms"),
        f64::from(default.scramble_ms),
        0.0,
        20000.0,
      ) as f32,
      glitch: self.range(
        raw.glitch.as_ref(),
        &key("glitch"),
        f64::from(default.glitch),
        0.0,
        1.0,
      ) as f32,
      glitch_ms: self.range(
        raw.glitch_ms.as_ref(),
        &key("glitch_ms"),
        f64::from(default.glitch_ms),
        200.0,
        20000.0,
      ) as f32,
    };
    // A scrolling ticker needs a window. Without one the item would silently
    // paint at its text width, which looks like a config typo rather than a
    // deliberate marquee, so tell the author exactly which key is missing.
    if motion.ticker_ms > 0.0 && motion.ticker_width <= 0 {
      self.issue(
        key("ticker_width"),
        raw.ticker_width.as_ref(),
        "ticker_ms needs ticker_width greater than 0",
      );
    }
    motion
  }
}
