//! Flip clock face.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_flip(
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
  plate: Option<&WidgetPlate>,
  theme: &Theme,
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

  let card_width = size * DIGIT_WIDTH + size * FLIP_CARD_PADDING;
  let gap = size * FACE_GAP;
  let radius = plate
    .map_or(1.0, |plate| (plate.radius * scale).max(1.0))
    .min(card_width * 0.25)
    .min(face_height * 0.25);
  let blink = colon_on(spec, elapsed);
  let mut cursor = x + (total_width - face_width) / 2.0;
  for character in time.chars() {
    let width = if character == ':' {
      card_width * 0.5
    } else {
      card_width
    };
    if character == ':' {
      if blink {
        let dot = (size * 0.075).max(1.0);
        let center_x = cursor + width / 2.0;
        canvas.fill_circle((center_x, y + face_height * 0.34), dot, color);
        canvas.fill_circle((center_x, y + face_height * 0.66), dot, color);
      }
    } else {
      draw_flip_card(
        canvas,
        text,
        Rect::new(cursor, y, width, face_height),
        character,
        color,
        plate,
        theme,
        radius,
      );
    }
    cursor += width + gap;
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

/// Paints one flip card. The plate supplies the material, but the card is
/// smaller than the widget plate, so a `Tint` plate becomes a denser glass card
/// instead of the usual scrim. A `None` plate draws no surface at all, which
/// lets a theme put the cards straight on the wallpaper.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_flip_card(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  rect: Rect,
  glyph: char,
  color: Rgb,
  plate: Option<&WidgetPlate>,
  theme: &Theme,
  radius: f32,
) {
  let style = plate.map_or(PlateStyle::Tint, |plate| plate.style);
  if style != PlateStyle::None {
    match style {
      PlateStyle::Tint => canvas.fill(rect, theme.tint.with_alpha(230), radius),
      PlateStyle::Solid => canvas.fill(
        rect,
        plate.and_then(|plate| plate.color).unwrap_or(theme.tint),
        radius,
      ),
      PlateStyle::Gradient => {
        let start = plate.and_then(|plate| plate.color).unwrap_or(theme.tint);
        let end = plate
          .and_then(|plate| plate.color_end)
          .unwrap_or(Rgb::rgba(0, 0, 0, 0));
        let angle = plate.map_or(0.0, |plate| plate.gradient_angle);
        canvas.linear_gradient_stops(rect, radius, angle, &[(0.0, start), (1.0, end)]);
      }
      // The widget plate already painted the shared backdrop underneath the
      // flip cards. Each card still needs its own denser surface to look like a
      // hinged flap, so Glass behaves like Tint here and honours a configured
      // colour when the author set one.
      PlateStyle::Glass => canvas.fill(
        rect,
        plate
          .and_then(|plate| plate.color)
          .unwrap_or(theme.tint.with_alpha(230)),
        radius,
      ),
      PlateStyle::None => {}
    }
    let border = plate
      .and_then(|plate| plate.border)
      .unwrap_or(theme.border.with_alpha(70));
    canvas.stroke(rect, border, radius, 1.0);
    // A faint highlight across the top half sells the hinged-flap look without
    // needing a second image or a real 3D transform.
    if rect.h > 4.0 {
      let highlight = Rect::new(
        rect.x + 1.0,
        rect.y + 1.0,
        (rect.w - 2.0).max(0.0),
        rect.h * 0.42,
      );
      canvas.fill(highlight, Rgb::rgba(255, 255, 255, 12), radius * 0.8);
    }
    canvas.line(
      (rect.x + 2.0, rect.center().1),
      (rect.right() - 2.0, rect.center().1),
      theme.border.with_alpha(90),
      1.0,
    );
  }

  let glyph_size = rect.h * 0.78;
  let label = glyph.to_string();
  let (_, glyph_height) = text.measure(&label, glyph_size, rect.w, false);
  text.draw(
    canvas,
    &label,
    Style::new(glyph_size, color),
    Area::new(rect.x, rect.y + (rect.h - glyph_height) / 2.0, rect.w).align(Align::Center),
  );
}
