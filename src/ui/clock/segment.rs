//! Segment clock face.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_segment(
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
  elapsed: f32,
  scale: f32,
) {
  if time.is_empty() {
    return;
  }
  let screen = (viewport.w, viewport.h);
  let (face_width, face_height) = face_size(text, spec, time, size);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    text.measure(date, date_size, screen.0 - 40.0, false)
  };
  let total_width = face_width.max(date_width);
  let total = (total_width, face_height + date_height + date_size * 0.15);
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);

  let digit_width = size * DIGIT_WIDTH;
  let gap = size * FACE_GAP;
  let blink = colon_on(spec, elapsed);
  let mut cursor = x + (total_width - face_width) / 2.0;
  for character in time.chars() {
    if character == ':' {
      if blink {
        let dot = (size * 0.07).max(1.0);
        let center_x = cursor + digit_width * 0.25;
        canvas.fill_circle((center_x, y + face_height * 0.34), dot, color);
        canvas.fill_circle((center_x, y + face_height * 0.66), dot, color);
      }
      cursor += digit_width * 0.5 + gap;
      continue;
    }
    let rect = Rect::new(cursor, y, digit_width, face_height);
    match segment_mask(character) {
      Some(mask) => draw_segment_digit(canvas, rect, mask, spec, color, scale),
      // A format can include AM/PM or a literal letter. Those have no segment
      // geometry, so print them as ordinary text instead of dropping them.
      None => draw_segment_letter(canvas, text, rect, character, color),
    }
    cursor += digit_width + gap;
  }

  draw_date(
    canvas,
    text,
    date,
    x,
    y + face_height + date_size * 0.15,
    total_width,
    date_size,
    date_color,
  );
}

pub(super) fn draw_segment_letter(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  rect: Rect,
  character: char,
  color: Rgb,
) {
  let glyph_size = rect.h * 0.8;
  let label = character.to_string();
  let (_, glyph_height) = text.measure(&label, glyph_size, rect.w, false);
  text.draw(
    canvas,
    &label,
    Style::new(glyph_size, color),
    Area::new(rect.x, rect.y + (rect.h - glyph_height) / 2.0, rect.w).align(Align::Center),
  );
}

pub(super) fn draw_segment_digit(
  canvas: &mut Canvas,
  rect: Rect,
  mask: u8,
  spec: &ClockSpec,
  color: Rgb,
  scale: f32,
) {
  let thickness = (rect.h * 0.12).max(1.5);
  if spec.ghost {
    for segment in 0..7 {
      if mask & (1 << segment) == 0 {
        draw_segment_line(canvas, rect, segment, color.with_alpha(30), thickness);
      }
    }
  }
  if let Some(glow) = spec.glow
    && spec.glow_size > 0.0
  {
    let glow_width = thickness + spec.glow_size * scale;
    for segment in 0..7 {
      if mask & (1 << segment) != 0 {
        draw_segment_line(canvas, rect, segment, glow.with_alpha(60), glow_width);
      }
    }
  }
  for segment in 0..7 {
    if mask & (1 << segment) != 0 {
      draw_segment_line(canvas, rect, segment, color, thickness);
    }
  }
}

/// One of the seven strokes. The small gaps at the joints keep adjacent
/// segments from merging into a blob at larger sizes, which is exactly how a
/// real seven-segment display looks.
pub(super) fn draw_segment_line(
  canvas: &mut Canvas,
  rect: Rect,
  segment: usize,
  color: Rgb,
  width: f32,
) {
  let thickness = (rect.h * 0.12).max(1.0);
  let inset = thickness * 0.6;
  let x0 = rect.x + inset;
  let x1 = rect.right() - inset;
  let y0 = rect.y + thickness * 0.5;
  let middle = rect.y + rect.h * 0.5;
  let y1 = rect.bottom() - thickness * 0.5;
  let gap = thickness * 0.35;
  let (from, to) = match segment {
    0 => ((x0, y0), (x1, y0)),
    1 => ((x1, y0 + gap), (x1, middle - gap)),
    2 => ((x1, middle + gap), (x1, y1 - gap)),
    3 => ((x0, y1), (x1, y1)),
    4 => ((x0, middle + gap), (x0, y1 - gap)),
    5 => ((x0, y0 + gap), (x0, middle - gap)),
    _ => ((x0, middle), (x1, middle)),
  };
  canvas.line(from, to, color, width);
}

/// Active-segment bitmask for the digits 0 to 9. Bit 0 is A (top), then B
/// (upper right), C (lower right), D (bottom), E (lower left), F (upper left),
/// and bit 6 is G (middle). Anything else has no seven-segment form.
pub(super) fn segment_mask(character: char) -> Option<u8> {
  const A: u8 = 1 << 0;
  const B: u8 = 1 << 1;
  const C: u8 = 1 << 2;
  const D: u8 = 1 << 3;
  const E: u8 = 1 << 4;
  const F: u8 = 1 << 5;
  const G: u8 = 1 << 6;
  match character {
    '0' => Some(A | B | C | D | E | F),
    '1' => Some(B | C),
    '2' => Some(A | B | G | E | D),
    '3' => Some(A | B | G | C | D),
    '4' => Some(F | G | B | C),
    '5' => Some(A | F | G | C | D),
    '6' => Some(A | F | G | E | C | D),
    '7' => Some(A | B | C),
    '8' => Some(A | B | C | D | E | F | G),
    '9' => Some(A | B | C | D | F | G),
    _ => None,
  }
}
