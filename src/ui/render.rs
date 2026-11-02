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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;

use cosmic_text::{Align, Weight};

use crate::color::Rgb;
use crate::config::{
  ButtonConfig, ButtonEffect, CaretStyle, DecorSample, HoverReveal, MediaArt, MediaLayout,
  MediaLook, MediaProgress, ParticleLayer, PlateStyle, SurfaceStyle, TickerAnchor, WidgetLayer,
  WidgetLook, WidgetPlate, WidgetState,
};
use crate::plugin::{Plugin, Widget};
use crate::ui::anim::{cycle, ease_out_cubic};
use crate::ui::background::BackgroundView;
use crate::ui::canvas::{Canvas, Rect, rotated_bounds};
use crate::ui::clock;
use crate::ui::drag::{Drag, HitRegion};
use crate::ui::glass::GlassCache;
use crate::ui::hud::{self, Hud};
use crate::ui::media::MediaSnapshot;
use crate::ui::motion::{MotionFrame, decor_beat_scale, ticker_beat_speed};
use crate::ui::particles;
use crate::ui::splash::{self, SplashArt};
use crate::ui::text::{Area, Style, TextEngine};
use crate::ui::theme::Theme;
use crate::ui::view::{Tone, View};
use crate::ui::{battery, calendar, system};

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
  /// When the success particle burst was fired, if one is running. Screenshots
  /// and idle frames leave this `None`; the particle module turns the age into
  /// a closed-form field, so nothing mutates per frame.
  pub burst_at: Option<Instant>,
  /// The dev studio's corner HUD. `None` in normal runs and screenshots, which
  /// is the whole reason the HUD cannot leak into either.
  pub hud: Option<&'a Hud>,
  /// A fully rasterised boot splash. The window prepares this once and only
  /// the underline is repainted; `None` keeps the old direct-paint path for
  /// screenshots and tests.
  pub splash_art: Option<&'a SplashArt>,
  /// True when the GPU compositor draws the vignette in its shader. The
  /// overlay then skips the full-screen CPU radial gradient.
  pub post_gpu: bool,
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
  // An opaque boot card owns every pixel. Decoding, blitting, and blurring a
  // wallpaper underneath it is work nobody can see, and on a live video it is
  // the difference between a smooth title card and a stuttering one.
  let splash_opaque = scene
    .view
    .splash
    .as_ref()
    .is_some_and(|splash| splash.opaque(scene.background_color));
  if !splash_opaque {
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
  }
  paint_overlay(canvas, text, glass, scene, false)
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
  static_decor_cached: bool,
) -> Vec<HitRegion> {
  let decor = if static_decor_cached {
    DecorFilter::Dynamic
  } else {
    DecorFilter::All
  };
  let scale = normalized_scale(scene.scale);
  let screen = (canvas.width() as f32, canvas.height() as f32);
  // While the boot splash fully covers the frame there is no form to paint and
  // no hit region to expose. This is the only branch that skips the login UI
  // entirely; the crossfade draws the normal scene first and then fades the
  // splash on top.
  if let Some(view) = scene.view.splash.as_ref()
    && view.covers()
  {
    draw_splash(canvas, text, scene, view, scale);
    return Vec::new();
  }
  let primary = scene.theme.primary_on(scene.background_color);
  let muted = scene.theme.muted_on(scene.background_color);
  let error = scene.theme.error_on(scene.background_color);
  let accent = scene.theme.accent_on(scene.background_color);
  let focus = scene.theme.focus_on(scene.background_color);

  let mut regions = Vec::new();
  // Ambient emitters and the marquee are part of the overlay so the GPU and
  // CPU paths, and therefore `--screenshot`, all agree on the same pixels.
  // The behind layer sits over the wallpaper and under every login control.
  let ambient = ambient_elapsed(scene);
  let burst_age = scene
    .burst_at
    .map(|at| scene.frame_now.saturating_duration_since(at).as_secs_f32());
  particles::draw(
    canvas,
    &scene.theme.particles,
    screen,
    ambient,
    ParticleLayer::Behind,
    burst_age,
  );
  draw_ticker(canvas, text, glass, scene, WidgetLayer::Behind);
  draw_plugins(
    canvas,
    text,
    glass,
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
    decor,
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
  // The front ticker stays on top of the form, then the front particles are
  // the true topmost layer. Both are opt-in, so a default theme is unchanged.
  draw_ticker(canvas, text, glass, scene, WidgetLayer::Front);
  particles::draw(
    canvas,
    &scene.theme.particles,
    screen,
    ambient,
    ParticleLayer::Front,
    burst_age,
  );
  if let Some(hud) = scene.hud {
    hud::draw(canvas, text, hud, screen, scale);
  }
  // Crossfade, when the splash is still leaving. A fully covered splash took
  // the early branch above, so this only runs for 0 < opacity < 1.
  if let Some(view) = scene.view.splash.as_ref() {
    draw_splash(canvas, text, scene, view, scale);
  }
  if scene.post_gpu {
    draw_post_gpu(canvas, scene, screen);
  } else {
    draw_post(canvas, scene, screen);
  }
  regions
}

/// Cached still decor for a settled full-bleed card. The wing image is almost
/// a megapixel of alpha blending every frame; uploading it once and letting
/// the compositor put it under the live UI is what keeps a fullscreen video at
/// its source rate. The cache is only valid for a fixed card, so the window
/// enables it after the boot motion has settled and the card is not dragged.
pub(crate) struct DecorCache {
  key: Option<(u32, u32, u32)>,
  id: u64,
  pixmap: Option<Pixmap>,
}

static NEXT_DECOR_ID: AtomicU64 = AtomicU64::new(1);

impl DecorCache {
  pub(crate) fn new() -> Self {
    Self {
      key: None,
      id: 0,
      pixmap: None,
    }
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) fn prepare(
    &mut self,
    scene: &Scene<'_>,
    text: &mut TextEngine,
    glass: &GlassCache,
    screen: (u32, u32),
    scale: f32,
    enabled: bool,
  ) -> Option<&Pixmap> {
    if !enabled || screen.0 == 0 || screen.1 == 0 {
      self.clear();
      return None;
    }
    let key = (screen.0, screen.1, scale.to_bits());
    if self.key != Some(key) || self.pixmap.is_none() {
      let mut canvas = Canvas::new(screen.0, screen.1)?;
      paint_decor_layer(
        &mut canvas,
        text,
        glass,
        scene,
        (screen.0 as f32, screen.1 as f32),
        scale,
      );
      self.pixmap = Some(canvas.into_pixmap());
      self.key = Some(key);
      self.id = NEXT_DECOR_ID.fetch_add(1, Ordering::Relaxed);
    }
    self.pixmap.as_ref()
  }

  pub(crate) fn art(&self) -> Option<&Pixmap> {
    self.pixmap.as_ref()
  }

  pub(crate) fn id(&self) -> u64 {
    if self.pixmap.is_some() { self.id } else { 0 }
  }

  pub(crate) fn clear(&mut self) {
    self.key = None;
    self.id = 0;
    self.pixmap = None;
  }
}

/// Paints only the still decor into a transparent surface. This is the exact
/// same geometry and order as the live card, minus the moving items.
fn paint_decor_layer(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
) {
  let card_scale = card_scale(scene, screen, scale);
  let metrics = Metrics::new(scene.theme, screen, card_scale);
  let height = card_height(text, scene.view, scene.theme, &metrics, card_scale);
  let card = place_card(scene, screen, card_scale, &metrics, height);
  draw_login_decor(
    canvas,
    text,
    glass,
    scene,
    card,
    card_scale,
    DecorFilter::Static,
  );
}

/// Paints one splash frame with the theme accent resolved the same way every
/// other accent is. The splash owns the whole frame while it covers.
mod button;
mod card;
mod card_geometry;
mod chrome;
mod decor;
mod field;
mod media;
mod post;
mod ticker;
mod widgets;

use button::*;
use card::*;
use card_geometry::*;
use chrome::*;
use decor::*;
use field::*;
use media::*;
use post::*;
use ticker::*;
use widgets::*;

#[cfg(test)]
mod tests;
