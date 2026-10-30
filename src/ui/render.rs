//! Turns a [`View`] into pixels.
//!
//! There is no retained widget tree. Every call paints a whole frame:
//! background, blurred glass cache, plugin widgets, card, overlays. That is
//! affordable because the window only redraws on input, a visible clock
//! change, an auth event, or an animation deadline.
//!
//! Plugin widgets default to painting before any core UI. A greeter cannot let
//! a third-party widget cover the password field, so the form always paints
//! last: a plugin may opt into the front layer, but only between decor and the
//! form, never over it. That ordering is a security property, not a style
//! choice.

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use cosmic_text::Align;

use crate::color::Rgb;
use crate::config::{
  ButtonConfig, ButtonEffect, CaretStyle, DecorSample, HoverReveal, LoginLayout, MediaArt,
  MediaLayout, MediaLook, MediaProgress, PlateStyle, SurfaceStyle, WidgetLayer, WidgetLook,
  WidgetPlate, WidgetState,
};
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
  /// Pointer position in physical pixels when it is inside the window. `None`
  /// in screenshots and whenever the pointer has left, which keeps parallax at
  /// its neutral offset instead of guessing a position.
  pub cursor: Option<(f32, f32)>,
  /// Window size in physical pixels. Parallax normalises the pointer against
  /// this so the effect is the same on every display size.
  pub screen: (f32, f32),
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
  draw_scanlines(canvas, scene);
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

/// Paints the CRT scanline overlay straight onto the wallpaper.
///
/// The CPU path calls this right after the wallpaper paint and before the glass
/// cache. The GPU path calls it on the transparent UI layer before the form, so
/// both presenters put the texture over the wallpaper and under the login UI.
/// The static rows are cheap enough to repaint every frame. Only the sweep band
/// moves, and reduced motion or low-power mode drops it while leaving the rows
/// exactly where they were.
pub(crate) fn draw_scanlines(canvas: &mut Canvas, scene: &Scene<'_>) {
  let theme = scene.theme;
  if theme.scanlines <= 0.0 {
    return;
  }
  let width = canvas.width() as f32;
  let height = canvas.height();
  let gap = theme.scanline_gap.max(1);
  let row_color = theme.text.with_alpha((theme.scanlines * 26.0) as u8);
  for row in 0..scanline_rows(gap, height) {
    let y = (row * gap) as f32;
    canvas.fill(Rect::new(0.0, y, width, 1.0), row_color, 0.0);
  }
  if theme.motion.reduced || theme.low_power {
    return;
  }
  // One cycle carries the band from just above the screen to just below it, so
  // the wrap happens where nobody can see it. We compute the period in seconds
  // and guard against zero even though validation already enforces a minimum.
  let period = (theme.scanline_ms / 1000.0).max(0.001);
  let elapsed = scene
    .frame_now
    .saturating_duration_since(scene.started)
    .as_secs_f32();
  let travel = height as f32 + 120.0;
  let y = (elapsed / period) % 1.0 * travel - 120.0;
  canvas.fill(
    Rect::new(0.0, y, width, 120.0),
    theme.accent.with_alpha((theme.scanlines * 18.0) as u8),
    0.0,
  );
}

/// Number of horizontal rows the static scanline pass paints.
///
/// A zero gap is clamped to one so a hand-built `Theme` (or a test) cannot
/// divide by zero. The config validator rejects zero with an issue and falls
/// back to the default, so this is only a safety net.
fn scanline_rows(gap: u32, height: u32) -> u32 {
  height.div_ceil(gap.max(1))
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
    WidgetFrame::screen(screen, scale),
    primary,
    muted,
    accent,
    WidgetLayer::Behind,
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

/// Card geometry and colours shared by the chrome and form passes. Keeping the
/// two passes separate is what lets a front widget paint between the decor and
/// the form without rotating with the card.
struct CardPaint<'a> {
  card: Rect,
  card_scale: f32,
  metrics: &'a Metrics,
  primary: Rgb,
  muted: Rgb,
  error: Rgb,
  accent: Rgb,
  focus: Rgb,
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
    draw_card_chrome(canvas, text, glass, scene, &paint, regions);
    draw_front_widgets(canvas, text, scene, &paint, regions);
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
    draw_card_chrome(canvas, text, glass, scene, &paint, regions);
    draw_front_widgets(canvas, text, scene, &paint, regions);
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

  draw_front_widgets(canvas, text, scene, &paint, regions);

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
fn draw_front_widgets(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  paint: &CardPaint<'_>,
  regions: &mut Vec<HitRegion>,
) {
  draw_plugins(
    canvas,
    text,
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
fn draw_card_chrome(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  paint: &CardPaint<'_>,
  regions: &mut Vec<HitRegion>,
) {
  let login = &scene.theme.login;
  let hero = login.layout == LoginLayout::Hero;
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
  draw_login_decor(canvas, text, scene, card, card_scale);
}

/// Paints the interactive form: fields, button, status, session row, hints, and
/// the card's drag highlight. A front widget paints underneath this pass, so
/// the form always wins the top layer.
fn draw_card_form(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  paint: &CardPaint<'_>,
  regions: &mut Vec<HitRegion>,
) {
  let view = scene.view;
  let login = &scene.theme.login;
  let hero = login.layout == LoginLayout::Hero;
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

/// Pointer-driven parallax offset for one decor item or built-in widget.
///
/// The result is in logical pixels unless `scale` is non-zero, in which case it
/// also carries the output scale for callers that paint in physical pixels.
/// `depth` is the per-item strength and `[motion].parallax` is the global
/// multiplier. Either one at zero disables the effect, and a missing pointer or
/// a zero-sized screen returns zero so screenshots and headless runs never
/// invent a cursor position.
fn parallax_offset(scene: &Scene<'_>, depth: f32, scale: f32) -> (f32, f32) {
  let Some(cursor) = scene.cursor else {
    return (0.0, 0.0);
  };
  let strength = scene.theme.motion.parallax * depth;
  if strength <= 0.0 || scene.screen.0 <= 0.0 || scene.screen.1 <= 0.0 {
    return (0.0, 0.0);
  }
  let nx = (cursor.0 / scene.screen.0 - 0.5) * 2.0;
  let ny = (cursor.1 / scene.screen.1 - 0.5) * 2.0;
  (-nx * strength * 28.0 * scale, -ny * strength * 20.0 * scale)
}

/// Whether the glitch window is open at `elapsed`.
///
/// The window is the first 8 percent of each `glitch_ms` period. Text changes
/// for a short burst and then rests, which reads as a corrupted signal instead
/// of a constant shimmer. A zero strength or period keeps it closed.
fn glitch_window(elapsed: f32, glitch: f32, glitch_ms: f32) -> bool {
  if glitch <= 0.0 || glitch_ms <= 0.0 {
    return false;
  }
  (elapsed / (glitch_ms / 1000.0)) % 1.0 < 0.08
}

/// One pseudo-random glyph for the scramble effect.
///
/// We deliberately do not pull in a random number generator. A tiny integer
/// hash keyed by the character index and the current tick is deterministic
/// across runs, so the same frame always looks the same, while a new tick
/// reshuffles the tail. That makes screenshots and tests reproducible.
fn scramble_glyph(index: usize, tick: u64) -> char {
  const GLYPHS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#$%&";
  let mut hash =
    (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ tick.wrapping_mul(0xBF58_476D_1CE4_E5B9);
  hash ^= hash >> 33;
  hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
  hash ^= hash >> 33;
  GLYPHS[(hash % GLYPHS.len() as u64) as usize] as char
}

/// Applies the scramble reveal to one run.
///
/// Characters become real from the head as `reveal` grows. Every character has
/// a threshold of `(index + 1) / count`, so a reveal of 0.5 keeps the first
/// half and replaces the rest. Whitespace is preserved verbatim: scrambling a
/// space into a glyph would change the line width and make the text jump.
fn scramble_run(run: &str, reveal: f32, tick: u64) -> String {
  let chars: Vec<char> = run.chars().collect();
  let count = chars.len();
  if count == 0 {
    return String::new();
  }
  let mut out = String::with_capacity(run.len());
  for (index, ch) in chars.into_iter().enumerate() {
    let threshold = (index + 1) as f32 / count as f32;
    if ch.is_whitespace() || reveal >= threshold {
      out.push(ch);
    } else {
      out.push(scramble_glyph(index, tick));
    }
  }
  out
}

/// Computes the visible scramble run from the motion clock.
///
/// At `elapsed = 0` the final text is returned unchanged. That is the one
/// exception to "start at the beginning": a screenshot has no time axis, so
/// showing a wall of noise would make the captured frame useless. Live, the
/// effect resolves over `scramble_ms`.
fn scramble_text(run: &str, elapsed: f32, scramble_ms: f32, tick: u64) -> String {
  if elapsed <= 0.0 || scramble_ms <= 0.0 {
    return run.to_string();
  }
  let reveal = (elapsed * 1000.0 / scramble_ms).clamp(0.0, 1.0);
  scramble_run(run, reveal, tick)
}

/// Draws a text run one character at a time with a sine wave per glyph.
///
/// Each character is measured and advanced independently, which is what lets
/// the glyphs ride different vertical offsets without changing the run width.
/// We ignore `stroke` here on purpose: the outline path draws eight copies per
/// character, and eight outline passes for every glyph is a lot of pixels for
/// an effect that already moves. A waving fill reads clearly without it.
/// One wavy text run. Bundling the shaping and wave inputs keeps the helper
/// readable; passing eleven positional arguments made every call site a game
/// of remembering which float was which.
struct WaveText<'a> {
  run: &'a str,
  style: Style,
  area: Area,
  size: f32,
  weight: cosmic_text::Weight,
  tracking: f32,
  amplitude: f32,
  phase: f32,
  step: f32,
}

fn draw_wave_text(text: &mut TextEngine, layer: &mut Canvas, wave: WaveText<'_>) {
  let WaveText {
    run,
    style,
    area,
    size,
    weight,
    tracking,
    amplitude,
    phase,
    step,
  } = wave;
  let mut x = area.x;
  for (index, ch) in run.chars().enumerate() {
    let mut buffer = [0u8; 4];
    let glyph = ch.encode_utf8(&mut buffer);
    let (char_w, _) = text.measure_with(glyph, size, f32::MAX / 4.0, false, weight, tracking);
    let y = (TAU * (phase + index as f32 * step)).sin() * amplitude;
    let mut char_area = Area::new(x, area.y + y, char_w.max(1.0));
    char_area.clip = area.clip;
    text.draw_line(layer, glyph, style, char_area);
    x += char_w;
  }
}

/// Draws the configured decor inside the card. Images are already decoded, so
/// this only card_scales them to their display width. Items paint in list
/// order, so a later item covers an earlier one. The form paints afterwards,
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
    // Parallax is a logical offset, so it joins the configured offset before
    // the card scale is applied. Passing 1.0 here keeps the helper from
    // painting the display scale twice.
    let parallax = parallax_offset(scene, item.motion.parallax_depth, 1.0);
    let offset = (
      ((item.offset.0 as f32 + sample.offset.0 + parallax.0) * card_scale).round() as i32,
      ((item.offset.1 as f32 + sample.offset.1 + parallax.1) * card_scale).round() as i32,
    );
    if item.panel {
      // Typer and ticker only have a text run to work on, so a panel simply
      // ignores them and stays static. Blink is different: it gates the whole
      // plate, which is how a theme gets a radar blip.
      if !sample.blink_on {
        continue;
      }
      let width = item.width * card_scale;
      let height = item.height * card_scale;
      if width <= 0.0 || height <= 0.0 {
        continue;
      }
      let (x, y) = item.anchor.place((card.w, card.h), (width, height), offset);
      let rect = Rect::new(card.x + x, card.y + y, width, height);
      paint_transformed(
        canvas,
        rect,
        rect.center(),
        angle,
        sample.scale,
        opacity,
        |layer, _| {
          let radius = item.radius * card_scale;
          // Use the same primitive as a field background. A solid fill skips
          // the shader entirely, which keeps the common plate cheap.
          if let Some(end) = item.color_end {
            layer.linear_gradient_stops(
              rect,
              radius,
              item.gradient_angle,
              &[(0.0, item.color), (1.0, end)],
            );
          } else {
            layer.fill(rect, item.color, radius);
          }
          if let Some(border) = item.border {
            layer.stroke(rect, border, radius, 1.0);
          }
        },
      );
    } else if let Some(image) = &item.image {
      // Blink is a hard visibility gate for images too, so a chip or cursor
      // can strobe without fading. Same rule as the panel and text branches.
      if !sample.blink_on {
        continue;
      }
      let width = item.width * card_scale;
      if width <= 0.0 || image.width() == 0 {
        continue;
      }
      let height = width * image.height() as f32 / image.width() as f32;
      let (x, y) = item.anchor.place((card.w, card.h), (width, height), offset);
      let rect = Rect::new(card.x + x, card.y + y, width, height);
      canvas.image_transformed(image, rect, angle, sample.scale, opacity);
    } else {
      // Blink is a hard visibility gate, not a fade: a terminal cursor is
      // either there or it is not. Do this before any measuring so a hidden
      // item costs nothing and cannot leave a partial paint behind.
      if !sample.blink_on {
        continue;
      }
      let size = item.size * card_scale;
      let style = if item.bold {
        Style::new(size, item.color).bold()
      } else {
        Style::new(size, item.color)
      }
      .tracking(item.tracking);
      if item.motion.ticker_ms > 0.0 && item.motion.ticker_width > 0 {
        // A ticker repeats a whole run at a fixed cadence, so it keeps the
        // fill style only. Stroke is deliberately ignored for tickered items.
        // The strip is a fixed window placed like a normal text run. We build
        // one tile per text run plus the configured gap, then repeat the tile
        // far enough to cover the window at the current scroll offset. The
        // tile keeps its exact measured width so the wrap at `loop_frac = 0`
        // lands on the same pixels as the previous frame.
        let strip_w = (item.motion.ticker_width as f32 * card_scale).max(1.0);
        // Measure with the style we paint with. The default measurement is
        // normal-weight and untracked, so a bold or letter-spaced ticker would
        // come back narrower than the glyphs and adjacent tiles would collide.
        let (tile_w, height) = text.measure_with(
          &item.text,
          size,
          f32::MAX / 4.0,
          false,
          style.weight,
          style.tracking,
        );
        let tile = (tile_w + item.motion.ticker_gap as f32 * card_scale).max(1.0);
        let (x, y) = item
          .anchor
          .place((card.w, card.h), (strip_w, height), offset);
        let strip = Rect::new(card.x + x, card.y + y, strip_w, height);
        let scroll = sample.loop_frac * tile;
        let tiles = ((strip_w + scroll) / tile).ceil() as usize;
        paint_transformed(
          canvas,
          strip,
          strip.center(),
          angle,
          sample.scale,
          opacity,
          |layer, _| {
            for k in 0..=tiles {
              let tile_x = strip.x + (k as f32) * tile - scroll;
              text.draw_line(
                layer,
                &item.text,
                style,
                Area::new(tile_x, strip.y, tile_w.max(1.0)).clip(strip),
              );
            }
          },
        );
      } else {
        // The scramble reveal wins over the typewriter on the same item, and
        // a ticker would have taken the branch above. At `elapsed = 0` the
        // scramble helper returns the full run so a screenshot stays readable
        // instead of showing a wall of noise.
        //
        // The typewriter reveals whole characters, never partial glyphs.
        // `round` means the last character appears once we are past its
        // midpoint, which keeps the cadence even for short strings. A
        // tickered item never reaches this branch because a marquee already
        // shows the run.
        let shown = if item.motion.scramble_ms > 0.0 {
          scramble_text(
            &item.text,
            sample.elapsed,
            item.motion.scramble_ms,
            (sample.elapsed * 12.0) as u64,
          )
        } else if item.motion.typer_ms > 0.0 {
          let count = item.text.chars().count();
          let visible = (sample.reveal * count as f32).round() as usize;
          if visible == 0 {
            continue;
          }
          item.text.chars().take(visible).collect::<String>()
        } else {
          item.text.clone()
        };
        // Same reasoning as the ticker: anchor the static and typer text by
        // its painted width, not the default-style width. The wave uses the
        // same box, so the item does not jump when it starts moving.
        let (width, height) = text.measure_with(
          &shown,
          size,
          card.w.max(1.0),
          false,
          style.weight,
          style.tracking,
        );
        let (x, y) = item
          .anchor
          .place((card.w, card.h), (width.max(1.0), height), offset);
        let rect = Rect::new(card.x + x, card.y + y, width.max(1.0), height);
        let wave_on = item.motion.wave > 0.0;
        let wave_amp = if wave_on {
          item.motion.wave * card_scale
        } else {
          0.0
        };
        let glitch_on = glitch_window(sample.elapsed, item.motion.glitch, item.motion.glitch_ms);
        let glitch_dx = if item.motion.glitch > 0.0 {
          (2.0 + 4.0 * item.motion.glitch) * card_scale
        } else {
          0.0
        };
        // The outline sits outside the fill bounds and the wave moves glyphs
        // vertically, so grow the layer by the largest excursion. Without this
        // the top and bottom of the wave would be clipped.
        let stroke_radius = if item.stroke.is_some() && !wave_on {
          item.stroke_width * card_scale
        } else {
          0.0
        };
        let pad = stroke_radius.max(wave_amp).max(glitch_dx);
        let bounds = if pad > 0.0 { rect.inset(-pad) } else { rect };
        let wave_phase = if item.motion.wave_ms > 0.0 {
          (sample.elapsed / (item.motion.wave_ms / 1000.0)) % 1.0
        } else {
          0.0
        };
        paint_transformed(
          canvas,
          bounds,
          rect.center(),
          angle,
          sample.scale,
          opacity,
          |layer, _| {
            // Glitch is text-only, and these two coloured copies sit behind
            // the real run. The offsets are deterministic, so the effect does
            // not flicker differently on every repaint.
            if glitch_on {
              for (color, shift) in [
                (Rgb::solid(255, 64, 64), -glitch_dx),
                (Rgb::solid(64, 220, 255), glitch_dx),
              ] {
                let copy_style = Style {
                  color: color.with_alpha(70),
                  ..style
                };
                let copy_area = Area::new(rect.x + shift, rect.y, width.max(1.0)).clip(bounds);
                if wave_on {
                  draw_wave_text(
                    text,
                    layer,
                    WaveText {
                      run: &shown,
                      style: copy_style,
                      area: copy_area,
                      size,
                      weight: style.weight,
                      tracking: style.tracking,
                      amplitude: wave_amp,
                      phase: wave_phase,
                      step: item.motion.wave_step,
                    },
                  );
                } else {
                  text.draw_line(layer, &shown, copy_style, copy_area);
                }
              }
            }
            if wave_on {
              // Wave ignores `stroke`: eight outline passes per character is
              // not worth the pixels, and the moving fill already reads well.
              draw_wave_text(
                text,
                layer,
                WaveText {
                  run: &shown,
                  style,
                  area: Area::new(rect.x, rect.y, width.max(1.0)).clip(bounds),
                  size,
                  weight: style.weight,
                  tracking: style.tracking,
                  amplitude: wave_amp,
                  phase: wave_phase,
                  step: item.motion.wave_step,
                },
              );
              return;
            }
            if let Some(stroke) = item.stroke {
              // Eight copies on a 45-degree ring is enough for a clean
              // outline without a real glyph-stroking pass. The fill goes on
              // top afterwards, so only the outer half of each copy shows.
              let stroke_style = Style {
                color: stroke,
                ..style
              };
              let step = std::f32::consts::FRAC_PI_4;
              for index in 0..8 {
                let radians = step * index as f32;
                let (dx, dy) = (radians.cos() * stroke_radius, radians.sin() * stroke_radius);
                text.draw_line(
                  layer,
                  &shown,
                  stroke_style,
                  Area::new(rect.x + dx, rect.y + dy, width.max(1.0)),
                );
              }
            }
            text.draw_line(
              layer,
              &shown,
              style,
              Area::new(rect.x, rect.y, width.max(1.0)),
            );
          },
        );
      }
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
    let caret_color = field.config.caret.unwrap_or(field.accent);
    let caret_width = if field.config.caret_width > 0.0 {
      field.config.caret_width * scale
    } else {
      match field.config.caret_style {
        CaretStyle::Bar => (base * 0.12).max(1.0),
        // A block stands in for one character cell, so it is much wider than
        // a bar. The floor of two pixels keeps it visible on tiny fields.
        CaretStyle::Block => (base * 0.55).max(2.0),
      }
    };
    // The whole caret has to fit inside the field, not just its left edge.
    // A block is wide enough that the old left-edge check would let it paint
    // over the border, so use the right edge of the caret here.
    if caret_x + caret_width <= rect.right() - inset * 0.5 {
      let caret_height = (ink_bottom - ink_top).max(base * 0.8) + base * 0.1;
      let caret_radius = match field.config.caret_style {
        CaretStyle::Bar => radius,
        CaretStyle::Block => 0.0,
      };
      canvas.fill(
        Rect::new(
          caret_x,
          rect.center().1 - caret_height / 2.0,
          caret_width,
          caret_height,
        ),
        caret_color,
        caret_radius,
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

/// True when at least one widget asked for the front layer and is visible on
/// this output. The rotated card uses this to avoid allocating a second
/// full-card layer when no widget will land between decor and form.
fn has_front_widgets(scene: &Scene<'_>) -> bool {
  scene.plugins.iter().any(|plugin| {
    plugin.widgets.iter().any(|widget| {
      widget_layer(scene, plugin, widget) == WidgetLayer::Front
        && widget.placement().visible_on(scene.monitor)
    })
  })
}

/// Resolves the paint layer for one widget. The built-in clock and media use
/// their `[clock]`/`[media]` layer so a theme can put them inside a HUD bar.
/// Every plugin widget carries its own manifest layer, which defaults to
/// `Behind`; the form still paints after both layers, so a plugin can never
/// cover an input.
fn widget_layer(scene: &Scene<'_>, plugin: &Plugin, widget: &Widget) -> WidgetLayer {
  if !plugin.builtin {
    return widget.layer();
  }
  match widget {
    Widget::Clock { .. } => scene.theme.clock_layer,
    Widget::Media { .. } => scene.theme.media_layer,
    _ => WidgetLayer::Behind,
  }
}

/// The coordinate frame a widget is placed in.
///
/// Behind widgets keep the historical screen-space frame: the screen's
/// top-left is the origin and the output scale sizes the offset and the font.
/// Front widgets on a built-in clock or media slot join the card frame the
/// same way decor does: the card's top-left is the origin, the card extent is
/// the anchor space, and `card_scale` scales both the offset and the size
/// base. That is what makes a HUD pill shrink with a hero card on a smaller
/// display instead of overflowing a decor bar that already shrank.
#[derive(Debug, Clone, Copy)]
struct WidgetFrame {
  origin: (f32, f32),
  extent: (f32, f32),
  scale: f32,
  card_space: bool,
}

impl WidgetFrame {
  fn screen(screen: (f32, f32), scale: f32) -> Self {
    Self {
      origin: (0.0, 0.0),
      extent: screen,
      scale,
      card_space: false,
    }
  }

  fn card(card: Rect, card_scale: f32) -> Self {
    Self {
      origin: (card.x, card.y),
      extent: (card.w, card.h),
      scale: card_scale,
      card_space: true,
    }
  }

  /// The rectangle the widget anchors inside. Its top-left is already an
  /// absolute screen coordinate, so drawing code adds it to the local anchor
  /// result instead of threading a separate origin.
  fn viewport(self) -> Rect {
    Rect::new(self.origin.0, self.origin.1, self.extent.0, self.extent.1)
  }
}

/// Resolves a manifest offset plus the live drag offset in the frame's units.
///
/// `configured` already includes any motion sample, because the sample is in
/// logical pixels and has to join the offset before the scale multiplication.
/// Card-space widgets round after adding the drag, matching decor, so a
/// subpixel drag cannot drift by a pixel every frame. Screen-space widgets
/// keep the old two-rounding rule, which is what leaves the behind layer
/// byte-for-byte unchanged when no motion is configured.
fn widget_offset(frame: &WidgetFrame, configured: (f32, f32), drag: (f32, f32)) -> (i32, i32) {
  if frame.card_space {
    (
      ((configured.0 + drag.0) * frame.scale).round() as i32,
      ((configured.1 + drag.1) * frame.scale).round() as i32,
    )
  } else {
    (
      (configured.0 * frame.scale).round() as i32 + (drag.0 * frame.scale).round() as i32,
      (configured.1 * frame.scale).round() as i32 + (drag.1 * frame.scale).round() as i32,
    )
  }
}

/// The current login state for widget visibility rules.
///
/// The order matters when more than one flag is set. A request in flight is
/// the most useful thing to react to, an error is next, and typing is only
/// reported when nothing louder is happening. That keeps `show_on` rules
/// predictable without asking a config author to think about combinations.
fn current_widget_state(view: &View) -> WidgetState {
  if view.busy {
    WidgetState::Busy
  } else if view
    .status
    .as_ref()
    .is_some_and(|line| line.tone == Tone::Error)
  {
    WidgetState::Error
  } else if view.authenticated {
    WidgetState::Authenticated
  } else if !view.username.is_empty() || !view.password.is_empty() {
    WidgetState::Typing
  } else {
    WidgetState::Idle
  }
}

/// Whether a state-gated widget should paint this frame. `hide_on` wins over
/// `show_on`; an empty `show_on` means the widget is visible in every state.
fn widget_visible(show_on: &[WidgetState], hide_on: &[WidgetState], view: &View) -> bool {
  let state = current_widget_state(view);
  if hide_on.contains(&state) {
    return false;
  }
  show_on.is_empty() || show_on.contains(&state)
}

/// Seconds of widget motion for this frame. Reduced motion and low-power mode
/// pin it to zero, which is the neutral pose. The clock also uses this for its
/// colon blink, so a screenshot (`started == frame_now`) always captures the
/// lit phase.
fn widget_elapsed(scene: &Scene<'_>) -> f32 {
  if scene.theme.motion.reduced || scene.theme.low_power {
    return 0.0;
  }
  scene
    .frame_now
    .saturating_duration_since(scene.started)
    .as_secs_f32()
}

/// Samples a built-in widget's motion. Reduced motion and low-power mode pin
/// the sample to `elapsed = 0`, which is the neutral pose, so the widget stays
/// still instead of disappearing or jumping.
fn widget_sample(look: Option<&WidgetLook>, scene: &Scene<'_>) -> Option<DecorSample> {
  let look = look?;
  Some(look.motion.sample(widget_elapsed(scene)))
}

#[allow(clippy::too_many_arguments)]
fn draw_plugins(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  frame: WidgetFrame,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  layer: WidgetLayer,
  regions: &mut Vec<HitRegion>,
) {
  let mut next_media = 0usize;
  for plugin in scene.plugins {
    for (index, widget) in plugin.widgets.iter().enumerate() {
      // The window spawns one MediaHandle per media widget in app.media_states()
      // order, which is this same plugin/widget order. The counter has to move
      // before the visible_on check so a widget hidden on this output does not
      // shift every later media widget onto the wrong player.
      let media_slot = if matches!(widget, Widget::Media { .. }) {
        let slot = next_media;
        next_media += 1;
        Some(slot)
      } else {
        None
      };
      if !widget.placement().visible_on(scene.monitor) {
        continue;
      }
      // Each widget is painted exactly once: the behind pass skips front
      // widgets and the front pass skips everything else. The hit region is
      // recorded in the same arm that paints, so both layers keep identical
      // ids and geometry.
      if widget_layer(scene, plugin, widget) != layer {
        continue;
      }
      let placement = widget.placement();
      // Only the built-in clock and media have a resolved look today. Plugin
      // widgets keep the historical fixed scrim and a static transform, so a
      // third-party manifest cannot opt into theme state rules yet.
      let look = if plugin.builtin {
        match widget {
          Widget::Clock { .. } => Some(&scene.theme.clock_look),
          Widget::Media { .. } => Some(&scene.theme.media_look),
          _ => None,
        }
      } else {
        None
      };
      if let Some(look) = look
        && !widget_visible(&look.show_on, &look.hide_on, scene.view)
      {
        continue;
      }
      let sample = widget_sample(look, scene);
      // Blink gates the whole widget, plate included. That is what makes a
      // two-state radar blip possible without a second decor item.
      if sample.is_some_and(|sample| !sample.blink_on) {
        continue;
      }
      // Parallax is logical here; `widget_offset` applies the frame scale
      // afterwards, exactly like the configured offset and the motion sample.
      let parallax = look.map_or((0.0, 0.0), |look| {
        parallax_offset(scene, look.motion.parallax_depth, 1.0)
      });
      let configured = (
        placement.offset.0 as f32 + sample.map_or(0.0, |sample| sample.offset.0) + parallax.0,
        placement.offset.1 as f32 + sample.map_or(0.0, |sample| sample.offset.1) + parallax.1,
      );
      let id = widget_id(plugin, index, widget);
      let drag = scene.drag.offset(&id);
      let offset = widget_offset(&frame, configured, drag);
      let rotation = placement.rotation;
      let angle = rotation + sample.map_or(0.0, |sample| sample.angle);
      let (scale, opacity) = match (look, sample) {
        (Some(look), Some(sample)) => (sample.scale, sample.opacity * look.plate.opacity),
        _ => (1.0, 1.0),
      };
      let plate = look.map(|look| &look.plate);
      let draggable = widget.draggable();
      match widget {
        Widget::Text {
          text: content,
          size,
          color,
          max_width,
          ..
        } => {
          let size = *size * frame.scale;
          let limit = max_width.map_or((frame.extent.0 - 16.0 * frame.scale).max(1.0), |width| {
            width * frame.scale
          });
          let (width, height) = text.measure(content, size, limit, max_width.is_some());
          let (x, y) = placement
            .anchor
            .place(frame.extent, (width, height), offset);
          let (x, y) = (frame.origin.0 + x, frame.origin.1 + y);
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
          draw_drag_highlight(canvas, rect, &id, scene, accent, frame.scale);
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
            frame.viewport(),
            frame.scale,
            scene.now,
          ) {
            paint_transformed(
              canvas,
              bounds,
              bounds.center(),
              angle,
              scale,
              opacity,
              |layer, _| {
                // The plate is drawn before the digits so the whole widget,
                // including its future progress bar, sits on one surface.
                paint_widget_backdrop(layer, bounds, scene.theme, frame.scale, plate);
                clock::draw(
                  layer,
                  text,
                  spec,
                  placement.anchor,
                  offset,
                  frame.viewport(),
                  frame.scale,
                  color,
                  date_color,
                  scene.now,
                  widget_elapsed(scene),
                  plate,
                  scene.theme,
                );
              },
            );
            regions.push(
              HitRegion::rotated(id.clone(), bounds, *draggable, angle)
                .with_drag_scale(frame.scale),
            );
            draw_drag_highlight(canvas, bounds, &id, scene, accent, frame.scale);
          }
        }
        Widget::Image { image, width, .. } => {
          let width = *width * frame.scale;
          let height = if image.width() == 0 {
            0.0
          } else {
            width * image.height() as f32 / image.width() as f32
          };
          let (x, y) = placement
            .anchor
            .place(frame.extent, (width, height), offset);
          let (x, y) = (frame.origin.0 + x, frame.origin.1 + y);
          let rect = Rect::new(x, y, width, height);
          paint_rotated(canvas, rect, rect.center(), rotation, |layer| {
            layer.image_in(image, rect);
          });
          regions.push(HitRegion::rotated(id.clone(), rect, draggable, rotation));
          draw_drag_highlight(canvas, rect, &id, scene, accent, frame.scale);
        }
        Widget::Media {
          state,
          size,
          show_art,
          look: media_look,
          color,
          show_before_login,
          draggable,
          ..
        } => {
          let slot = media_slot.unwrap_or(0);
          let snapshot = state.snapshot();
          let bounds = media_bounds(
            text,
            &snapshot,
            placement.anchor,
            offset,
            frame.viewport(),
            frame.scale,
            *size,
            *show_art,
            *show_before_login,
            scene.view.authenticated,
            *media_look,
          );
          let mut painted = None;
          paint_transformed(
            canvas,
            bounds,
            bounds.center(),
            angle,
            scale,
            opacity,
            |layer, _| {
              painted = Some(draw_media(
                layer,
                text,
                scene,
                slot,
                &snapshot,
                placement.anchor,
                offset,
                frame.viewport(),
                frame.scale,
                *size,
                *show_art,
                *show_before_login,
                color.unwrap_or(primary),
                muted,
                accent,
                plate,
                *media_look,
                angle,
              ));
            },
          );
          // `paint_transformed` skips the closure entirely at zero opacity, so
          // the fallback keeps the widget's own drag region even when it faded
          // out this frame. Controls stay unclickable in that case.
          let draw = painted.unwrap_or(MediaDraw {
            bounds,
            controls: [None; 3],
          });
          regions.push(
            HitRegion::rotated(id.clone(), draw.bounds, *draggable, angle)
              .with_drag_scale(frame.scale),
          );
          // Pushed after the widget region, in the same order the glyphs were
          // drawn. Reverse hit testing then sees a control before the plate.
          for (action, rect) in ["prev", "play", "next"].into_iter().zip(draw.controls) {
            if let Some(rect) = rect {
              regions.push(HitRegion::new(
                format!("media:{slot}:{action}"),
                rect,
                false,
              ));
            }
          }
          draw_drag_highlight(canvas, draw.bounds, &id, scene, accent, frame.scale);
        }
      }
    }
  }
}

/// The album art a layout should paint. `Disc` prefers the circular pixmap
/// masked once in [`crate::ui::media`], but falls back to the square crop when
/// a snapshot was built without one (tests and older cached tracks).
fn media_art(
  snapshot: &MediaSnapshot,
  show_art: bool,
  art: MediaArt,
) -> Option<&tiny_skia::Pixmap> {
  if !show_art {
    return None;
  }
  match art {
    MediaArt::None => None,
    MediaArt::Cover => snapshot.art.as_deref(),
    MediaArt::Disc => snapshot.art_disc.as_deref().or(snapshot.art.as_deref()),
  }
}

/// Height a progress style needs inside its layout. A wave needs vertical room
/// to be visible; a line stays a hairline.
fn progress_height(style: MediaProgress, size: f32, scale: f32) -> f32 {
  match style {
    MediaProgress::Line => 2.0 * scale,
    MediaProgress::Segments => 4.0 * scale,
    MediaProgress::Wave => size * 0.8,
  }
}

/// The rectangle a media widget will occupy. Split from `draw_media` so the
/// rotation layer can size itself before painting. The layout switch lives
/// here and in `draw_media`; keeping the two matches in the same file is what
/// stops the hit region from drifting away from the pixels.
#[allow(clippy::too_many_arguments)]
fn media_bounds(
  text: &mut TextEngine,
  snapshot: &MediaSnapshot,
  anchor: crate::layout::Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  logical_size: f32,
  show_art: bool,
  show_before_login: bool,
  authenticated: bool,
  look: MediaLook,
) -> Rect {
  let size = logical_size * scale;
  let screen = (viewport.w, viewport.h);
  if (!authenticated && !show_before_login) || !snapshot.available {
    let label = "No media";
    let (width, height) = text.measure(label, size * 0.85, 320.0 * scale, false);
    let (x, y) = anchor.place(screen, (width, height), offset);
    return Rect::new(viewport.x + x, viewport.y + y, width, height);
  }
  let art_visible = media_art(snapshot, show_art, look.art).is_some();
  let (width, height) = match look.layout {
    MediaLayout::Card => {
      let art_size = if art_visible { size * 3.0 } else { 0.0 };
      let mut height = art_size.max(size * 2.8);
      if look.controls {
        height = height.max(size * 4.2);
      }
      ((300.0 * scale).max(art_size + size * 12.0), height)
    }
    MediaLayout::Compact => {
      let height = if look.controls {
        size * 3.4
      } else {
        size * 2.6
      };
      (320.0 * scale, height)
    }
    MediaLayout::Vinyl => {
      let disc = if art_visible { size * 4.0 } else { 0.0 };
      let mut height = disc + size * 3.4;
      if look.controls {
        height += size * 1.2;
      }
      ((size * 5.0).max(disc + size * 0.8), height)
    }
  };
  let (x, y) = anchor.place(screen, (width, height), offset);
  Rect::new(viewport.x + x, viewport.y + y, width, height)
}

/// The music progress indicator. None of these styles reads audio samples:
/// the wave is a deterministic sine that only encodes the playback fraction,
/// so it looks alive without claiming to be a real spectrum.
fn draw_progress(
  canvas: &mut Canvas,
  rect: Rect,
  fraction: f32,
  style: MediaProgress,
  accent: Rgb,
  muted: Rgb,
  scale: f32,
) {
  let fraction = fraction.clamp(0.0, 1.0);
  match style {
    MediaProgress::Line => {
      canvas.fill(rect, muted.with_alpha(50), 1.0);
      canvas.fill(
        Rect::new(rect.x, rect.y, rect.w * fraction, rect.h),
        accent,
        1.0,
      );
    }
    MediaProgress::Segments => {
      const BLOCKS: usize = 12;
      let gap = 3.0 * scale;
      let block_width = ((rect.w - gap * (BLOCKS - 1) as f32) / BLOCKS as f32).max(1.0);
      let filled = ((fraction * BLOCKS as f32).round() as usize).min(BLOCKS);
      for index in 0..BLOCKS {
        let x = rect.x + index as f32 * (block_width + gap);
        let color = if index < filled {
          accent
        } else {
          muted.with_alpha(70)
        };
        canvas.fill(
          Rect::new(x, rect.y, block_width, rect.h.max(4.0)),
          color,
          1.0,
        );
      }
    }
    MediaProgress::Wave => {
      let amplitude = (rect.h * 0.5).max(3.0);
      let baseline = rect.y + rect.h * 0.5;
      let step = (4.0 * scale).max(1.0);
      let steps = (rect.w / step).ceil().max(2.0) as usize;
      let mut previous = None;
      for index in 0..=steps {
        let progress = index as f32 / steps as f32;
        let point = (
          rect.x + progress * rect.w,
          baseline + (progress * TAU * 3.0).sin() * amplitude,
        );
        if let Some(from) = previous {
          let color = if progress <= fraction {
            accent
          } else {
            muted.with_alpha(80)
          };
          canvas.line(from, point, color, 1.5 * scale);
        }
        previous = Some(point);
      }
    }
  }
}

/// The three transport glyphs and their click targets.
///
/// The glyph geometry is the same one the widget always drew. The new part is
/// that every centre also gets a button rectangle, returned in previous, play,
/// next order so the caller can push hit regions without guessing. A hovered
/// button gets a faint accent pill behind the glyph. The rectangles are always
/// returned; the caller decides whether they are really clickable, which is how
/// a rotated widget can keep the picture and still drop the hit regions.
fn draw_controls(
  canvas: &mut Canvas,
  rect: Rect,
  size: f32,
  color: Rgb,
  accent: Rgb,
  playing: bool,
  hovered: [bool; 3],
) -> [Rect; 3] {
  let center_y = rect.y + rect.h * 0.5;
  let center_x = rect.x + rect.w * 0.5;
  let gap = size * 1.1;
  let half = rect.h * 0.45;
  let stroke = (size * 0.08).max(1.0);
  let button_w = (gap * 0.85).max(size * 0.7);
  let button_h = (rect.h * 1.15).max(size * 0.9);
  let buttons = [
    Rect::new(
      center_x - gap - button_w * 0.5,
      center_y - button_h * 0.5,
      button_w,
      button_h,
    ),
    Rect::new(
      center_x - button_w * 0.5,
      center_y - button_h * 0.5,
      button_w,
      button_h,
    ),
    Rect::new(
      center_x + gap - button_w * 0.5,
      center_y - button_h * 0.5,
      button_w,
      button_h,
    ),
  ];
  for (button, active) in buttons.iter().zip(hovered) {
    if active {
      canvas.fill(*button, accent.with_alpha(40), (size * 0.25).max(2.0));
    }
  }
  let colors = [
    if hovered[0] { accent } else { color },
    if hovered[1] { accent } else { color },
    if hovered[2] { accent } else { color },
  ];
  let triangle = |canvas: &mut Canvas, points: [(f32, f32); 3], color: Rgb| {
    for index in 0..3 {
      let from = points[index];
      let to = points[(index + 1) % 3];
      canvas.line(from, to, color, stroke);
    }
  };
  let previous_x = center_x - gap;
  canvas.fill(
    Rect::new(previous_x - half * 0.8, center_y - half, stroke, half * 2.0),
    colors[0],
    1.0,
  );
  triangle(
    canvas,
    [
      (previous_x + half * 0.55, center_y - half),
      (previous_x + half * 0.55, center_y + half),
      (previous_x - half * 0.45, center_y),
    ],
    colors[0],
  );

  if playing {
    canvas.fill(
      Rect::new(
        center_x - half * 0.45,
        center_y - half,
        half * 0.3,
        half * 2.0,
      ),
      colors[1],
      1.0,
    );
    canvas.fill(
      Rect::new(
        center_x + half * 0.15,
        center_y - half,
        half * 0.3,
        half * 2.0,
      ),
      colors[1],
      1.0,
    );
  } else {
    triangle(
      canvas,
      [
        (center_x - half * 0.4, center_y - half),
        (center_x - half * 0.4, center_y + half),
        (center_x + half * 0.65, center_y),
      ],
      colors[1],
    );
  }

  let next_x = center_x + gap;
  triangle(
    canvas,
    [
      (next_x - half * 0.55, center_y - half),
      (next_x - half * 0.55, center_y + half),
      (next_x + half * 0.45, center_y),
    ],
    colors[2],
  );
  canvas.fill(
    Rect::new(next_x + half * 0.55, center_y - half, stroke, half * 2.0),
    colors[2],
    1.0,
  );
  buttons
}

/// Circular progress used by the vinyl layout. Drawing the ring as short line
/// segments avoids needing an arc primitive on the canvas.
fn draw_ring_progress(
  canvas: &mut Canvas,
  center: (f32, f32),
  radius: f32,
  fraction: f32,
  accent: Rgb,
  muted: Rgb,
  width: f32,
) {
  const STEPS: usize = 96;
  let fraction = fraction.clamp(0.0, 1.0);
  for index in 0..STEPS {
    let start = index as f32 / STEPS as f32 * TAU - TAU / 4.0;
    let end = (index + 1) as f32 / STEPS as f32 * TAU - TAU / 4.0;
    let color = if (index + 1) as f32 / STEPS as f32 <= fraction {
      accent
    } else {
      muted.with_alpha(70)
    };
    canvas.line(
      (
        center.0 + start.cos() * radius,
        center.1 + start.sin() * radius,
      ),
      (center.0 + end.cos() * radius, center.1 + end.sin() * radius),
      color,
      width,
    );
  }
}

/// What one media paint produced.
///
/// `bounds` is the widget rectangle the rotation layer and the drag region
/// already used. `controls` holds clickable previous, play, and next rectangles
/// in screen space, or `[None; 3]` when the widget has no metadata, controls
/// are off, or the widget is rotated. Keeping both in one return value means
/// the hit regions cannot drift from the pixels that were actually painted.
struct MediaDraw {
  bounds: Rect,
  controls: [Option<Rect>; 3],
}

#[allow(clippy::too_many_arguments)]
fn draw_media(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  media_index: usize,
  snapshot: &MediaSnapshot,
  anchor: crate::layout::Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  logical_size: f32,
  show_art: bool,
  show_before_login: bool,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  plate: Option<&WidgetPlate>,
  look: MediaLook,
  angle: f32,
) -> MediaDraw {
  let size = logical_size * scale;
  let rect = media_bounds(
    text,
    snapshot,
    anchor,
    offset,
    viewport,
    scale,
    logical_size,
    show_art,
    show_before_login,
    scene.view.authenticated,
    look,
  );
  // A greeter normally runs before the user's session exists. `show_before_login`
  // lets a desktop-session test or a single-user machine display the last
  // track; shared machines can turn it off and keep metadata off the pixels.
  if (!scene.view.authenticated && !show_before_login) || !snapshot.available {
    let label = "No media";
    paint_widget_backdrop(canvas, rect, scene.theme, scale, plate);
    text.draw(
      canvas,
      label,
      Style::new(size * 0.85, muted.with_alpha(160)),
      Area::new(rect.x, rect.y, rect.w.max(1.0)),
    );
    return MediaDraw {
      bounds: rect,
      controls: [None; 3],
    };
  }

  // A rotated widget keeps its controls as decoration but exposes no clicks:
  // the hit math for a rotated button is easy to get subtly wrong, and a
  // transport button that fires from the wrong pixel is worse than a static
  // one. The same rule covers a motion sample that rotated the whole widget.
  let interactive = angle.abs() <= 0.01;
  let mut controls: [Option<Rect>; 3] = [None; 3];

  // Art tinting is opt-in and derived from pixels, never from metadata. The
  // progress bar and an existing plate border/glow pick up the same hue so the
  // widget reads as one object instead of a coloured bar on a grey pill.
  let art_tint = if look.tint_from_art {
    snapshot.art_tint
  } else {
    None
  };
  let accent = art_tint.unwrap_or(accent);
  let tinted_plate = plate.map(|plate| {
    let mut plate = *plate;
    if let Some(tint) = art_tint {
      plate.border = Some(tint.with_alpha(180));
      if plate.glow.is_some() && plate.glow_size > 0.0 {
        plate.glow = Some(tint);
      }
    }
    plate
  });
  let plate = tinted_plate.as_ref();
  let art = media_art(snapshot, show_art, look.art);
  paint_widget_backdrop(canvas, rect, scene.theme, scale, plate);

  let (x, y) = (rect.x, rect.y);
  let width = rect.w;
  let playing = snapshot.playing;
  // Hover is resolved by the same ids the hit regions will use, so the glyph
  // and the click target cannot disagree about which button is under the
  // pointer. When controls are off we never even format the ids.
  let hovered = [
    look.controls && scene.drag.is_hovered(&format!("media:{media_index}:prev")),
    look.controls && scene.drag.is_hovered(&format!("media:{media_index}:play")),
    look.controls && scene.drag.is_hovered(&format!("media:{media_index}:next")),
  ];
  match look.layout {
    MediaLayout::Card => {
      let art_size = if art.is_some() { size * 3.0 } else { 0.0 };
      let mut text_x = x;
      if let Some(art) = art {
        canvas.image_in(art, Rect::new(x, y, art_size, art_size));
        text_x += art_size + size * 0.7;
      }
      let text_width = (width - (text_x - x)).max(1.0);
      text.draw_line(
        canvas,
        &snapshot.title,
        Style::new(
          size,
          primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
        ),
        Area::new(text_x, y, text_width),
      );
      text.draw_line(
        canvas,
        &snapshot.artist,
        Style::new(size * 0.85, muted),
        Area::new(text_x, y + size * 1.25, text_width),
      );
      if snapshot.stale {
        text.draw_line(
          canvas,
          "Last played",
          Style::new(size * 0.7, muted.with_alpha(150)),
          Area::new(text_x, y + size * 2.25, text_width),
        );
      } else if !snapshot.length.is_zero() {
        let fraction =
          (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0);
        let bar = Rect::new(
          text_x,
          y + size * 2.35,
          text_width,
          progress_height(look.progress, size, scale),
        );
        draw_progress(canvas, bar, fraction, look.progress, accent, muted, scale);
      }
      if look.controls {
        let rects = draw_controls(
          canvas,
          Rect::new(text_x, y + size * 3.15, text_width, size * 0.9),
          size,
          muted,
          accent,
          playing,
          hovered,
        );
        if interactive {
          controls = rects.map(Some);
        }
      }
    }
    MediaLayout::Compact => {
      let thumb = if art.is_some() { size * 1.6 } else { 0.0 };
      let mut text_x = x;
      if let Some(art) = art {
        let thumb_y = y + ((size * 2.6 - thumb) * 0.5).max(0.0);
        canvas.image_in(art, Rect::new(x, thumb_y, thumb, thumb));
        text_x += thumb + size * 0.5;
      }
      let text_width = (width - (text_x - x)).max(1.0);
      text.draw_line(
        canvas,
        &snapshot.title,
        Style::new(
          size,
          primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
        ),
        Area::new(text_x, y + size * 0.1, text_width),
      );
      text.draw_line(
        canvas,
        &snapshot.artist,
        Style::new(size * 0.8, muted),
        Area::new(text_x, y + size * 1.05, text_width),
      );
      if snapshot.stale {
        text.draw_line(
          canvas,
          "Last played",
          Style::new(size * 0.7, muted.with_alpha(150)),
          Area::new(text_x, y + size * 2.05, text_width),
        );
      } else if !snapshot.length.is_zero() {
        let fraction =
          (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0);
        let bar = Rect::new(
          text_x,
          y + size * 2.05,
          text_width,
          progress_height(look.progress, size, scale),
        );
        draw_progress(canvas, bar, fraction, look.progress, accent, muted, scale);
      }
      if look.controls {
        let rects = draw_controls(
          canvas,
          Rect::new(text_x, y + size * 2.75, text_width, size * 0.9),
          size,
          muted,
          accent,
          playing,
          hovered,
        );
        if interactive {
          controls = rects.map(Some);
        }
      }
    }
    MediaLayout::Vinyl => {
      let disc = if art.is_some() { size * 4.0 } else { 0.0 };
      let center_x = x + width * 0.5;
      if let Some(art) = art {
        canvas.image_in(art, Rect::new(center_x - disc * 0.5, y, disc, disc));
      }
      let fraction = if snapshot.length.is_zero() {
        0.0
      } else {
        (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0)
      };
      let ring = look.progress == MediaProgress::Line && disc > 0.0;
      if ring && !snapshot.stale {
        draw_ring_progress(
          canvas,
          (center_x, y + disc * 0.5),
          disc * 0.5 + size * 0.1,
          fraction,
          accent,
          muted,
          (size * 0.12).max(1.0),
        );
      }
      // The ring sits just outside the disc, so the title starts a little
      // lower than the card layout to keep clear of it.
      let text_y = y + disc + size * 0.35;
      text.draw_line(
        canvas,
        &snapshot.title,
        Style::new(
          size * 0.78,
          primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
        ),
        Area::new(x, text_y, width).align(Align::Center),
      );
      text.draw_line(
        canvas,
        &snapshot.artist,
        Style::new(size * 0.68, muted),
        Area::new(x, text_y + size * 0.95, width).align(Align::Center),
      );
      if snapshot.stale {
        text.draw_line(
          canvas,
          "Last played",
          Style::new(size * 0.7, muted.with_alpha(150)),
          Area::new(x, text_y + size * 2.0, width).align(Align::Center),
        );
      } else if !ring && !snapshot.length.is_zero() {
        let bar = Rect::new(
          x + width * 0.12,
          text_y + size * 2.0,
          width * 0.76,
          progress_height(look.progress, size, scale),
        );
        draw_progress(canvas, bar, fraction, look.progress, accent, muted, scale);
      }
      if look.controls {
        let rects = draw_controls(
          canvas,
          Rect::new(
            x + width * 0.25,
            text_y + size * 2.9,
            width * 0.5,
            size * 0.9,
          ),
          size,
          muted,
          accent,
          playing,
          hovered,
        );
        if interactive {
          controls = rects.map(Some);
        }
      }
    }
  }
  MediaDraw {
    bounds: rect,
    controls,
  }
}

/// Scales a colour's existing alpha by `opacity`. Alpha is the only channel
/// the plate multiplier touches, so hue stays exactly what the theme wrote.
fn alpha(color: Rgb, opacity: f32) -> Rgb {
  let value = (f32::from(color.a) * opacity.clamp(0.0, 1.0))
    .round()
    .clamp(0.0, 255.0) as u8;
  color.with_alpha(value)
}

/// Paints the backdrop for one widget. Built-in widgets pass their resolved
/// plate; plugin widgets have no look yet and keep the fixed scrim. Keeping the
/// choice in one place is what stops the two paths from drifting.
fn paint_widget_backdrop(
  canvas: &mut Canvas,
  bounds: Rect,
  theme: &Theme,
  scale: f32,
  plate: Option<&WidgetPlate>,
) {
  match plate {
    // The transform already multiplied the plate opacity into the whole
    // widget, so the plate itself must not apply it a second time.
    Some(plate) => draw_widget_plate(canvas, bounds, plate, theme, scale, 1.0),
    None => draw_scrim(canvas, bounds, theme, scale),
  }
}

/// Draws a themed plate behind a widget. The plate is sized from the widget's
/// content bounds plus the configured padding, so it always wraps the digits,
/// album art, and progress bar as one surface.
///
/// Shadow and glow paint first, then the fill, then the border. Every alpha is
/// multiplied by `opacity`, which lets a caller fade a plate without touching
/// the widget's text colours.
fn draw_widget_plate(
  canvas: &mut Canvas,
  bounds: Rect,
  plate: &WidgetPlate,
  theme: &Theme,
  scale: f32,
  opacity: f32,
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
    PlateStyle::None => {}
  }

  match (plate.border, plate.style) {
    (Some(border), _) => canvas.stroke(rect, alpha(border, opacity), radius, 1.0),
    // The historical scrim always had a faint theme border. Only Tint keeps
    // it implicitly; Solid and Gradient get a border only when asked.
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
  use std::time::{Duration, Instant};

  use super::{
    Scene, WidgetFrame, draw_media, glitch_window, parallax_offset, scanline_rows, scramble_run,
    scramble_text, widget_offset, widget_visible,
  };
  use crate::color::Rgb;
  use crate::config::{Config, MediaLayout, MediaLook};
  use crate::layout::Anchor;
  use crate::ui::canvas::{Canvas, Rect};
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
      cursor: None,
      screen: (480.0, 160.0),
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
      0,
      &snapshot,
      Anchor::TopLeft,
      (0, 0),
      Rect::new(0.0, 0.0, 480.0, 160.0),
      1.0,
      16.0,
      false,
      show_before_login,
      Rgb::solid(255, 255, 255),
      Rgb::solid(200, 200, 200),
      Rgb::solid(80, 120, 255),
      None,
      MediaLook::default(),
      0.0,
    );
    canvas.pixmap().data().to_vec()
  }

  /// Renders one media widget and returns only the transport rectangles. The
  /// pixel test above deliberately uses the default look (controls off), so
  /// this keeps the clickability check independent from the metadata privacy
  /// check.
  fn media_control_rects(controls: bool, angle: f32, available: bool) -> [Option<Rect>; 3] {
    let config = Config::default();
    let theme = Theme::from(&config);
    let view = View::default();
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
      cursor: None,
      screen: (480.0, 220.0),
    };
    let snapshot = MediaSnapshot {
      available,
      title: "Song".to_string(),
      artist: "Artist".to_string(),
      playing: true,
      position: Duration::from_secs(30),
      length: Duration::from_secs(180),
      ..MediaSnapshot::default()
    };
    let look = MediaLook {
      controls,
      layout: MediaLayout::Card,
      ..MediaLook::default()
    };
    let mut canvas = Canvas::new(480, 220).unwrap();
    let mut text = TextEngine::new(&config.ui.font);
    draw_media(
      &mut canvas,
      &mut text,
      &scene,
      2,
      &snapshot,
      Anchor::TopLeft,
      (0, 0),
      Rect::new(0.0, 0.0, 480.0, 220.0),
      1.0,
      16.0,
      false,
      true,
      Rgb::solid(255, 255, 255),
      Rgb::solid(200, 200, 200),
      Rgb::solid(80, 120, 255),
      None,
      look,
      angle,
    )
    .controls
  }

  #[test]
  fn scanline_rows_cover_the_height_and_clamp_a_zero_gap() {
    assert_eq!(scanline_rows(0, 10), 10);
    assert_eq!(scanline_rows(3, 9), 3);
    assert_eq!(scanline_rows(3, 10), 4);
    assert_eq!(scanline_rows(2, 720), 360);
  }

  #[test]
  fn media_controls_expose_click_targets_only_when_they_can_work() {
    let controls = media_control_rects(true, 0.0, true);
    assert!(
      controls.iter().all(Option::is_some),
      "a visible, unrotated widget with controls should expose all three buttons"
    );
    let [previous, play, next] = controls.map(Option::unwrap);
    assert!(
      previous.x < play.x && play.x < next.x,
      "buttons should follow the drawn previous, play, next order"
    );

    // Controls off, a rotated widget, and the No media state all keep the
    // picture but must not offer a click target.
    assert!(
      media_control_rects(false, 0.0, true)
        .iter()
        .all(Option::is_none)
    );
    assert!(
      media_control_rects(true, 25.0, true)
        .iter()
        .all(Option::is_none)
    );
    assert!(
      media_control_rects(true, 0.0, false)
        .iter()
        .all(Option::is_none)
    );
  }

  #[test]
  fn front_widget_offsets_and_extent_follow_the_card_scale() {
    // The card is 100x80 at 0.5 scale, so a front widget anchors inside that
    // shrunken card and its configured offset shrinks with it. This is the
    // exact math that keeps the kinich clock pill inside the scaled bar.
    let card = WidgetFrame::card(Rect::new(10.0, 20.0, 100.0, 80.0), 0.5);
    assert_eq!(card.origin, (10.0, 20.0));
    assert_eq!(card.extent, (100.0, 80.0));
    assert_eq!(card.scale, 0.5);
    assert_eq!(widget_offset(&card, (48.0, 14.0), (0.0, 0.0)), (24, 7));
    // Adding the drag before rounding matches decor and keeps a card-space
    // widget moving with the same pixel grid as its configured offset.
    assert_eq!(widget_offset(&card, (48.0, 14.0), (10.0, -4.0)), (29, 5));
    // A motion sample joins the configured offset in logical pixels before
    // the scale multiply, exactly like a decor item.
    assert_eq!(widget_offset(&card, (48.5, 14.4), (0.0, 0.0)), (24, 7));

    // The behind layer keeps the screen-space frame and the old rounding.
    let screen = WidgetFrame::screen((1920.0, 1080.0), 1.0);
    assert_eq!(screen.origin, (0.0, 0.0));
    assert_eq!(screen.extent, (1920.0, 1080.0));
    assert_eq!(widget_offset(&screen, (48.0, 14.0), (0.0, 0.0)), (48, 14));
    // A fractional sample rounds the same way the integer manifest offset
    // used to, so an unconfigured widget keeps its exact pixels.
    assert_eq!(widget_offset(&screen, (48.25, 14.75), (0.0, 0.0)), (48, 15));
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

  #[test]
  fn widget_visibility_prefers_hide_and_treats_an_empty_show_list_as_always() {
    use crate::config::WidgetState;
    use crate::ui::view::Line;

    let idle = View::default();
    // Empty show_on means always, even with a hide list that does not match.
    assert!(widget_visible(&[], &[], &idle));
    assert!(widget_visible(&[], &[WidgetState::Busy], &idle));

    // A state in hide_on wins even when show_on also lists it.
    assert!(!widget_visible(
      &[WidgetState::Typing],
      &[WidgetState::Typing],
      &View {
        username: "ada".to_string(),
        ..View::default()
      }
    ));

    // Typed content is the typing state.
    let typing = View {
      password: "secret".to_string(),
      ..View::default()
    };
    assert!(widget_visible(&[WidgetState::Typing], &[], &typing));
    assert!(!widget_visible(&[WidgetState::Idle], &[], &typing));

    // Busy outranks typing when a request is in flight.
    let busy = View {
      busy: true,
      username: "ada".to_string(),
      ..View::default()
    };
    assert!(widget_visible(&[WidgetState::Busy], &[], &busy));
    assert!(!widget_visible(&[WidgetState::Typing], &[], &busy));

    // An error status is the error state.
    let error = View {
      status: Some(Line::error("denied")),
      ..View::default()
    };
    assert!(widget_visible(&[WidgetState::Error], &[], &error));
    assert!(!widget_visible(&[WidgetState::Idle], &[], &error));

    // Authentication outranks an empty form.
    let authenticated = View {
      authenticated: true,
      ..View::default()
    };
    assert!(widget_visible(
      &[WidgetState::Authenticated],
      &[],
      &authenticated
    ));
    assert!(!widget_visible(&[WidgetState::Idle], &[], &authenticated));
  }

  #[test]
  fn scramble_run_reveals_from_the_head_and_is_deterministic() {
    let run = "hello world";
    // A screenshot and a settled effect both show the final text.
    assert_eq!(scramble_text(run, 0.0, 1000.0, 3), run);
    assert_eq!(scramble_run(run, 1.0, 3), run);

    let half = scramble_run(run, 0.5, 3);
    let original: Vec<char> = run.chars().collect();
    let scrambled: Vec<char> = half.chars().collect();
    assert_eq!(scrambled.len(), original.len());
    for (index, (before, after)) in original.iter().zip(&scrambled).enumerate() {
      if before.is_whitespace() {
        assert_eq!(before, after, "whitespace at {index} must survive");
      } else if index < original.len() / 2 {
        assert_eq!(before, after, "head character at {index} should stay");
      } else {
        assert_ne!(before, after, "tail character at {index} should scramble");
      }
    }
    // The same tick produces the same pixels; a new tick is free to reshuffle.
    assert_eq!(scramble_run(run, 0.5, 3), half);
  }

  #[test]
  fn glitch_window_is_open_only_at_the_start_of_a_period() {
    assert!(glitch_window(0.0, 1.0, 1000.0));
    assert!(glitch_window(0.05, 1.0, 1000.0));
    assert!(!glitch_window(0.08, 1.0, 1000.0));
    assert!(!glitch_window(0.5, 1.0, 1000.0));
    // The next period opens again.
    assert!(glitch_window(1.0, 1.0, 1000.0));
    assert!(!glitch_window(0.0, 0.0, 1000.0));
  }

  #[test]
  fn parallax_follows_the_pointer_and_needs_strength() {
    let mut config = Config::default();
    config.motion.parallax = 1.0;
    let theme = Theme::from(&config);
    let view = View::default();
    let now = jiff::Zoned::now();
    let drag = Drag::empty();
    let frame_now = Instant::now();
    let mut scene = Scene {
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
      cursor: None,
      screen: (100.0, 100.0),
    };
    // No pointer means no offset, which is what a screenshot needs.
    assert_eq!(parallax_offset(&scene, 1.0, 1.0), (0.0, 0.0));
    scene.cursor = Some((0.0, 50.0));
    let left = parallax_offset(&scene, 1.0, 1.0);
    assert!(left.0 > 0.0);
    scene.cursor = Some((100.0, 50.0));
    let right = parallax_offset(&scene, 1.0, 1.0);
    assert!(right.0 < 0.0);
    // Depth zero removes the strength even while the pointer is inside.
    assert_eq!(parallax_offset(&scene, 0.0, 1.0), (0.0, 0.0));
  }
}
