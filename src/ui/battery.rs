//! The local battery indicator.
//!
//! All three styles share the same `bounds` and active-colour rule, so a theme
//! can switch from a ring to a pill without moving the plate or the hit region.
//! The widget never reads the filesystem itself: [`crate::sysinfo`] supplies a
//! snapshot through the view, which also makes the fake-data screenshot path
//! behave exactly like the live one.

use std::f32::consts::TAU;

use cosmic_text::Align;

use crate::color::Rgb;
use crate::config::{BatteryConfig, BatteryStyle};
use crate::layout::{Anchor, Placement};
use crate::sysinfo::{BatterySnapshot, format_percent};
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};

/// Logical content height for one style. The width always comes from config.
fn content_size(spec: &BatteryConfig, battery: &BatterySnapshot, scale: f32) -> (f32, f32) {
  let size = spec.size * scale;
  let width = spec.width * scale;
  let height = if !battery.present {
    size * 1.8
  } else {
    match spec.style {
      BatteryStyle::Ring => size * 4.4,
      BatteryStyle::Bar => size * 2.2,
      BatteryStyle::Pill => size * 2.4,
    }
  };
  (width, height)
}

fn place(
  spec: &BatteryConfig,
  battery: &BatterySnapshot,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
) -> Rect {
  let (width, height) = content_size(spec, battery, scale);
  let (x, y) = anchor.place((viewport.w, viewport.h), (width, height), offset);
  Rect::new(viewport.x + x, viewport.y + y, width, height)
}

/// The rectangle the battery widget occupies on screen.
///
/// The size depends on the style and on whether a battery is present, so the
/// renderer asks here instead of guessing from the config alone.
#[must_use]
pub fn bounds(
  spec: &BatteryConfig,
  battery: &BatterySnapshot,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
) -> Option<Rect> {
  Some(place(spec, battery, anchor, offset, viewport, scale))
}

/// The colour a charged or discharged reading should use.
///
/// Charging wins over low because a charging pack at 8% is not a warning: it is
/// recovering. Full packs keep the normal colour, which avoids a permanent
/// green flash on a desktop that is always plugged in.
fn active_color(spec: &BatteryConfig, battery: &BatterySnapshot) -> Rgb {
  if battery.charging {
    spec.charging_color
  } else if battery.percent <= spec.low_at {
    spec.low_color
  } else {
    spec.color
  }
}

/// A small four-stroke lightning bolt, drawn with plain lines so the widget has
/// no glyph or font dependency.
fn draw_bolt(canvas: &mut Canvas, center: (f32, f32), height: f32, color: Rgb, scale: f32) {
  let height = height.max(2.0 * scale);
  let width = height * 0.45;
  let stroke = (height * 0.12).max(1.0 * scale);
  let top = (center.0 - width * 0.22, center.1 - height / 2.0);
  let right = (center.0 + width * 0.42, center.1 - height * 0.05);
  let notch = (center.0 - width * 0.06, center.1 - height * 0.05);
  let bottom = (center.0 - width * 0.30, center.1 + height / 2.0);
  canvas.line(top, right, color, stroke);
  canvas.line(right, notch, color, stroke);
  canvas.line(notch, bottom, color, stroke);
}

fn draw_label(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  label: &str,
  size: f32,
  color: Rgb,
  area: Rect,
) {
  let (_, height) = text.measure(label, size, area.w.max(1.0), false);
  text.draw(
    canvas,
    label,
    Style::new(size, color),
    Area::new(area.x, area.y + (area.h - height) / 2.0, area.w.max(1.0)).align(Align::Center),
  );
}

/// Paints the battery widget at its configured anchor.
///
/// `!present` becomes a muted `NO BATTERY` label. The ring, bar, and pill styles
/// all use the same active colour: charging colour while charging, low colour at
/// or below `low_at`, and the normal colour otherwise.
pub fn draw(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &BatteryConfig,
  battery: &BatterySnapshot,
  placement: &Placement,
  viewport: Rect,
  scale: f32,
) {
  let rect = place(
    spec,
    battery,
    placement.anchor,
    placement.offset,
    viewport,
    scale,
  );
  let size = spec.size * scale;
  if !battery.present {
    let (_, height) = text.measure("NO BATTERY", size, rect.w, false);
    text.draw(
      canvas,
      "NO BATTERY",
      Style::new(size, spec.color.with_alpha(150)),
      Area::new(rect.x, rect.y + (rect.h - height) / 2.0, rect.w).align(Align::Center),
    );
    return;
  }

  let color = active_color(spec, battery);
  let percent = format_percent(battery.percent);
  let fraction = f32::from(battery.percent) / 100.0;

  match spec.style {
    BatteryStyle::Ring => {
      let center = rect.center();
      let thickness = (size * 0.18).max(1.5);
      let radius = (rect.h / 2.0 - thickness * 0.6).max(thickness);
      let inner = radius - thickness;
      let inactive = spec.color.with_alpha(38);
      for index in 0..60 {
        let angle = index as f32 * TAU / 60.0 - TAU / 4.0;
        let segment = (index as f32 + 0.5) / 60.0;
        let ink = if segment <= fraction { color } else { inactive };
        canvas.line(
          (
            center.0 + angle.cos() * inner,
            center.1 + angle.sin() * inner,
          ),
          (
            center.0 + angle.cos() * radius,
            center.1 + angle.sin() * radius,
          ),
          ink,
          thickness * 0.75,
        );
      }
      if spec.show_percent {
        draw_label(
          canvas,
          text,
          &percent,
          size,
          color,
          Rect::new(center.0 - radius, center.1 - size / 2.0, radius * 2.0, size),
        );
      }
      if battery.charging {
        draw_bolt(
          canvas,
          (center.0, center.1 - size * 0.95),
          size * 0.9,
          color,
          scale,
        );
      }
    }
    BatteryStyle::Bar => {
      let text_size = size * 0.9;
      let gap = size * 0.5;
      let (text_width, _) = if spec.show_percent {
        text.measure(&percent, text_size, rect.w, false)
      } else {
        (0.0, 0.0)
      };
      let bolt_width = if battery.charging { size * 0.9 } else { 0.0 };
      let trailing = if spec.show_percent {
        gap + text_width
      } else {
        0.0
      } + if battery.charging {
        gap * 0.6 + bolt_width
      } else {
        0.0
      };
      let bar_width = (rect.w - trailing).max(size * 2.0);
      let bar_height = (rect.h * 0.55).clamp(size * 0.6, size * 1.1);
      let bar = Rect::new(
        rect.x,
        rect.y + (rect.h - bar_height) / 2.0,
        bar_width,
        bar_height,
      );
      canvas.fill(bar, spec.color.with_alpha(34), bar_height / 2.0);
      let fill_width = bar.w * fraction;
      if fill_width > 0.0 {
        canvas.fill(
          Rect::new(bar.x, bar.y, fill_width.max(bar_height * 0.5), bar.h),
          color,
          bar_height / 2.0,
        );
      }
      canvas.stroke(bar, spec.color.with_alpha(80), bar_height / 2.0, 1.0);
      let mut x = bar.right() + gap;
      if battery.charging {
        draw_bolt(
          canvas,
          (x + bolt_width / 2.0, bar.center().1),
          size * 0.9,
          color,
          scale,
        );
        x += bolt_width + gap * 0.6;
      }
      if spec.show_percent {
        draw_label(
          canvas,
          text,
          &percent,
          text_size,
          color,
          Rect::new(x, bar.y, text_width.max(1.0), bar.h),
        );
      }
    }
    BatteryStyle::Pill => {
      let text_size = size * 0.9;
      let gap = size * 0.6;
      let (text_width, _) = if spec.show_percent {
        text.measure(&percent, text_size, rect.w, false)
      } else {
        (0.0, 0.0)
      };
      let bolt_width = if battery.charging { size * 0.9 } else { 0.0 };
      let nub_width = size * 0.4;
      let trailing = nub_width
        + gap
        + if spec.show_percent {
          text_width + gap
        } else {
          0.0
        }
        + if battery.charging {
          bolt_width + gap * 0.6
        } else {
          0.0
        };
      let body_width = (rect.w - trailing).max(size * 2.2);
      let body_height = (rect.h * 0.62).clamp(size * 1.1, size * 1.6);
      let body = Rect::new(
        rect.x,
        rect.y + (rect.h - body_height) / 2.0,
        body_width,
        body_height,
      );
      canvas.fill(body, spec.color.with_alpha(30), body_height * 0.18);
      let inner = body.inset((2.0 * scale).max(1.0));
      let fill_width = inner.w * fraction;
      if fill_width > 0.0 {
        canvas.fill(
          Rect::new(inner.x, inner.y, fill_width, inner.h),
          color,
          (body_height * 0.12).min(inner.h / 2.0),
        );
      }
      canvas.stroke(body, spec.color.with_alpha(90), body_height * 0.18, 1.0);
      let nub = Rect::new(
        body.right() + 1.0,
        body.center().1 - body_height * 0.2,
        nub_width,
        body_height * 0.4,
      );
      canvas.fill(nub, spec.color.with_alpha(90), nub_width * 0.25);
      let mut x = nub.right() + gap;
      if battery.charging {
        draw_bolt(
          canvas,
          (x + bolt_width / 2.0, body.center().1),
          size * 0.9,
          color,
          scale,
        );
        x += bolt_width + gap * 0.6;
      }
      if spec.show_percent {
        draw_label(
          canvas,
          text,
          &percent,
          text_size,
          color,
          Rect::new(x, body.y, text_width.max(1.0), body.h),
        );
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{active_color, bounds};
  use crate::config::{BatteryConfig, BatteryStyle};
  use crate::layout::Anchor;
  use crate::sysinfo::BatterySnapshot;
  use crate::ui::canvas::Rect;

  fn spec() -> BatteryConfig {
    crate::config::Config::default().battery
  }

  #[test]
  fn charging_and_low_colours_are_resolved_in_the_right_order() {
    let config = spec();
    let low = BatterySnapshot {
      present: true,
      percent: 10,
      ..BatterySnapshot::default()
    };
    assert_eq!(active_color(&config, &low), config.low_color);
    let charging = BatterySnapshot {
      charging: true,
      ..low.clone()
    };
    assert_eq!(active_color(&config, &charging), config.charging_color);
    let normal = BatterySnapshot {
      present: true,
      percent: 80,
      ..BatterySnapshot::default()
    };
    assert_eq!(active_color(&config, &normal), config.color);
  }

  #[test]
  fn each_style_has_a_stable_bounds_height() {
    let battery = BatterySnapshot {
      present: true,
      percent: 50,
      ..BatterySnapshot::default()
    };
    let viewport = Rect::new(0.0, 0.0, 1280.0, 720.0);
    for style in [BatteryStyle::Ring, BatteryStyle::Bar, BatteryStyle::Pill] {
      let config = BatteryConfig { style, ..spec() };
      let rect = bounds(&config, &battery, Anchor::TopLeft, (0, 0), viewport, 1.0).unwrap();
      assert_eq!(rect.w, config.width);
      assert!(rect.h > 0.0);
    }
  }

  #[test]
  fn a_missing_battery_uses_the_short_label_height() {
    let battery = BatterySnapshot::default();
    let config = spec();
    let rect = bounds(
      &config,
      &battery,
      Anchor::TopLeft,
      (0, 0),
      Rect::new(0.0, 0.0, 1280.0, 720.0),
      1.0,
    )
    .unwrap();
    assert_eq!(rect.h, config.size * 1.8);
  }
}
