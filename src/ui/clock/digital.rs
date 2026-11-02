//! Digital clock face.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_digital(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
  time: &str,
  date: &str,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  size: f32,
  date_size: f32,
  color: Rgb,
  date_color: Rgb,
) {
  if time.is_empty() {
    return;
  }
  let screen = (viewport.w, viewport.h);
  let (width, time_height) = text.measure(time, size, screen.0 - 40.0, false);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    text.measure(date, date_size, screen.0 - 40.0, false)
  };
  let (label_width, label_height) = label_measure(text, spec, size, screen.0 - 40.0);
  // Both lines share the widget's full width. Using the time width for the
  // date was a real bug: a long weekday name wrapped to two lines under a
  // short time, which looked like the clock had fallen apart.
  let total_width = width.max(date_width).max(label_width);
  let total = (
    total_width,
    label_height + time_height + date_height + date_size * 0.15,
  );
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);

  if !spec.label.is_empty() {
    let label_size = if spec.label_size > 0.0 {
      spec.label_size
    } else {
      size * 0.22
    };
    let label_color = spec.label_color.unwrap_or(color);
    text.draw(
      canvas,
      &spec.label,
      Style::new(label_size, label_color).tracking(spec.label_tracking),
      Area::new(x, y, total_width.max(1.0)).align(Align::Center),
    );
  }
  text.draw(
    canvas,
    time,
    Style::new(size, color),
    Area::new(x, y + label_height, total_width.max(1.0)).align(Align::Center),
  );
  if !date.is_empty() {
    text.draw(
      canvas,
      date,
      Style::new(date_size, date_color),
      Area::new(
        x,
        y + label_height + time_height + date_size * 0.15,
        total_width.max(1.0),
      )
      .align(Align::Center),
    );
  }
}

/// Whether the colon should be lit at `elapsed` seconds.
///
/// `colon_blink_ms` is a full on/off period: the first half is lit and the
/// second half is dark. Zero (the default) means "steady", so an unconfigured
/// clock never blinks.
pub(super) fn colon_on(spec: &ClockSpec, elapsed: f32) -> bool {
  if spec.colon_blink_ms <= 0.0 {
    return true;
  }
  let phase = (elapsed * 1000.0 / spec.colon_blink_ms).rem_euclid(2.0);
  phase < 1.0
}

/// Draws the date line under a text face. Flip and segment share it so their
/// date placement cannot drift from each other.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_date(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  date: &str,
  x: f32,
  y: f32,
  width: f32,
  date_size: f32,
  date_color: Rgb,
) {
  if date.is_empty() {
    return;
  }
  text.draw(
    canvas,
    date,
    Style::new(date_size, date_color),
    Area::new(x, y, width.max(1.0)).align(Align::Center),
  );
}
