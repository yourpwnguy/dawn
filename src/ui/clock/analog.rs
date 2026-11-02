//! Analog clock face.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_analog(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
  date: &str,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  diameter: f32,
  date_size: f32,
  color: Rgb,
  date_color: Rgb,
  now: &Zoned,
) {
  let local = spec.local(now);
  let screen = (viewport.w, viewport.h);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    let (width, height) = text.measure(date, date_size, screen.0, false);
    (width, height + date_size * 0.2)
  };
  // The date can be wider than the dial. Sizing the widget to the wider of the
  // two keeps the date on one line instead of wrapping it into a narrow column
  // under a small clock.
  let total_width = diameter.max(date_width);
  let total = (total_width, diameter + date_height);
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);
  let radius = diameter / 2.0;
  let center = (x + total_width / 2.0, y + radius);
  let rim = color.with_alpha(70);
  let face = color.with_alpha(30);
  let tick = color.with_alpha(120);

  canvas.fill_circle(center, radius, face);
  canvas.stroke_circle(center, radius, rim, (diameter * 0.008).max(1.0));

  for index in 0..60u32 {
    let angle = index as f32 * TAU / 60.0 - TAU / 4.0;
    let (inner, width) = if index % 5 == 0 {
      (radius * 0.82, (diameter * 0.012).max(1.5))
    } else {
      (radius * 0.90, (diameter * 0.005).max(1.0))
    };
    let outer = radius * 0.96;
    canvas.line(
      (
        center.0 + angle.cos() * inner,
        center.1 + angle.sin() * inner,
      ),
      (
        center.0 + angle.cos() * outer,
        center.1 + angle.sin() * outer,
      ),
      tick,
      width,
    );
  }

  let hour = f32::from(local.hour() % 12) + f32::from(local.minute()) / 60.0;
  let minute = f32::from(local.minute()) + f32::from(local.second()) / 60.0;
  // A ticking hand jumps to whole seconds; a sweeping hand follows the
  // subsecond clock so it glides around the dial. The minute and hour hands are
  // always smooth, which is what makes a sweep second hand look correct.
  let second = match spec.second_hand {
    SecondHand::Tick => f32::from(local.second()),
    SecondHand::Sweep => {
      f32::from(local.second()) + local.subsec_nanosecond() as f32 / 1_000_000_000.0
    }
  };
  draw_hand(
    canvas,
    center,
    hour * TAU / 12.0 - TAU / 4.0,
    radius * 0.50,
    color,
    (diameter * 0.022).max(2.0),
  );
  draw_hand(
    canvas,
    center,
    minute * TAU / 60.0 - TAU / 4.0,
    radius * 0.72,
    color,
    (diameter * 0.014).max(1.5),
  );
  if spec.seconds {
    draw_hand(
      canvas,
      center,
      second * TAU / 60.0 - TAU / 4.0,
      radius * 0.82,
      color.with_alpha(200),
      (diameter * 0.006).max(1.0),
    );
  }
  canvas.fill_circle(center, (diameter * 0.018).max(2.0), color);

  if !date.is_empty() {
    text.draw(
      canvas,
      date,
      Style::new(date_size, date_color),
      Area::new(x, y + diameter + date_size * 0.2, total_width).align(Align::Center),
    );
  }
}

pub(super) fn draw_hand(
  canvas: &mut Canvas,
  center: (f32, f32),
  angle: f32,
  length: f32,
  color: Rgb,
  width: f32,
) {
  canvas.line(
    center,
    (
      center.0 + angle.cos() * length,
      center.1 + angle.sin() * length,
    ),
    color,
    width,
  );
}
