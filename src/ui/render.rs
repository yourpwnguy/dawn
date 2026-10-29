//! Turns a [`View`] into pixels.
//!
//! There is no retained widget tree. Every call paints a whole frame:
//! background, blurred glass cache, plugin widgets, card, overlays. That is
//! affordable because the window only redraws on input, a visible clock
//! change, an auth event, or an animation deadline.
//!
//! Plugin widgets are painted before any core UI. A greeter cannot let a
//! third-party widget cover the password field, so the ordering here is a
//! security property, not a style choice.

use std::time::{Duration, Instant};

use cosmic_text::Align;

use crate::color::Rgb;
use crate::config::{ButtonConfig, ButtonEffect, HoverReveal, LoginLayout, SurfaceStyle};
use crate::plugin::{Plugin, Widget};
use crate::ui::anim::{cycle, ease_out_cubic};
use crate::ui::background::BackgroundView;
use crate::ui::canvas::{Canvas, Rect, rotated_bounds};
use crate::ui::clock;
use crate::ui::drag::{Drag, HitRegion};
use crate::ui::glass::GlassCache;
use crate::ui::media::MediaSnapshot;
use crate::ui::motion::MotionFrame;
use crate::ui::text::{Area, Style, TextEngine};
use crate::ui::theme::Theme;
use crate::ui::view::{Tone, View};

const MAX_PICKER_ROWS: usize = 10;

/// Everything the renderer needs for one frame. Grouping it keeps the paint
/// call from growing a dozen positional arguments as features land.
pub struct Scene<'a> {
  pub view: &'a View,
  pub theme: &'a Theme,
  pub plugins: &'a [Plugin],
  pub now: &'a jiff::Zoned,
  /// Instant the UI started, used to phase decor motion. Screenshots set this
  /// to `frame_now` so they capture the neutral pose.
  pub started: Instant,
  pub frame_now: Instant,
  pub scale: f32,
  pub motion: MotionFrame,
  pub monitor: Option<&'a str>,
  pub background_color: Rgb,
  pub asset_warning: Option<&'a str>,
  /// Current drag offsets, keyed by widget id.
  pub drag: &'a Drag,
}

/// Draws one complete frame and returns the hit regions it produced. The
/// window feeds those regions back into [`Drag`] on the next pointer event.
pub fn paint(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  background: &mut BackgroundView,
  glass: &mut GlassCache,
  scene: &Scene<'_>,
) -> Vec<HitRegion> {
  let scale = normalized_scale(scene.scale);
  background.paint(canvas, scene.frame_now);
  glass.prepare(
    canvas,
    background.frame_id(scene.frame_now),
    scene.frame_now,
    scene.theme,
    scale,
    background.is_animated(),
  );
  paint_overlay(canvas, text, glass, scene)
}

/// Paints only the interactive layer: plugins, the card, the picker, badges,
/// and warnings. The GPU path uses this on a transparent canvas and lets the
/// compositor put the wallpaper underneath, while the CPU path calls it after
/// [`BackgroundView::paint`] on the same canvas.
///
/// The caller must call [`GlassCache::prepare`] for this frame first. This
/// function only reads the cached blur, so it can stay identical between the
/// two paths and cannot accidentally paint a panel with a stale backdrop.
///
/// Keeping one function for both paths is deliberate: hit regions, focus
/// rings, and the security ordering of plugin pixels cannot drift between the
/// GPU renderer and the screenshot renderer.
pub fn paint_overlay(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
) -> Vec<HitRegion> {
  let scale = normalized_scale(scene.scale);
  let screen = (canvas.width() as f32, canvas.height() as f32);
  let primary = scene.theme.primary_on(scene.background_color);
  let muted = scene.theme.muted_on(scene.background_color);
  let error = scene.theme.error_on(scene.background_color);
  let accent = scene.theme.accent_on(scene.background_color);
  let focus = scene.theme.focus_on(scene.background_color);

  let mut regions = Vec::new();
  draw_plugins(
    canvas,
    text,
    scene,
    screen,
    scale,
    primary,
    muted,
    accent,
    &mut regions,
  );
  draw_card(
    canvas,
    text,
    glass,
    scene,
    screen,
    scale,
    primary,
    muted,
    error,
    accent,
    focus,
    &mut regions,
  );
  if scene.view.picker_open() {
    draw_picker(
      canvas, text, glass, scene, screen, scale, primary, muted, accent,
    );
  }
  if scene.view.dry_run {
    draw_badge(canvas, text, glass, scene, scale, accent);
  }
  draw_warnings(canvas, text, scene, screen, scale, error);
  regions
}

/// The window and screenshot paths both accept a caller-provided scale, but a
/// zero or negative value would collapse every layout constant. Clamping here
/// keeps the two render entry points on the same rule.
fn normalized_scale(scale: f32) -> f32 {
  if scale.is_finite() && scale > 0.25 {
    scale
  } else {
    1.0
  }
}

/// Layout constants derived from the configured font size. Keeping them in one
/// place means a larger font grows the card instead of overlapping the fields.
struct Metrics {
  base: f32,
  pad: f32,
  gap: f32,
  inner_w: f32,
  card_w: f32,
  radius: f32,
}

impl Metrics {
  fn new(theme: &Theme, screen: (f32, f32), scale: f32) -> Self {
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
fn paint_transformed(
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
fn paint_rotated(
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
fn field_size(theme: &Theme, field: &crate::config::FieldConfig, scale: f32) -> f32 {
  if field.font_size > 0.0 {
    field.font_size * scale
  } else {
    theme.font_size.max(10.0) * scale
  }
}

/// The height of one field, including its padding. Zero means "derive it".
fn field_height(theme: &Theme, field: &crate::config::FieldConfig, scale: f32) -> f32 {
  if field.height > 0.0 {
    field.height * scale
  } else {
    field_size(theme, field, scale) * 2.5
  }
}

/// Height of an optional label above a field.
fn label_height(text: &mut TextEngine, label: &str, size: f32, width: f32) -> f32 {
  if label.is_empty() {
    0.0
  } else {
    text.measure(label, size, width, false).1
  }
}

/// Computes the card height before drawing, so the card can be positioned and
/// its background painted before any text lands on it. Every step here has a
/// matching step in `draw_card`; if you change one, change both.
fn card_height(text: &mut TextEngine, view: &View, theme: &Theme, m: &Metrics, scale: f32) -> f32 {
  let login = &theme.login;
  if login.layout == LoginLayout::Hero && login.height > 0.0 {
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
fn field_block_height(
  text: &mut TextEngine,
  theme: &Theme,
  field: &crate::config::FieldConfig,
  m: &Metrics,
  scale: f32,
) -> f32 {
  let label = label_height(
    text,
    &field.label,
    field_size(theme, field, scale) * 0.8,
    m.inner_w,
  );
  let label_gap = if label > 0.0 { m.gap * 0.35 } else { 0.0 };
  label + label_gap + field_height(theme, field, scale)
}

#[allow(clippy::too_many_arguments)]
fn draw_card(
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
) {
  let view = scene.view;
  let login = &scene.theme.login;
  let hero = login.layout == LoginLayout::Hero;
  // A fixed hero card is a website clone, but it still has to fit a smaller
  // display. Scale the whole card uniformly instead of letting the form run
  // off the edge; 16 px of breathing room is enough for the shadow.
  let card_scale = if hero && login.width > 0.0 && login.height > 0.0 {
    let margin = 16.0 * scale;
    let fit_w = ((screen.0 - margin * 2.0).max(1.0)) / (login.width * scale);
    let fit_h = ((screen.1 - margin * 2.0).max(1.0)) / (login.height * scale);
    scale * fit_w.min(fit_h).min(1.0)
  } else {
    scale
  };
  let m = Metrics::new(scene.theme, screen, card_scale);
  let height = card_height(text, view, scene.theme, &m, card_scale);
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
  let card = Rect::new(x, y, m.card_w, height);

  if login.rotation.abs() <= 0.01 {
    draw_card_body(
      canvas, text, glass, scene, card, card_scale, &m, primary, muted, error, accent, focus,
      regions,
    );
    return;
  }

  // Rotate the whole card as one layer. Nested field/button rotations still
  // compose inside the layer, and every hit region is moved around the card
  // centre afterwards so the pointer keeps matching the pixels.
  let center = card.center();
  let reach = (card.w.hypot(card.h) / 2.0 + 32.0 * card_scale).max(64.0);
  let bounds = Rect::new(center.0 - reach, center.1 - reach, reach * 2.0, reach * 2.0);
  let width = bounds.w.ceil() as u32;
  let height = bounds.h.ceil() as u32;
  let Some(mut layer) = Canvas::region(width, height, (bounds.x, bounds.y))
    .map(|layer| layer.with_screen(canvas.screen()))
  else {
    draw_card_body(
      canvas, text, glass, scene, card, card_scale, &m, primary, muted, error, accent, focus,
      regions,
    );
    return;
  };
  let mut inner = Vec::new();
  draw_card_body(
    &mut layer, text, glass, scene, card, card_scale, &m, primary, muted, error, accent, focus,
    &mut inner,
  );
  canvas.draw_layer(&layer, center, login.rotation, 1.0);
  for region in inner {
    regions.push(region.rotated_around(center, login.rotation));
  }
}

#[allow(clippy::too_many_arguments)]
fn draw_card_body(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  card: Rect,
  card_scale: f32,
  m: &Metrics,
  primary: Rgb,
  muted: Rgb,
  error: Rgb,
  accent: Rgb,
  focus: Rgb,
  regions: &mut Vec<HitRegion>,
) {
  let view = scene.view;
  let login = &scene.theme.login;
  let hero = login.layout == LoginLayout::Hero;
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
  draw_login_decor(canvas, text, scene, card, card_scale);
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

  draw_drag_highlight(canvas, card, "login", scene, accent, card_scale);
}

/// Paints the wide hero panel. The panel fades in from the artwork side so the
/// wallpaper stays visible on the left while the form side has a solid,
/// contrast-safe backdrop. A wallpaper is never dimmed by the glass cache here:
/// the hero look is meant to be crisp, not frosted.
fn draw_hero_surface(
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
fn draw_login_tab(
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

/// Draws the configured decor inside the card. Images are already decoded, so
/// this only card_scales them to their display width. The form paints afterwards,
/// which means a decor item can never cover an input or the submit button.
fn draw_login_decor(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  card: Rect,
  card_scale: f32,
) {
  let animate = !scene.theme.motion.reduced && !scene.theme.low_power;
  let elapsed = if animate {
    scene
      .frame_now
      .saturating_duration_since(scene.started)
      .as_secs_f32()
  } else {
    0.0
  };
  for item in &scene.theme.decor {
    let sample = item.motion.sample(elapsed);
    let opacity = item.opacity * sample.opacity;
    let angle = item.rotation + sample.angle;
    let offset = (
      ((item.offset.0 as f32 + sample.offset.0) * card_scale).round() as i32,
      ((item.offset.1 as f32 + sample.offset.1) * card_scale).round() as i32,
    );
    if let Some(image) = &item.image {
      let width = item.width * card_scale;
      if width <= 0.0 || image.width() == 0 {
        continue;
      }
      let height = width * image.height() as f32 / image.width() as f32;
      let (x, y) = item.anchor.place((card.w, card.h), (width, height), offset);
      let rect = Rect::new(card.x + x, card.y + y, width, height);
      canvas.image_transformed(image, rect, angle, sample.scale, opacity);
    } else {
      let size = item.size * card_scale;
      let (width, height) = text.measure(&item.text, size, card.w.max(1.0), false);
      let (x, y) = item
        .anchor
        .place((card.w, card.h), (width.max(1.0), height), offset);
      let rect = Rect::new(card.x + x, card.y + y, width.max(1.0), height);
      let style = if item.bold {
        Style::new(size, item.color).bold()
      } else {
        Style::new(size, item.color)
      };
      paint_transformed(
        canvas,
        rect,
        rect.center(),
        angle,
        sample.scale,
        opacity,
        |layer, _| {
          text.draw_line(
            layer,
            &item.text,
            style.tracking(item.tracking),
            Area::new(rect.x, rect.y, width.max(1.0)),
          );
        },
      );
    }
  }
}

/// The target-style submit button: a solid fill inside a broken dark bracket.
/// The text is separately configurable so a theme can keep the same geometry
/// with its own language.
#[allow(clippy::too_many_arguments)]
fn draw_login_button(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &ButtonConfig,
  rect: Rect,
  card_scale: f32,
  primary: Rgb,
  accent: Rgb,
  clip: Rect,
) {
  let hovered = scene.drag.is_hovered("login.button");
  let reduced = scene.theme.motion.reduced || scene.theme.low_power;
  let duration = Duration::from_secs_f32((config.hover_ms / 1000.0).max(0.0));
  let hover_t = if reduced {
    f32::from(u8::from(hovered))
  } else {
    scene
      .drag
      .hover_t("login.button", scene.frame_now, duration)
  };
  let ease = ease_out_cubic(hover_t);
  let age = scene.drag.hover_elapsed("login.button", scene.frame_now);

  let scale = 1.0 + (config.hover_scale - 1.0) * ease;
  let lift = config.hover_lift * ease * card_scale;
  let center = rect.center();
  let width = rect.w * scale;
  let height = rect.h * scale;
  let animated = Rect::new(
    center.0 - width / 2.0,
    center.1 - height / 2.0 - lift,
    width,
    height,
  );
  let bounds =
    animated.inset(-(config.bracket_gap.max(config.glow_size) * card_scale + 10.0 * card_scale));
  paint_rotated(
    canvas,
    bounds,
    animated.center(),
    config.rotation,
    |layer| {
      draw_login_button_body(
        layer, text, scene, config, animated, card_scale, scale, primary, accent, clip, hovered,
        ease, age, reduced,
      );
    },
  );
}

#[allow(clippy::too_many_arguments)]
fn draw_login_button_body(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &ButtonConfig,
  rect: Rect,
  card_scale: f32,
  scale: f32,
  primary: Rgb,
  accent: Rgb,
  clip: Rect,
  hovered: bool,
  ease: f32,
  age: Duration,
  reduced: bool,
) {
  let base = config.background.unwrap_or(accent);
  let base_end = config.background_end.unwrap_or(base);
  let hover = config
    .hover_background
    .unwrap_or_else(|| base.lighten(0.16));
  let hover_end = config
    .hover_background_end
    .unwrap_or_else(|| base_end.lighten(0.16));
  let radius = config.radius * card_scale;

  let glow_size = if config.effect == ButtonEffect::Glow {
    config.glow_size.max(8.0)
  } else {
    config.glow_size
  };
  if glow_size > 0.0 && ease > 0.0 {
    let glow = config.glow.unwrap_or(hover);
    let pulse = if matches!(config.effect, ButtonEffect::Pulse | ButtonEffect::Glow) && !reduced {
      0.55
        + 0.45
          * (cycle(
            age,
            Duration::from_secs_f32((config.effect_ms / 1000.0).max(0.1)),
          ) * std::f32::consts::TAU)
            .sin()
    } else {
      1.0
    };
    let size = glow_size * card_scale;
    for (index, falloff) in [(1.0f32, 0.30f32), (0.66, 0.22), (0.33, 0.14)] {
      let expand = size * index;
      let alpha = (255.0 * falloff * ease * pulse) as u8;
      canvas.fill(rect.inset(-expand), glow.with_alpha(alpha), radius + expand);
    }
  }

  if config.bracket_gap > 0.0 {
    let color = config.bracket.unwrap_or(scene.theme.shadow);
    draw_bracket(
      canvas,
      rect.inset(-config.bracket_gap * card_scale),
      color,
      (1.4 * card_scale).max(1.0),
    );
  }

  draw_button_fill(
    canvas,
    rect,
    radius,
    config.gradient_angle,
    base,
    base_end,
    config.background_end.is_some(),
  );

  if config.hover_direction == HoverReveal::Fade {
    let start = base.mix(hover, ease);
    let end = base_end.mix(hover_end, ease);
    draw_button_fill(
      canvas,
      rect,
      radius,
      config.gradient_angle,
      start,
      end,
      config.background_end.is_some() || config.hover_background_end.is_some(),
    );
  } else if ease > 0.0 {
    let (angle, stops) = reveal_stops(config.hover_direction, ease, hover, hover_end);
    canvas.linear_gradient_stops(rect, radius, angle, &stops);
  }

  if hovered && !reduced {
    draw_button_effect(canvas, config, rect, radius, ease, age);
  }

  let size = if config.font_size > 0.0 {
    config.font_size * card_scale * scale
  } else {
    scene.theme.font_size.max(10.0) * card_scale * 1.05 * scale
  };
  let text_color = config.text_color.unwrap_or(primary).mix(
    config
      .hover_text_color
      .unwrap_or_else(|| config.text_color.unwrap_or(primary)),
    ease,
  );
  let mut style = Style::new(size, text_color);
  if config.bold {
    style = style.bold();
  }
  style = style.tracking(config.tracking);
  let (ink_top, ink_bottom) = text.ink_bounds(&config.text, size, style.weight, style.tracking);
  text.draw_line(
    canvas,
    &config.text,
    style,
    Area::new(
      rect.x,
      rect.center().1 - (ink_top + ink_bottom) / 2.0,
      rect.w,
    )
    .align(Align::Center)
    .clip(clip),
  );
}

fn draw_button_fill(
  canvas: &mut Canvas,
  rect: Rect,
  radius: f32,
  angle: f32,
  start: Rgb,
  end: Rgb,
  gradient: bool,
) {
  if gradient && start != end {
    canvas.linear_gradient_stops(rect, radius, angle, &[(0.0, start), (1.0, end)]);
  } else {
    canvas.fill(rect, start, radius);
  }
}

/// Directional hover wipes. The edge is feathered so it reads as paint being
/// poured in rather than a rectangle sliding across the button.
fn reveal_stops(
  direction: HoverReveal,
  progress: f32,
  color: Rgb,
  end_color: Rgb,
) -> (f32, Vec<(f32, Rgb)>) {
  let t = progress.clamp(0.0, 1.0);
  let feather = 0.08;
  let clear = color.with_alpha(0);
  let clear_end = end_color.with_alpha(0);
  let mut stops = match direction {
    HoverReveal::Left => vec![
      (0.0, color),
      (t, end_color),
      ((t + feather).min(1.0), clear_end),
      (1.0, clear),
    ],
    HoverReveal::Right => vec![
      (0.0, clear),
      ((1.0 - t - feather).max(0.0), clear_end),
      (1.0 - t, end_color),
      (1.0, color),
    ],
    HoverReveal::Up => vec![
      (0.0, color),
      (t, end_color),
      ((t + feather).min(1.0), clear_end),
      (1.0, clear),
    ],
    HoverReveal::Down => vec![
      (0.0, clear),
      ((1.0 - t - feather).max(0.0), clear_end),
      (1.0 - t, end_color),
      (1.0, color),
    ],
    HoverReveal::Center => {
      let edge = (t / 2.0).min(0.5);
      vec![
        (0.0, color),
        (edge, end_color),
        ((edge + feather).min(1.0), clear_end),
        ((1.0 - edge - feather).max(0.0), clear_end),
        (1.0 - edge, end_color),
        (1.0, color),
      ]
    }
    HoverReveal::Fade => vec![(0.0, color), (1.0, end_color)],
  };
  stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
  let angle = match direction {
    HoverReveal::Left | HoverReveal::Center | HoverReveal::Fade => 0.0,
    HoverReveal::Right => 180.0,
    HoverReveal::Up => 270.0,
    HoverReveal::Down => 90.0,
  };
  (angle, stops)
}

fn draw_button_effect(
  canvas: &mut Canvas,
  config: &ButtonConfig,
  rect: Rect,
  radius: f32,
  ease: f32,
  age: Duration,
) {
  let period = Duration::from_secs_f32((config.effect_ms / 1000.0).max(0.1));
  let phase = cycle(age, period);
  let color = config
    .effect_color
    .unwrap_or(Rgb::solid(255, 255, 255))
    .with_alpha((255.0 * config.effect_strength * ease) as u8);
  match config.effect {
    ButtonEffect::None => {}
    ButtonEffect::Sweep | ButtonEffect::Sheen => {
      let width = if config.effect == ButtonEffect::Sheen {
        0.05
      } else {
        0.16
      };
      let center = -0.25 + phase * 1.5;
      let stops = [
        (0.0, color.with_alpha(0)),
        ((center - width).max(0.0), color.with_alpha(0)),
        (center.clamp(0.0, 1.0), color),
        ((center + width).min(1.0), color.with_alpha(0)),
        (1.0, color.with_alpha(0)),
      ];
      canvas.linear_gradient_stops(rect, radius, config.effect_angle, &stops);
    }
    ButtonEffect::Glow => {}
    ButtonEffect::Pulse => {
      let wave = 0.5 + 0.5 * (phase * std::f32::consts::TAU).sin();
      let alpha = (255.0 * config.effect_strength * ease * wave * 0.35) as u8;
      canvas.fill(rect, color.with_alpha(alpha), radius);
    }
    ButtonEffect::Corners => {
      let grow = 0.5 + 0.5 * (phase * std::f32::consts::TAU).sin();
      let length = (8.0 + 10.0 * grow) * rect.h.max(1.0) / 48.0;
      draw_corner_marks(canvas, rect.inset(-4.0), color, length.max(4.0));
    }
  }
}

/// Four L-shaped marks that breathe outward around the button. This is the
/// "target acquired" cue, and it is deliberately drawn outside the fill so it
/// never makes the label harder to read.
fn draw_corner_marks(canvas: &mut Canvas, rect: Rect, color: Rgb, length: f32) {
  let length = length.min(rect.w / 3.0).min(rect.h / 3.0);
  let width = (length * 0.12).max(1.0);
  for (x, y, sx, sy) in [
    (rect.x, rect.y, 1.0, 1.0),
    (rect.right(), rect.y, -1.0, 1.0),
    (rect.x, rect.bottom(), 1.0, -1.0),
    (rect.right(), rect.bottom(), -1.0, -1.0),
  ] {
    canvas.line((x, y), (x + sx * length, y), color, width);
    canvas.line((x, y), (x, y + sy * length), color, width);
  }
}

/// The broken targeting bracket used by hero fields and buttons. The
/// top and bottom edges are split in the middle, while the sides stay solid.
fn draw_bracket(canvas: &mut Canvas, rect: Rect, color: Rgb, width: f32) {
  if rect.w <= 0.0 || rect.h <= 0.0 {
    return;
  }
  let gap = (rect.w * 0.28).min(rect.w * 0.6);
  let left_end = rect.x + (rect.w - gap) / 2.0;
  let right_start = left_end + gap;
  canvas.line((rect.x, rect.y), (left_end, rect.y), color, width);
  canvas.line((right_start, rect.y), (rect.right(), rect.y), color, width);
  canvas.line(
    (rect.x, rect.bottom()),
    (left_end, rect.bottom()),
    color,
    width,
  );
  canvas.line(
    (right_start, rect.bottom()),
    (rect.right(), rect.bottom()),
    color,
    width,
  );
  canvas.line((rect.x, rect.y), (rect.x, rect.bottom()), color, width);
  canvas.line(
    (rect.right(), rect.y),
    (rect.right(), rect.bottom()),
    color,
    width,
  );
}

/// Draws the leading user/lock glyph. These are vector shapes instead of font
/// glyphs so the icon stays crisp at every card_scale and never depends on the
/// configured family.
fn draw_field_icon(canvas: &mut Canvas, icon: &str, x: f32, center_y: f32, size: f32, color: Rgb) {
  if size <= 0.0 {
    return;
  }
  // Both glyphs are laid out inside the same -0.38..+0.38 box. That symmetry
  // is what keeps the icon on the same optical line as the centred text.
  match icon {
    "user" => {
      canvas.fill_circle((x + size * 0.5, center_y - size * 0.22), size * 0.18, color);
      canvas.fill(
        Rect::new(
          x + size * 0.16,
          center_y + size * 0.02,
          size * 0.68,
          size * 0.36,
        ),
        color,
        size * 0.2,
      );
    }
    "lock" => {
      canvas.stroke_circle(
        (x + size * 0.5, center_y - size * 0.17),
        size * 0.2,
        color,
        (size * 0.1).max(1.0),
      );
      canvas.fill(
        Rect::new(
          x + size * 0.18,
          center_y + size * 0.03,
          size * 0.64,
          size * 0.34,
        ),
        color,
        size * 0.08,
      );
    }
    _ => {}
  }
}

/// A thin accent outline while a widget is hovered or being dragged. The
/// outline sits just outside the widget so it never covers text.
fn draw_drag_highlight(
  canvas: &mut Canvas,
  rect: Rect,
  id: &str,
  scene: &Scene<'_>,
  accent: Rgb,
  scale: f32,
) {
  let active = scene.drag.is_active(id);
  let hovered = scene.drag.is_hovered(id);
  if !active && !hovered {
    return;
  }
  let alpha = if active { 170 } else { 90 };
  canvas.stroke(
    rect.inset(-4.0 * scale),
    accent.with_alpha(alpha),
    (14.0 * scale).min(rect.h / 2.0 + 4.0 * scale),
    1.5 * scale,
  );
}

/// Paints an optional label and one input field. Returns the vertical space
/// consumed and the field rectangle, which the window uses for hit testing.
#[allow(clippy::too_many_arguments)]
fn draw_field_block(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &crate::config::FieldConfig,
  left: f32,
  top: f32,
  width: f32,
  scale: f32,
  value: &str,
  focused: bool,
  cursor: usize,
  caret_visible: bool,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  focus: Rgb,
  clip: Rect,
) -> (f32, Rect) {
  let size = field_size(scene.theme, config, scale);
  let label = label_height(text, &config.label, size * 0.8, width);
  let label_gap = if label > 0.0 {
    scene.theme.login.gap * scale * 0.35
  } else {
    0.0
  };
  let height = field_height(scene.theme, config, scale);
  let field_rect = Rect::new(left, top + label + label_gap, width, height);
  let used = label + label_gap + height;
  let pad = config.bracket_gap.max(config.icon_size) * scale + 8.0 * scale;
  let bounds = Rect::new(left, top, width, used).inset(-pad);
  paint_rotated(
    canvas,
    bounds,
    field_rect.center(),
    config.rotation,
    |layer| {
      draw_field_block_inner(
        layer,
        text,
        scene,
        config,
        left,
        top,
        width,
        scale,
        value,
        focused,
        cursor,
        caret_visible,
        primary,
        muted,
        accent,
        focus,
        clip,
      );
    },
  );
  (used, field_rect)
}

#[allow(clippy::too_many_arguments)]
fn draw_field_block_inner(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &crate::config::FieldConfig,
  left: f32,
  top: f32,
  width: f32,
  scale: f32,
  value: &str,
  focused: bool,
  cursor: usize,
  caret_visible: bool,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  focus: Rgb,
  clip: Rect,
) {
  let size = field_size(scene.theme, config, scale);
  let mut y = top;
  if !config.label.is_empty() {
    let label_size = size * 0.8;
    let (_, label_h) = text.measure(&config.label, label_size, width, false);
    text.draw_line(
      canvas,
      &config.label,
      Style::new(label_size, config.label_color.unwrap_or(muted)),
      Area::new(left, y, width).clip(clip),
    );
    y += label_h + scene.theme.login.gap * scale * 0.35;
  }
  let height = field_height(scene.theme, config, scale);
  let rect = Rect::new(left, y, width, height);
  draw_field(
    canvas,
    text,
    scene.theme,
    scale,
    &Field {
      rect,
      value,
      focused,
      cursor,
      caret_visible,
      focus_t: scene.motion.focus_t,
      primary,
      muted,
      accent,
      focus,
      config,
    },
  );
}

/// Everything needed to paint one input field. Grouped so the function does
/// not grow a positional argument for every property.
struct Field<'a> {
  rect: Rect,
  value: &'a str,
  focused: bool,
  cursor: usize,
  caret_visible: bool,
  focus_t: f32,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  focus: Rgb,
  config: &'a crate::config::FieldConfig,
}

#[allow(clippy::too_many_arguments)]
fn draw_field(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  theme: &Theme,
  scale: f32,
  field: &Field<'_>,
) {
  let base = field_size(theme, field.config, scale);
  let rect = field.rect;
  let radius = if field.config.radius > 0.0 {
    field.config.radius * scale
  } else {
    base * 0.55
  };
  if field.config.bracket_gap > 0.0 {
    let bracket_color = field
      .config
      .bracket
      .unwrap_or_else(|| theme.shadow.with_alpha(220));
    draw_bracket(
      canvas,
      rect.inset(-field.config.bracket_gap * scale),
      bracket_color,
      (1.4 * scale).max(1.0),
    );
  }
  let background = field
    .config
    .background
    .unwrap_or_else(|| theme.tint.with_alpha(110));
  if let Some(end) = field.config.background_end {
    canvas.horizontal_gradient_stops(rect, radius, &[(0.0, background), (1.0, end)]);
  } else {
    canvas.fill(rect, background, radius);
  }

  let focus_color = field.config.focus.unwrap_or(field.focus);
  let focus = if field.focused {
    0.35 + 0.65 * field.focus_t.clamp(0.0, 1.0)
  } else {
    0.0
  };
  let underline = field.config.underline;
  let rule_width = |focus: f32| {
    let base = if field.config.underline_size > 0.0 {
      field.config.underline_size * scale
    } else {
      scale
    };
    (base * (1.0 + 1.2 * focus)).max(1.0)
  };
  if let Some(color) = underline {
    let thickness = rule_width(0.0);
    canvas.fill(
      Rect::new(rect.x, rect.bottom() - thickness, rect.w, thickness),
      color,
      0.0,
    );
  }
  if focus > 0.0 {
    if underline.is_some() {
      // A line field does not need a box on focus. Grow the rule and add a
      // soft underglow so the active input is obvious without breaking the
      // cardless composition.
      let thickness = rule_width(focus);
      canvas.fill(
        Rect::new(
          rect.x,
          rect.bottom() - thickness - 2.0 * scale,
          rect.w,
          thickness + 2.0 * scale,
        ),
        focus_color.with_alpha((110.0 * focus) as u8),
        0.0,
      );
      canvas.fill(
        Rect::new(rect.x, rect.bottom() - thickness, rect.w, thickness),
        focus_color.with_alpha((150.0 + 105.0 * focus) as u8),
        0.0,
      );
    } else {
      canvas.stroke(
        rect.inset(-2.0),
        focus_color.with_alpha((70.0 * focus) as u8),
        radius + 2.0,
        2.0,
      );
      canvas.stroke(
        rect,
        focus_color.with_alpha((120.0 + 135.0 * focus) as u8),
        radius,
        1.0 + focus,
      );
    }
  } else if underline.is_none() {
    let border = field
      .config
      .border
      .unwrap_or_else(|| theme.border.with_alpha(28));
    canvas.stroke(rect, border, radius, 1.0);
  }

  let inset = if field.config.padding > 0.0 {
    field.config.padding * scale
  } else {
    base * 0.9
  };
  let mut text_x = rect.x + inset;
  if !field.config.icon.is_empty() {
    let icon_size = if field.config.icon_size > 0.0 {
      field.config.icon_size * scale
    } else {
      base * 1.1
    };
    let icon_color = field.config.icon_color.unwrap_or(field.primary);
    draw_field_icon(
      canvas,
      &field.config.icon,
      text_x,
      rect.center().1,
      icon_size,
      icon_color,
    );
    let gap = if field.config.icon_gap > 0.0 {
      field.config.icon_gap * scale
    } else {
      base * 0.55
    };
    text_x += icon_size + gap;
  }
  let empty = field.value.is_empty();
  let shown = if empty {
    field.config.placeholder.as_str()
  } else {
    field.value
  };
  let color = if empty {
    field.config.placeholder_color.unwrap_or(field.muted)
  } else {
    field.config.text.unwrap_or(field.primary)
  };

  // Centre the actual ink, not the line box. The line box carries the font's
  // ascent and descent, so centring it leaves the glyphs a pixel or two above
  // a centred icon. Ink bounds make the placeholder, the typed text, and the
  // icon share one optical centre for any family.
  let (ink_top, ink_bottom) = text.ink_bounds(shown, base, cosmic_text::Weight::NORMAL, 0.0);
  let text_y = rect.center().1 - (ink_top + ink_bottom) / 2.0;
  text.draw_line(
    canvas,
    shown,
    Style::new(base, color),
    Area::new(text_x, text_y, (rect.right() - inset - text_x).max(1.0)).clip(rect.inset(2.0)),
  );

  if field.focused && field.caret_visible {
    let prefix: String = shown.chars().take(field.cursor).collect();
    let caret_x = text_x + text.line_width(&prefix, base);
    if caret_x < rect.right() - inset * 0.5 {
      let caret_color = field.config.caret.unwrap_or(field.accent);
      let caret_width = if field.config.caret_width > 0.0 {
        field.config.caret_width * scale
      } else {
        (base * 0.12).max(1.0)
      };
      let caret_height = (ink_bottom - ink_top).max(base * 0.8) + base * 0.1;
      canvas.fill(
        Rect::new(
          caret_x,
          rect.center().1 - caret_height / 2.0,
          caret_width,
          caret_height,
        ),
        caret_color,
        radius,
      );
    }
  }
}

#[allow(clippy::too_many_arguments)]
fn draw_session_row(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  view: &View,
  theme: &Theme,
  base: f32,
  area: Rect,
  clip: Option<Rect>,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  error: Rgb,
) {
  let session = &theme.login.session;
  let size = base * 0.9;
  let y = area.y;
  let mut cursor_x = area.x;
  let label = if session.label.is_empty() {
    String::new()
  } else {
    format!("{}  ", session.label)
  };
  if !label.is_empty() {
    text.draw_line(
      canvas,
      &label,
      Style::new(size, muted),
      Area {
        clip,
        ..Area::new(cursor_x, y, area.w)
      },
    );
    cursor_x += text.line_width(&label, size);
  }

  let (name, color) = if view.session.is_empty() {
    (session.empty.as_str(), error)
  } else {
    (view.session.as_str(), accent)
  };
  let remaining = (area.w - (cursor_x - area.x)).max(0.0);
  text.draw_line(
    canvas,
    name,
    Style::new(size, color),
    Area {
      clip,
      ..Area::new(cursor_x, y, remaining)
    },
  );
  cursor_x += text.line_width(name, size);

  if view.session_count > 1 && theme.login.show_picker && !session.change_hint.is_empty() {
    let hint = format!("    {}", session.change_hint);
    let remaining = (area.w - (cursor_x - area.x)).max(0.0);
    text.draw_line(
      canvas,
      &hint,
      Style::new(size, muted.with_alpha(170)),
      Area {
        clip,
        ..Area::new(cursor_x, y, remaining)
      },
    );
  }
  let _ = primary;
}

fn hint_text(view: &View, login: &crate::config::LoginConfig) -> String {
  if !login.show_hints {
    return String::new();
  }
  if view.busy {
    return if login.hints.busy.is_empty() {
      login.busy_text.clone()
    } else {
      login.hints.busy.clone()
    };
  }
  let hint = match view.focus {
    crate::ui::view::Focus::Username => &login.hints.username,
    crate::ui::view::Focus::Password => &login.hints.password,
    crate::ui::view::Focus::Picker => &login.hints.picker,
  };
  hint.clone()
}

#[allow(clippy::too_many_arguments)]
fn draw_picker(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
) {
  // Dim the login form so it is obvious the picker owns the keyboard.
  canvas.fill(
    Rect::new(0.0, 0.0, screen.0, screen.1),
    Rgb::rgba(0, 0, 0, 150),
    0.0,
  );

  let base = scene.theme.font_size.max(10.0) * scale;
  let row_h = base * 2.3;
  let pad = base * 1.2 * scene.theme.spacing;
  let min_width = 260.0 * scale;
  let width =
    (scene.theme.login.width * scale).clamp(min_width, (screen.0 - base * 3.0).max(min_width));
  let rows = scene.view.picker.len().min(MAX_PICKER_ROWS);
  let header_h = base * 1.6;
  let height = pad * 2.0 + header_h + rows.max(1) as f32 * row_h;
  let x = (screen.0 - width) / 2.0;
  let y = ((screen.1 - height) / 2.0).max(base);
  let panel = Rect::new(x, y, width, height);

  glass.panel(
    canvas,
    panel,
    scene.theme.corner_radius * scale,
    scene.theme,
    1.0,
  );

  let left = panel.x + pad;
  let inner = panel.w - pad * 2.0;
  if !scene.theme.login.picker_title.is_empty() {
    text.draw_line(
      canvas,
      &scene.theme.login.picker_title,
      Style::new(base * 0.85, muted),
      Area::new(left, panel.y + pad, inner),
    );
  }

  if scene.view.picker.is_empty() {
    text.draw_line(
      canvas,
      &scene.theme.login.session.empty,
      Style::new(base, scene.theme.error_on(scene.background_color)),
      Area::new(left, panel.y + pad + header_h, inner),
    );
    return;
  }

  // Keep the selected row visible without needing a scrollbar. With more than
  // ten sessions the window slides, and the ends simply stay put.
  let first = scene
    .view
    .picker_selected
    .saturating_sub(rows.saturating_sub(1));
  let mut row_y = panel.y + pad + header_h;
  for (offset, name) in scene.view.picker.iter().enumerate().skip(first).take(rows) {
    let row = Rect::new(left, row_y, inner, row_h);
    if offset == scene.view.picker_selected {
      canvas.fill(row, accent.with_alpha(45), base * 0.45);
      canvas.stroke(row, accent.with_alpha(80), base * 0.45, 1.0);
    }
    text.draw_line(
      canvas,
      name,
      Style::new(
        base,
        if offset == scene.view.picker_selected {
          primary
        } else {
          muted
        },
      ),
      Area::new(
        left + base * 0.6,
        row_y + (row_h - base * 1.35) / 2.0,
        inner - base * 1.2,
      )
      .clip(row),
    );
    row_y += row_h;
  }
}

fn draw_badge(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  scale: f32,
  accent: Rgb,
) {
  let login = &scene.theme.login;
  if !login.show_dry_run_badge || login.dry_run_badge.is_empty() {
    return;
  }
  let size = scene.theme.font_size * 0.8 * scale;
  let label = login.dry_run_badge.as_str();
  let width = text.line_width(label, size);
  let pad = size * 0.6;
  let margin = size * 1.1;
  let radius = size * 0.4;
  let rect = Rect::new(margin, margin, width + pad * 2.0, size * 1.6 + pad);
  glass.panel(canvas, rect, radius, scene.theme, 1.0);
  text.draw_line(
    canvas,
    label,
    Style::new(size, accent),
    Area::new(rect.x + pad, rect.y + (rect.h - size * 1.35) / 2.0, rect.w),
  );
}

fn draw_warnings(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
  error: Rgb,
) {
  let mut messages = Vec::new();
  if let Some(warning) = &scene.view.warning {
    messages.push(warning.as_str());
  }
  if let Some(warning) = scene.asset_warning {
    messages.push(warning);
  }
  if messages.is_empty() {
    return;
  }

  let size = scene.theme.font_size * 0.82 * scale;
  let margin = scene.theme.font_size * 1.9 * scale;
  let width = (screen.0 - margin * 2.0).max(1.0);
  let mut y = screen.1 - margin * 0.5;
  for message in messages.iter().rev() {
    let (_, height) = text.measure(message, size, width, true);
    y -= height;
    text.draw(
      canvas,
      message,
      Style::new(size, error.with_alpha(230)),
      Area::new(margin, y, width).align(Align::Center),
    );
    y -= size * 0.3;
  }
}

/// A stable id for a widget's drag offset. Built-ins use their kind so
/// disabling the clock does not hand the media widget the clock's saved spot.
fn widget_id(plugin: &Plugin, index: usize, widget: &Widget) -> String {
  if plugin.builtin {
    match widget {
      Widget::Clock { .. } => return "clock".to_string(),
      Widget::Media { .. } => return "media".to_string(),
      _ => {}
    }
  }
  format!("{}:{index}", plugin.id)
}

#[allow(clippy::too_many_arguments)]
fn draw_plugins(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  regions: &mut Vec<HitRegion>,
) {
  for plugin in scene.plugins {
    for (index, widget) in plugin.widgets.iter().enumerate() {
      if !widget.placement().visible_on(scene.monitor) {
        continue;
      }
      let placement = widget.placement();
      let id = widget_id(plugin, index, widget);
      let drag = scene.drag.offset(&id);
      let offset = (
        (placement.offset.0 as f32 * scale).round() as i32 + (drag.0 * scale).round() as i32,
        (placement.offset.1 as f32 * scale).round() as i32 + (drag.1 * scale).round() as i32,
      );
      let rotation = placement.rotation;
      let draggable = widget.draggable();
      match widget {
        Widget::Text {
          text: content,
          size,
          color,
          max_width,
          ..
        } => {
          let size = *size * scale;
          let limit = max_width.map_or((screen.0 - 16.0 * scale).max(1.0), |width| width * scale);
          let (width, height) = text.measure(content, size, limit, max_width.is_some());
          let (x, y) = placement.anchor.place(screen, (width, height), offset);
          let rect = Rect::new(x, y, width, height);
          paint_rotated(canvas, rect, rect.center(), rotation, |layer| {
            text.draw(
              layer,
              content,
              Style::new(size, *color),
              Area::new(x, y, limit),
            );
          });
          regions.push(HitRegion::rotated(id.clone(), rect, draggable, rotation));
          draw_drag_highlight(canvas, rect, &id, scene, accent, scale);
        }
        Widget::Clock {
          spec, draggable, ..
        } => {
          let color = spec.color.unwrap_or(primary);
          let date_color = spec.date_color.unwrap_or(muted);
          if let Some(bounds) = clock::bounds(
            text,
            spec,
            placement.anchor,
            offset,
            screen,
            scale,
            scene.now,
          ) {
            paint_rotated(canvas, bounds, bounds.center(), rotation, |layer| {
              draw_scrim(layer, bounds, scene.theme, scale);
              clock::draw(
                layer,
                text,
                spec,
                placement.anchor,
                offset,
                screen,
                scale,
                color,
                date_color,
                scene.now,
              );
            });
            regions.push(HitRegion::rotated(id.clone(), bounds, *draggable, rotation));
            draw_drag_highlight(canvas, bounds, &id, scene, accent, scale);
          }
        }
        Widget::Image { image, width, .. } => {
          let width = *width * scale;
          let height = if image.width() == 0 {
            0.0
          } else {
            width * image.height() as f32 / image.width() as f32
          };
          let (x, y) = placement.anchor.place(screen, (width, height), offset);
          let rect = Rect::new(x, y, width, height);
          paint_rotated(canvas, rect, rect.center(), rotation, |layer| {
            layer.image_in(image, rect);
          });
          regions.push(HitRegion::rotated(id.clone(), rect, draggable, rotation));
          draw_drag_highlight(canvas, rect, &id, scene, accent, scale);
        }
        Widget::Media {
          state,
          size,
          show_art,
          color,
          show_before_login,
          draggable,
          ..
        } => {
          let snapshot = state.snapshot();
          let bounds = media_bounds(
            text,
            &snapshot,
            placement.anchor,
            offset,
            screen,
            scale,
            *size,
            *show_art,
            *show_before_login,
            scene.view.authenticated,
          );
          paint_rotated(canvas, bounds, bounds.center(), rotation, |layer| {
            draw_media(
              layer,
              text,
              scene,
              &snapshot,
              placement.anchor,
              offset,
              screen,
              scale,
              *size,
              *show_art,
              *show_before_login,
              color.unwrap_or(primary),
              muted,
              accent,
            );
          });
          regions.push(HitRegion::rotated(id.clone(), bounds, *draggable, rotation));
          draw_drag_highlight(canvas, bounds, &id, scene, accent, scale);
        }
      }
    }
  }
}

/// The rectangle a media widget will occupy. Split from `draw_media` so the
/// rotation layer can size itself before painting.
#[allow(clippy::too_many_arguments)]
fn media_bounds(
  text: &mut TextEngine,
  snapshot: &MediaSnapshot,
  anchor: crate::layout::Anchor,
  offset: (i32, i32),
  screen: (f32, f32),
  scale: f32,
  logical_size: f32,
  show_art: bool,
  show_before_login: bool,
  authenticated: bool,
) -> Rect {
  let size = logical_size * scale;
  if (!authenticated && !show_before_login) || !snapshot.available {
    let label = "No media";
    let (width, height) = text.measure(label, size * 0.85, 320.0 * scale, false);
    let (x, y) = anchor.place(screen, (width, height), offset);
    return Rect::new(x, y, width, height);
  }
  let art_size = if show_art && snapshot.art.is_some() {
    size * 3.0
  } else {
    0.0
  };
  let width = (300.0 * scale).max(art_size + size * 12.0);
  let height = art_size.max(size * 2.8);
  let (x, y) = anchor.place(screen, (width, height), offset);
  Rect::new(x, y, width, height)
}

#[allow(clippy::too_many_arguments)]
fn draw_media(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  snapshot: &MediaSnapshot,
  anchor: crate::layout::Anchor,
  offset: (i32, i32),
  screen: (f32, f32),
  scale: f32,
  logical_size: f32,
  show_art: bool,
  show_before_login: bool,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
) -> Rect {
  let size = logical_size * scale;
  let rect = media_bounds(
    text,
    snapshot,
    anchor,
    offset,
    screen,
    scale,
    logical_size,
    show_art,
    show_before_login,
    scene.view.authenticated,
  );
  // A greeter normally runs before the user's session exists. `show_before_login`
  // lets a desktop-session test or a single-user machine display the last
  // track; shared machines can turn it off and keep metadata off the pixels.
  if (!scene.view.authenticated && !show_before_login) || !snapshot.available {
    let label = "No media";
    draw_scrim(canvas, rect, scene.theme, scale);
    text.draw(
      canvas,
      label,
      Style::new(size * 0.85, muted.with_alpha(160)),
      Area::new(rect.x, rect.y, rect.w.max(1.0)),
    );
    return rect;
  }

  let (x, y) = (rect.x, rect.y);
  let width = rect.w;
  let art_size = if show_art && snapshot.art.is_some() {
    size * 3.0
  } else {
    0.0
  };
  draw_scrim(canvas, rect, scene.theme, scale);
  let mut text_x = x;
  if let Some(art) = &snapshot.art
    && show_art
  {
    canvas.image_in(art, Rect::new(x, y, art_size, art_size));
    text_x += art_size + size * 0.7;
  }

  text.draw_line(
    canvas,
    &snapshot.title,
    Style::new(
      size,
      primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
    ),
    Area::new(text_x, y, (width - (text_x - x)).max(1.0)),
  );
  text.draw_line(
    canvas,
    &snapshot.artist,
    Style::new(size * 0.85, muted),
    Area::new(text_x, y + size * 1.25, (width - (text_x - x)).max(1.0)),
  );

  if snapshot.stale {
    text.draw_line(
      canvas,
      "Last played",
      Style::new(size * 0.7, muted.with_alpha(150)),
      Area::new(text_x, y + size * 2.25, (width - (text_x - x)).max(1.0)),
    );
  } else if !snapshot.length.is_zero() {
    let fraction =
      (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0);
    let bar = Rect::new(
      text_x,
      y + size * 2.35,
      (width - (text_x - x)).max(1.0),
      2.0 * scale,
    );
    canvas.fill(bar, muted.with_alpha(50), 1.0);
    canvas.fill(
      Rect::new(bar.x, bar.y, bar.w * fraction, bar.h),
      accent,
      1.0,
    );
  }
  rect
}

/// A readable backplate for a widget that sits directly on the wallpaper.
///
/// The card has its own glass panel, but the clock and media widgets do not.
/// White text over a bright photo is a legibility bug, so each built-in widget
/// gets a translucent theme-tinted pill. It uses the same tint token as the
/// card, which keeps the design language consistent across presets.
fn draw_scrim(canvas: &mut Canvas, bounds: Rect, theme: &Theme, scale: f32) {
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
fn scaled_offset(offset: (i32, i32), scale: f32) -> (i32, i32) {
  (
    (offset.0 as f32 * scale).round() as i32,
    (offset.1 as f32 * scale).round() as i32,
  )
}

#[cfg(test)]
mod tests {
  use std::time::Instant;

  use super::{Scene, draw_media};
  use crate::color::Rgb;
  use crate::config::Config;
  use crate::layout::Anchor;
  use crate::ui::canvas::Canvas;
  use crate::ui::drag::Drag;
  use crate::ui::media::MediaSnapshot;
  use crate::ui::motion::MotionFrame;
  use crate::ui::text::TextEngine;
  use crate::ui::theme::Theme;
  use crate::ui::view::View;

  /// Paints one media widget with a deliberate title and artist. Comparing
  /// the pixels of two different titles is how we prove the renderer either
  /// used them or ignored them completely.
  fn media_pixels(
    authenticated: bool,
    show_before_login: bool,
    title: &str,
    artist: &str,
  ) -> Vec<u8> {
    let config = Config::default();
    let theme = Theme::from(&config);
    let view = View {
      authenticated,
      ..View::default()
    };
    let now = jiff::Zoned::now();
    let drag = Drag::empty();
    let frame_now = Instant::now();
    let scene = Scene {
      view: &view,
      theme: &theme,
      plugins: &[],
      now: &now,
      started: frame_now,
      frame_now,
      scale: 1.0,
      motion: MotionFrame::settled(),
      monitor: None,
      background_color: config.background.color,
      asset_warning: None,
      drag: &drag,
    };
    let snapshot = MediaSnapshot {
      available: true,
      title: title.to_string(),
      artist: artist.to_string(),
      playing: true,
      ..MediaSnapshot::default()
    };
    let mut canvas = Canvas::new(480, 160).unwrap();
    let mut text = TextEngine::new(&config.ui.font);
    draw_media(
      &mut canvas,
      &mut text,
      &scene,
      &snapshot,
      Anchor::TopLeft,
      (0, 0),
      (480.0, 160.0),
      1.0,
      16.0,
      false,
      show_before_login,
      Rgb::solid(255, 255, 255),
      Rgb::solid(200, 200, 200),
      Rgb::solid(80, 120, 255),
    );
    canvas.pixmap().data().to_vec()
  }

  #[test]
  fn media_metadata_follows_the_privacy_setting() {
    // Shared machines can keep metadata off the login pixels. Single-user
    // machines can show the last track before login, which is the whole point
    // of the media widget in a desktop test or a `--dry-run` session.
    let first = media_pixels(false, false, "Secret Song", "Private Artist");
    let second = media_pixels(false, false, "Another Track", "Somebody Else");
    assert_eq!(
      first, second,
      "pre-auth media rendering ignored show_before_login = false"
    );

    let first = media_pixels(false, true, "Secret Song", "Private Artist");
    let second = media_pixels(false, true, "Another Track", "Somebody Else");
    assert_ne!(
      first, second,
      "pre-auth media rendering ignored show_before_login = true"
    );

    let first = media_pixels(true, false, "Secret Song", "Private Artist");
    let second = media_pixels(true, false, "Another Track", "Somebody Else");
    assert_ne!(
      first, second,
      "authenticated media rendering ignored the track metadata"
    );
  }
}
