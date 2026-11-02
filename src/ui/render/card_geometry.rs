//! Card Geometry pass for the renderer.

use super::*;

/// Layout constants derived from the configured font size. Keeping them in one
/// place means a larger font grows the card instead of overlapping the fields.
pub(super) struct Metrics {
  pub(super) base: f32,
  pub(super) pad: f32,
  pub(super) gap: f32,
  pub(super) inner_w: f32,
  pub(super) card_w: f32,
  pub(super) radius: f32,
}

impl Metrics {
  pub(super) fn new(theme: &Theme, screen: (f32, f32), scale: f32) -> Self {
    let login = &theme.login;
    let base = theme.font_size.max(10.0) * scale;
    let min_card = 240.0 * scale;
    let card_w = (login.width * scale).clamp(min_card, (screen.0 - base * 2.0).max(min_card));
    let pad = (login.padding * scale).max(base * 0.6);
    Self {
      base,
      pad,
      gap: login.gap * scale,
      inner_w: (card_w - pad * 2.0).max(1.0),
      card_w,
      radius: login.radius * scale,
    }
  }
}

/// Draws a widget into a bounded transparent layer and composites it with
/// rotation, scale, and opacity. The layer is only as large as the rotated
/// widget, so an animated ornament costs a small pixmap, not a second
/// full-screen canvas.
pub(super) fn paint_transformed(
  canvas: &mut Canvas,
  bounds: Rect,
  center: (f32, f32),
  angle: f32,
  scale: f32,
  opacity: f32,
  draw: impl FnOnce(&mut Canvas, f32),
) {
  if opacity <= 0.0 {
    return;
  }
  let scale = scale.clamp(0.01, 8.0);
  if angle.abs() <= 0.01 && (scale - 1.0).abs() <= 0.001 && (opacity - 1.0).abs() <= 0.001 {
    draw(canvas, opacity);
    return;
  }
  let region = rotated_bounds(bounds, angle).inset(-8.0);
  let width = region.w.ceil().max(1.0) as u32;
  let height = region.h.ceil().max(1.0) as u32;
  let Some(mut layer) = Canvas::region(width, height, (region.x, region.y))
    .map(|layer| layer.with_screen(canvas.screen()))
  else {
    draw(canvas, opacity);
    return;
  };
  draw(&mut layer, 1.0);
  canvas.draw_layer_scaled(&layer, center, angle, scale, opacity);
}

/// Rotation-only wrapper used by cards, fields, buttons, and widgets.
pub(super) fn paint_rotated(
  canvas: &mut Canvas,
  bounds: Rect,
  center: (f32, f32),
  angle: f32,
  draw: impl FnOnce(&mut Canvas),
) {
  paint_transformed(canvas, bounds, center, angle, 1.0, 1.0, |layer, _| {
    draw(layer);
  });
}

/// The font size for one field. A configured size of zero inherits the global
/// `ui.font_size`, which keeps the common config to just a placeholder string.
pub(super) fn field_size(theme: &Theme, field: &crate::config::FieldConfig, scale: f32) -> f32 {
  if field.font_size > 0.0 {
    field.font_size * scale
  } else {
    theme.font_size.max(10.0) * scale
  }
}

/// The height of one field, including its padding. Zero means "derive it".
pub(super) fn field_height(theme: &Theme, field: &crate::config::FieldConfig, scale: f32) -> f32 {
  if field.height > 0.0 {
    field.height * scale
  } else {
    field_size(theme, field, scale) * 2.5
  }
}

/// Height of an optional label above a field.
pub(super) fn label_height(
  text: &mut TextEngine,
  label: &str,
  size: f32,
  width: f32,
  tracking: f32,
) -> f32 {
  if label.is_empty() {
    0.0
  } else {
    text
      .measure_with(label, size, width, false, Weight::NORMAL, tracking)
      .1
  }
}

/// Size of a field's label. Zero keeps the historical 80 percent of the field
/// size; a theme that wants a micro label sets an absolute value instead.
pub(super) fn label_size(theme: &Theme, field: &crate::config::FieldConfig, scale: f32) -> f32 {
  if field.label_size > 0.0 {
    field.label_size * scale
  } else {
    field_size(theme, field, scale) * 0.8
  }
}

/// Computes the card height before drawing, so the card can be positioned and
/// its background painted before any text lands on it. Every step here has a
/// matching step in `draw_card`; if you change one, change both.
pub(super) fn card_height(
  text: &mut TextEngine,
  view: &View,
  theme: &Theme,
  m: &Metrics,
  scale: f32,
) -> f32 {
  let login = &theme.login;
  if login.layout.is_full_bleed() && login.height > 0.0 {
    return login.height * scale;
  }
  let mut height = m.pad * 2.0;
  if !view.greeting.is_empty() {
    let (_, h) = text.measure(&view.greeting, m.base * 1.25, m.inner_w, true);
    height += h + m.gap;
  }
  height += field_block_height(text, theme, &login.username, m, scale);
  if let Some(prompt) = &view.prompt {
    let (_, h) = text.measure(prompt, m.base * 0.95, m.inner_w, true);
    height += m.gap * 0.7 + h;
  }
  height += m.gap + field_block_height(text, theme, &login.password, m, scale);
  height += m.gap;
  if let Some(status) = &view.status {
    let (_, h) = text.measure(&status.text, m.base * 0.95, m.inner_w, true);
    height += h.max(m.base) + m.gap;
  } else {
    height += m.gap * 0.15;
  }
  if login.session.show {
    height += m.gap * 0.5;
    height += m.base * 1.2;
    height += m.gap * 0.55;
  }
  let hint = hint_text(view, login);
  if !hint.is_empty() {
    let (_, hint_height) = text.measure(&hint, m.base * 0.82, m.inner_w, false);
    height += hint_height;
  }
  height
}

/// A label plus a field, which is what both credentials contribute.
pub(super) fn field_block_height(
  text: &mut TextEngine,
  theme: &Theme,
  field: &crate::config::FieldConfig,
  m: &Metrics,
  scale: f32,
) -> f32 {
  let label = label_height(
    text,
    &field.label,
    label_size(theme, field, scale),
    m.inner_w,
    field.label_tracking,
  );
  let label_gap = if label > 0.0 { m.gap * 0.35 } else { 0.0 };
  label + label_gap + field_height(theme, field, scale)
}

/// Card geometry and colours shared by the chrome and form passes. Keeping the
/// two passes separate is what lets a front widget paint between the decor and
/// the form without rotating with the card.
pub(super) struct CardPaint<'a> {
  pub(super) card: Rect,
  pub(super) card_scale: f32,
  pub(super) metrics: &'a Metrics,
  pub(super) primary: Rgb,
  pub(super) muted: Rgb,
  pub(super) error: Rgb,
  pub(super) accent: Rgb,
  pub(super) focus: Rgb,
}
