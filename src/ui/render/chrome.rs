//! Chrome pass for the renderer.

use super::*;

/// Paints the backdrop for one widget. Built-in widgets pass their resolved
/// plate; plugin widgets have no look yet and keep the fixed scrim. Keeping the
/// choice in one place is what stops the two paths from drifting.
pub(super) fn paint_widget_backdrop(
  canvas: &mut Canvas,
  bounds: Rect,
  theme: &Theme,
  scale: f32,
  plate: Option<&WidgetPlate>,
  glass: &GlassCache,
) {
  match plate {
    // The transform already multiplied the plate opacity into the whole
    // widget, so the plate itself must not apply it a second time.
    Some(plate) => draw_widget_plate(canvas, bounds, plate, theme, scale, 1.0, glass),
    None => draw_scrim(canvas, bounds, theme, scale),
  }
}

/// Draws a themed plate behind a widget. The plate is sized from the widget's
/// content bounds plus the configured padding, so it always wraps the digits,
/// album art, and progress bar as one surface.
///
/// Shadow and glow paint first, then the glass backdrop and fill, then the
/// border. Every alpha is multiplied by `opacity`, which lets a caller fade a
/// plate without touching the widget's text colours.
pub(super) fn draw_widget_plate(
  canvas: &mut Canvas,
  bounds: Rect,
  plate: &WidgetPlate,
  theme: &Theme,
  scale: f32,
  opacity: f32,
  glass: &GlassCache,
) {
  if bounds.w <= 0.0 || bounds.h <= 0.0 || opacity <= 0.0 {
    return;
  }
  // `None` means the widget floats on the wallpaper. A shadow or glow with no
  // plate underneath would just be a floating smudge, so skip all of it.
  if plate.style == PlateStyle::None {
    return;
  }
  let pad = plate.padding * scale;
  let rect = Rect::new(
    bounds.x - pad,
    bounds.y - pad,
    bounds.w + pad * 2.0,
    bounds.h + pad * 2.0,
  );
  let radius = (plate.radius * scale).min(rect.h / 2.0);

  if plate.shadow {
    let spread = pad + 4.0 * scale;
    let shadow = Rect::new(
      bounds.x - spread,
      bounds.y - spread + 3.0 * scale,
      bounds.w + spread * 2.0,
      bounds.h + spread * 2.0,
    );
    let shadow_radius = (plate.radius * scale + 4.0 * scale).min(shadow.h / 2.0);
    canvas.fill(
      shadow,
      alpha(theme.shadow.with_alpha(90), opacity),
      shadow_radius,
    );
  }
  if let Some(glow) = plate.glow
    && plate.glow_size > 0.0
  {
    let spread = pad + plate.glow_size * scale;
    let glow_rect = Rect::new(
      bounds.x - spread,
      bounds.y - spread,
      bounds.w + spread * 2.0,
      bounds.h + spread * 2.0,
    );
    let glow_radius = (plate.radius * scale + plate.glow_size * scale).min(glow_rect.h / 2.0);
    canvas.fill(glow_rect, alpha(glow.with_alpha(40), opacity), glow_radius);
  }

  match plate.style {
    PlateStyle::Tint => canvas.fill(rect, alpha(theme.tint.with_alpha(180), opacity), radius),
    PlateStyle::Solid => canvas.fill(
      rect,
      alpha(plate.color.unwrap_or(theme.tint), opacity),
      radius,
    ),
    PlateStyle::Gradient => {
      let start = plate.color.unwrap_or(theme.tint);
      let end = plate.color_end.unwrap_or(Rgb::rgba(0, 0, 0, 0));
      canvas.linear_gradient_stops(
        rect,
        radius,
        plate.gradient_angle,
        &[(0.0, alpha(start, opacity)), (1.0, alpha(end, opacity))],
      );
    }
    PlateStyle::Glass => {
      // A backdrop blur, not a blur of the plate's own colour: the cached
      // wallpaper is sampled through the rounded rect, then an optional
      // translucent tint goes on top. With no colour this is pure blur.
      glass.backdrop(canvas, rect, radius, opacity);
      if let Some(color) = plate.color {
        canvas.fill(rect, alpha(color, opacity), radius);
      }
    }
    PlateStyle::None => {}
  }

  match (plate.border, plate.style) {
    (Some(border), _) => canvas.stroke(rect, alpha(border, opacity), radius, 1.0),
    // The historical scrim always had a faint theme border. Only Tint keeps
    // it implicitly; Solid, Gradient, and Glass get a border only when asked.
    (None, PlateStyle::Tint) => {
      canvas.stroke(
        rect,
        alpha(theme.border.with_alpha(45), opacity),
        radius,
        1.0,
      );
    }
    (None, _) => {}
  }
}

/// The fixed backplate a plugin widget still gets. Built-in widgets use
/// [`draw_widget_plate`] instead; this stays for plugins until their manifests
/// grow a look table.
pub(super) fn draw_scrim(canvas: &mut Canvas, bounds: Rect, theme: &Theme, scale: f32) {
  if bounds.w <= 0.0 || bounds.h <= 0.0 {
    return;
  }
  let pad = 12.0 * scale;
  let rect = Rect::new(
    bounds.x - pad,
    bounds.y - pad,
    bounds.w + pad * 2.0,
    bounds.h + pad * 2.0,
  );
  let radius = (14.0 * scale).min(rect.h / 2.0);
  canvas.fill(rect, theme.tint.with_alpha(180), radius);
  canvas.stroke(rect, theme.border.with_alpha(45), radius, 1.0);
}

/// Converts a manifest offset (logical pixels) into physical pixels.
#[allow(dead_code)]
pub(super) fn scaled_offset(offset: (i32, i32), scale: f32) -> (i32, i32) {
  (
    (offset.0 as f32 * scale).round() as i32,
    (offset.1 as f32 * scale).round() as i32,
  )
}
