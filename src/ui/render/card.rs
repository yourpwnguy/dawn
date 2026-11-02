//! Card pass for the renderer.

use super::*;

/// Uniform scale for a full-bleed card on a smaller display. The static decor
/// layer and the live card share this so they can never disagree.
pub(super) fn card_scale(scene: &Scene<'_>, screen: (f32, f32), scale: f32) -> f32 {
  let login = &scene.theme.login;
  if login.layout.is_full_bleed() && login.width > 0.0 && login.height > 0.0 {
    let margin = 16.0 * scale;
    let fit_w = ((screen.0 - margin * 2.0).max(1.0)) / (login.width * scale);
    let fit_h = ((screen.1 - margin * 2.0).max(1.0)) / (login.height * scale);
    scale * fit_w.min(fit_h).min(1.0)
  } else {
    scale
  }
}

/// Resolves the card rectangle after anchor placement, drag clamping, and the
/// boot offset. The static decor layer uses the same geometry as the live card,
/// so the two can never drift apart.
pub(super) fn place_card(
  scene: &Scene<'_>,
  screen: (f32, f32),
  card_scale: f32,
  m: &Metrics,
  height: f32,
) -> Rect {
  let login = &scene.theme.login;
  let placed_offset = (
    (login.placement.offset.0 as f32 * card_scale).round() as i32,
    (login.placement.offset.1 as f32 * card_scale).round() as i32,
  );
  let (base_x, base_y) = login
    .placement
    .anchor
    .place(screen, (m.card_w, height), placed_offset);
  let base_x = base_x.min((screen.0 - m.card_w).max(0.0)).max(0.0);
  let base_y = base_y.min((screen.1 - height).max(0.0)).max(0.0);
  let drag = scene.drag.offset("login");
  // Saved offsets are portable across monitors, but a position that was fine
  // on a 4K panel can push the card off a smaller laptop screen. Clamp only
  // the drag contribution so the boot animation keeps its full travel, and the
  // login form can never be dragged out of reach.
  let drag_x = (drag.0 * card_scale).clamp(-base_x, (screen.0 - m.card_w - base_x).max(-base_x));
  let drag_y = (drag.1 * card_scale).clamp(-base_y, (screen.1 - height - base_y).max(-base_y));
  let x = base_x + scene.motion.card_dx + drag_x;
  let y = base_y + scene.motion.card_dy + drag_y;
  Rect::new(x, y, m.card_w, height)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_card(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
  primary: Rgb,
  muted: Rgb,
  error: Rgb,
  accent: Rgb,
  focus: Rgb,
  regions: &mut Vec<HitRegion>,
  decor: DecorFilter,
) {
  let view = scene.view;
  let login = &scene.theme.login;
  let card_scale = card_scale(scene, screen, scale);
  let m = Metrics::new(scene.theme, screen, card_scale);
  let height = card_height(text, view, scene.theme, &m, card_scale);
  let card = place_card(scene, screen, card_scale, &m, height);
  let paint = CardPaint {
    card,
    card_scale,
    metrics: &m,
    primary,
    muted,
    error,
    accent,
    focus,
  };

  if login.rotation.abs() <= 0.01 {
    draw_card_chrome(canvas, text, glass, scene, &paint, regions, decor);
    draw_front_widgets(canvas, text, glass, scene, &paint, regions);
    draw_card_form(canvas, text, scene, &paint, regions);
    return;
  }

  // Rotate the card in up to two layers so a front widget can land between the
  // decor and the form in screen space. Painting the widget inside either
  // layer would rotate its pixels away from the hit region the pointer sees.
  let center = card.center();
  let reach = (card.w.hypot(card.h) / 2.0 + 32.0 * card_scale).max(64.0);
  let bounds = Rect::new(center.0 - reach, center.1 - reach, reach * 2.0, reach * 2.0);
  let layer_w = bounds.w.ceil() as u32;
  let layer_h = bounds.h.ceil() as u32;
  let Some(mut chrome_layer) = Canvas::region(layer_w, layer_h, (bounds.x, bounds.y))
    .map(|layer| layer.with_screen(canvas.screen()))
  else {
    // No intermediate surface: fall back to the unrotated order. The card
    // still paints, so a failed allocation never blanks the login form.
    draw_card_chrome(canvas, text, glass, scene, &paint, regions, decor);
    draw_front_widgets(canvas, text, glass, scene, &paint, regions);
    draw_card_form(canvas, text, scene, &paint, regions);
    return;
  };
  let mut chrome_regions = Vec::new();
  draw_card_chrome(
    &mut chrome_layer,
    text,
    glass,
    scene,
    &paint,
    &mut chrome_regions,
    decor,
  );
  if !has_front_widgets(scene) {
    // No widget asked for the front layer, so the card can stay a single
    // layer exactly like before. This keeps the common rotated card cheap.
    draw_card_form(&mut chrome_layer, text, scene, &paint, &mut chrome_regions);
    canvas.draw_layer(&chrome_layer, center, login.rotation, 1.0);
    for region in chrome_regions {
      regions.push(region.rotated_around(center, login.rotation));
    }
    return;
  }
  canvas.draw_layer(&chrome_layer, center, login.rotation, 1.0);
  for region in chrome_regions {
    regions.push(region.rotated_around(center, login.rotation));
  }

  draw_front_widgets(canvas, text, glass, scene, &paint, regions);

  let Some(mut form_layer) = Canvas::region(layer_w, layer_h, (bounds.x, bounds.y))
    .map(|layer| layer.with_screen(canvas.screen()))
  else {
    // The form still has to paint even if the second layer cannot be
    // allocated. It loses the card rotation in that unlikely case, which is
    // better than hiding the inputs.
    draw_card_form(canvas, text, scene, &paint, regions);
    return;
  };
  let mut form_regions = Vec::new();
  draw_card_form(&mut form_layer, text, scene, &paint, &mut form_regions);
  canvas.draw_layer(&form_layer, center, login.rotation, 1.0);
  for region in form_regions {
    regions.push(region.rotated_around(center, login.rotation));
  }
}

/// Paints the widgets that opted into the front layer. Both built-ins and
/// plugins can take this path; the form paints afterwards either way.
pub(super) fn draw_front_widgets(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  paint: &CardPaint<'_>,
  regions: &mut Vec<HitRegion>,
) {
  draw_plugins(
    canvas,
    text,
    glass,
    scene,
    WidgetFrame::card(paint.card, paint.card_scale),
    paint.primary,
    paint.muted,
    paint.accent,
    WidgetLayer::Front,
    regions,
  );
}

/// Paints the card surface and its decor. This is the half of the card that a
/// front widget is allowed to cover.
pub(super) fn draw_card_chrome(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  paint: &CardPaint<'_>,
  regions: &mut Vec<HitRegion>,
  decor: DecorFilter,
) {
  let login = &scene.theme.login;
  let hero = login.layout.is_full_bleed();
  let card = paint.card;
  let card_scale = paint.card_scale;
  let m = paint.metrics;
  match login.surface {
    SurfaceStyle::None => {}
    SurfaceStyle::Glass if hero => {
      draw_hero_surface(
        canvas,
        scene,
        card,
        m.radius,
        card_scale,
        login.show_shadow,
        login.show_border,
      );
    }
    SurfaceStyle::Glass => {
      glass.panel_visible(
        canvas,
        card,
        m.radius,
        scene.theme,
        scene.motion.opacity,
        login.show_shadow,
        login.show_border,
      );
    }
    SurfaceStyle::Flat if hero => {
      draw_hero_surface(
        canvas,
        scene,
        card,
        m.radius,
        card_scale,
        login.show_shadow,
        login.show_border,
      );
    }
    SurfaceStyle::Flat => {
      let fill = login.panel.unwrap_or(scene.theme.tint);
      canvas.fill(card, fill, m.radius);
      if login.show_border {
        let border = login.border.unwrap_or(scene.theme.border);
        canvas.stroke(card, border, m.radius, card_scale.max(1.0));
      }
    }
  }
  // The outer wrapper adds the card rotation to every region. Keeping this
  // one at zero avoids rotating the card's own hit box twice.
  regions.push(HitRegion::new("login", card, login.draggable));
  draw_login_decor(canvas, text, glass, scene, card, card_scale, decor);
}

/// Paints the interactive form: fields, button, status, session row, hints, and
/// the card's drag highlight. A front widget paints underneath this pass, so
/// the form always wins the top layer.
pub(super) fn draw_card_form(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  paint: &CardPaint<'_>,
  regions: &mut Vec<HitRegion>,
) {
  let view = scene.view;
  let login = &scene.theme.login;
  let hero = login.layout.is_full_bleed();
  let card = paint.card;
  let card_scale = paint.card_scale;
  let m = paint.metrics;
  let (primary, muted, error, accent, focus) = (
    paint.primary,
    paint.muted,
    paint.error,
    paint.accent,
    paint.focus,
  );

  // The hero tab belongs to the card chrome, but it paints at the start of the
  // form pass so a front widget lands directly after the decor and cannot
  // cover the tab either.
  if hero {
    draw_login_tab(canvas, text, scene, card, card_scale, primary);
  }

  let mut cursor_y = if hero {
    card.y + login.form_offset.1 as f32 * card_scale
  } else {
    card.y + m.pad
  };
  let left = if hero {
    card.x + login.form_offset.0 as f32 * card_scale
  } else {
    card.x + m.pad
  };
  let inner_w = if hero && login.form_width > 0.0 {
    login.form_width * card_scale
  } else {
    m.inner_w
  };
  let clip = card.inset(2.0);

  if !view.greeting.is_empty() {
    let (_, h) = text.measure(&view.greeting, m.base * 1.25, inner_w, true);
    text.draw(
      canvas,
      &view.greeting,
      Style::new(m.base * 1.25, primary),
      Area::new(left, cursor_y, inner_w)
        .align(Align::Center)
        .clip(clip),
    );
    cursor_y += h + m.gap;
  }

  let (used, username_rect) = draw_field_block(
    canvas,
    text,
    scene,
    &login.username,
    left,
    cursor_y,
    inner_w,
    card_scale,
    &view.username,
    view.focus == crate::ui::view::Focus::Username && !view.busy,
    view.username_cursor,
    !view.busy,
    key_pop_age(
      scene,
      &login.username,
      view.focus == crate::ui::view::Focus::Username,
    ),
    primary,
    muted,
    accent,
    focus,
    clip,
  );
  cursor_y += used;
  regions.push(HitRegion::rotated(
    "login.username",
    username_rect,
    false,
    login.username.rotation,
  ));

  if let Some(prompt) = &view.prompt {
    let (_, h) = text.measure(prompt, m.base * 0.95, inner_w, true);
    cursor_y += m.gap * 0.7;
    text.draw(
      canvas,
      prompt,
      Style::new(m.base * 0.95, muted),
      Area::new(left, cursor_y, inner_w).clip(clip),
    );
    cursor_y += h;
  }

  cursor_y += m.gap;
  let (used, password_rect) = draw_field_block(
    canvas,
    text,
    scene,
    &login.password,
    left,
    cursor_y,
    inner_w,
    card_scale,
    &view.password,
    view.focus == crate::ui::view::Focus::Password && !view.busy,
    view.password_cursor,
    !view.busy,
    key_pop_age(
      scene,
      &login.password,
      view.focus == crate::ui::view::Focus::Password,
    ),
    primary,
    muted,
    accent,
    focus,
    clip,
  );
  cursor_y += used;
  regions.push(HitRegion::rotated(
    "login.password",
    password_rect,
    false,
    login.password.rotation,
  ));

  if login.button.show {
    cursor_y += login.button.gap * card_scale;
    let button_rect = Rect::new(left, cursor_y, inner_w, login.button.height * card_scale);
    draw_login_button(
      canvas,
      text,
      scene,
      &login.button,
      button_rect,
      card_scale,
      primary,
      accent,
      clip,
    );
    // The bracket is decoration outside the hit area, so a click on the gap
    // still lands on the card instead of submitting unexpectedly.
    regions.push(HitRegion::rotated(
      "login.button",
      button_rect,
      false,
      login.button.rotation,
    ));
    cursor_y += button_rect.h + login.button.bracket_gap * card_scale;
  }
  cursor_y += m.gap;

  if let Some(status) = &view.status {
    let color = match status.tone {
      Tone::Info => muted,
      Tone::Error => error,
    };
    let (_, h) = text.measure(&status.text, m.base * 0.95, inner_w, true);
    text.draw(
      canvas,
      &status.text,
      Style::new(m.base * 0.95, color),
      Area::new(left, cursor_y, inner_w).clip(clip),
    );
    cursor_y += h.max(m.base) + m.gap;
  } else {
    cursor_y += m.gap * 0.15;
  }

  if login.session.show {
    cursor_y += m.gap * 0.5;
    draw_session_row(
      canvas,
      text,
      view,
      scene.theme,
      m.base,
      Rect::new(left, cursor_y, inner_w, m.base * 1.2),
      Some(clip),
      primary,
      muted,
      accent,
      error,
    );
    cursor_y += m.base * 1.2 + m.gap * 0.55;
  }

  let hint = hint_text(view, login);
  if !hint.is_empty() {
    text.draw_line(
      canvas,
      &hint,
      Style::new(m.base * 0.82, muted.with_alpha(190)),
      Area::new(left, cursor_y, inner_w).clip(clip),
    );
  }

  if login.draggable {
    draw_drag_highlight(canvas, card, "login", scene, accent, card_scale);
  }
}

/// Paints the wide hero panel. The panel fades in from the artwork side so the
/// wallpaper stays visible on the left while the form side has a solid,
/// contrast-safe backdrop. A wallpaper is never dimmed by the glass cache here:
/// the hero look is meant to be crisp, not frosted.
pub(super) fn draw_hero_surface(
  canvas: &mut Canvas,
  scene: &Scene<'_>,
  card: Rect,
  radius: f32,
  card_scale: f32,
  shadow: bool,
  border: bool,
) {
  let login = &scene.theme.login;
  let shadow_color = scene.theme.shadow;
  if shadow {
    canvas.fill(
      Rect::new(card.x, card.y + 12.0 * card_scale, card.w, card.h),
      shadow_color.with_alpha(55),
      radius + 8.0 * card_scale,
    );
    canvas.fill(
      Rect::new(
        card.x - 2.0 * card_scale,
        card.y + 2.0 * card_scale,
        card.w + 4.0 * card_scale,
        card.h + 4.0 * card_scale,
      ),
      shadow_color.with_alpha(45),
      radius + 3.0 * card_scale,
    );
  }

  let panel = login.panel.unwrap_or(scene.theme.tint);
  let (start, end) = login.panel_fade;
  if start >= 1.0 || start >= end {
    canvas.fill(card, panel, radius);
  } else {
    canvas.horizontal_gradient_stops(
      card,
      radius,
      &[
        (0.0, panel.with_alpha(0)),
        (start, panel.with_alpha(0)),
        (end, panel),
        (1.0, panel),
      ],
    );
  }
  if border {
    let border = login.border.unwrap_or(scene.theme.border);
    canvas.stroke(card, border, radius, card_scale.max(1.0));
  }
}

/// A small translucent tab over the artwork, matching the browser login page
/// language. It is decoration only; the form focus still lives in the fields.
pub(super) fn draw_login_tab(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  card: Rect,
  card_scale: f32,
  primary: Rgb,
) {
  let label = &scene.theme.login.tab;
  if label.is_empty() {
    return;
  }
  let size = scene.theme.font_size.max(10.0) * card_scale * 1.3;
  let (width, height) = text.measure(label, size, card.w, false);
  let pad_x = 18.0 * card_scale;
  let pad_y = 8.0 * card_scale;
  let rect = Rect::new(
    card.x,
    card.y + 10.0 * card_scale,
    width + pad_x * 2.0,
    height + pad_y * 2.0,
  );
  canvas.fill(rect, Rgb::solid(255, 255, 255).with_alpha(30), 0.0);
  text.draw_line(
    canvas,
    label,
    Style::new(size, primary),
    Area::new(
      rect.x + pad_x,
      rect.y + (rect.h - height) / 2.0,
      rect.w - pad_x * 2.0,
    ),
  );
}
