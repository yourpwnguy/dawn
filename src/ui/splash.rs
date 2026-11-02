//! The optional boot title card.
//!
//! This is a tiny, clock-only state machine: full splash, crossfade, normal
//! login. It never talks to the auth worker and it has no opinion about
//! credentials. A theme that does not enable `[login.splash]` gets `None` and
//! the renderer behaves exactly as it did before the key existed.
//!
//! The crossfade is drawn by the splash itself rather than by the login cut
//! sequence. That keeps the form hidden until the fade starts, lets Escape keep
//! its normal app meaning, and means a broken splash can only look wrong, never
//! delay a login.

use std::time::Instant;

use cosmic_text::{Align, Weight};
use tiny_skia::Pixmap;

use crate::app::Key;
use crate::color::Rgb;
use crate::config::{SplashConfig, SplashDismiss};
use crate::ui::anim::progress;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};
use crate::ui::view::SplashView;

/// Which part of the boot sequence is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
  /// Full-screen title card. The login form is not painted.
  Splash,
  /// Splash fading out over the live login form.
  SplashFade,
  /// Normal login scene. The splash is finished for good.
  Login,
}

/// A running boot splash. Construction returns `None` when the key is disabled.
#[derive(Debug, Clone)]
pub struct SplashState {
  config: SplashConfig,
  started: Instant,
  /// Set when a key dismissed the card early. The timeout uses `started + ms`
  /// instead, so both paths share the same fade math.
  dismissed_at: Option<Instant>,
  /// Fully painted title card. The card is static except for the beat
  /// underline, so rasterising the text every 16 ms was pure waste. The cache
  /// is invalidated by size, scale, copy, or colour changes.
  cache: Option<SplashArt>,
}

impl SplashState {
  #[must_use]
  pub fn new(config: &SplashConfig, now: Instant) -> Option<Self> {
    config.enabled.then(|| Self {
      config: config.clone(),
      started: now,
      dismissed_at: None,
      cache: None,
    })
  }

  /// The instant the crossfade begins, whether by timeout or by key.
  fn fade_start(&self) -> Instant {
    self
      .dismissed_at
      .unwrap_or_else(|| self.started + self.config.ms)
  }

  #[must_use]
  pub fn phase(&self, now: Instant) -> Phase {
    let fade_start = self.fade_start();
    if now < fade_start {
      Phase::Splash
    } else if now < fade_start + self.config.fade {
      Phase::SplashFade
    } else {
      Phase::Login
    }
  }

  /// True while the splash still owns some pixels or is waiting for its
  /// timeout. The window uses this to keep its animation deadline alive.
  #[must_use]
  pub fn active(&self, now: Instant) -> bool {
    self.phase(now) != Phase::Login
  }

  /// True while the card fully hides the wallpaper. The GPU path can then
  /// upload the cached card once and composite only the underline.
  #[must_use]
  pub fn opaque(&self, now: Instant, background: Rgb) -> bool {
    self.opacity(now) >= 0.999 && self.config.bg.unwrap_or(background).a == 255
  }

  /// Handles one key. Returns true when the key belonged to the splash and the
  /// caller must not forward it to the login form.
  ///
  /// Escape is deliberately never consumed: the splash hides the form, but the
  /// normal clear/cancel semantics stay available.
  pub fn dismiss(&mut self, key: Key, now: Instant) -> bool {
    if key == Key::Escape || self.phase(now) != Phase::Splash {
      return false;
    }
    let matches = match self.config.dismiss_on {
      SplashDismiss::Timeout => false,
      SplashDismiss::Enter => key == Key::Enter,
      SplashDismiss::Any => true,
    };
    if matches && self.dismissed_at.is_none() {
      self.dismissed_at = Some(now);
    }
    matches
  }

  #[must_use]
  pub fn opacity(&self, now: Instant) -> f32 {
    match self.phase(now) {
      Phase::Splash => 1.0,
      Phase::SplashFade => 1.0 - progress(now, self.fade_start(), self.config.fade),
      Phase::Login => 0.0,
    }
  }

  /// The view for this frame, or `None` once the form owns the screen.
  #[must_use]
  pub fn view(&self, now: Instant, beat_phase: f32, beat_on: bool) -> Option<SplashView> {
    if self.phase(now) == Phase::Login {
      return None;
    }
    Some(SplashView {
      title: self.config.title.clone(),
      sub: self.config.sub.clone(),
      opacity: self.opacity(now),
      beat_phase,
      underline: beat_on,
      bg: self.config.bg,
      accent: self.config.accent,
    })
  }

  /// Paints the static part of the card once and returns the cached art.
  ///
  /// This is called from the window's redraw path, which owns the text engine.
  /// It is intentionally a no-op after the splash reaches `Login`, so a later
  /// clock-driven repaint cannot resurrect the card.
  pub fn prepare(
    &mut self,
    now: Instant,
    size: (u32, u32),
    scale: f32,
    background: Rgb,
    theme_accent: Rgb,
    text: &mut TextEngine,
  ) -> Option<&SplashArt> {
    if self.phase(now) == Phase::Login {
      self.cache = None;
      return None;
    }
    let bg = self.config.bg.unwrap_or(background);
    let accent = self.config.accent.unwrap_or(theme_accent);
    let key = SplashKey {
      width: size.0,
      height: size.1,
      scale: scale.to_bits(),
      title: self.config.title.clone(),
      sub: self.config.sub.clone(),
      bg,
      accent,
    };
    if self.cache.as_ref().is_none_or(|art| art.key != key) {
      let Some(mut canvas) = Canvas::new(size.0, size.1) else {
        self.cache = None;
        return None;
      };
      let (title_width, underline_y) = render_base(
        &mut canvas,
        text,
        &self.config.title,
        &self.config.sub,
        bg,
        accent,
        scale,
        1.0,
      );
      self.cache = Some(SplashArt {
        key,
        id: NEXT_SPLASH_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        pixmap: canvas.into_pixmap(),
        title_width,
        underline_y,
        scale: normalized_scale(scale),
        accent,
      });
    }
    self.cache.as_ref()
  }

  #[must_use]
  pub fn art(&self) -> Option<&SplashArt> {
    self.cache.as_ref()
  }
}

/// One fully rasterised title card. The underline is deliberately not part of
/// the bitmap because it pulses with the beat; everything else is static.
#[derive(Debug, Clone)]
pub struct SplashArt {
  key: SplashKey,
  id: u64,
  pixmap: Pixmap,
  title_width: f32,
  underline_y: f32,
  scale: f32,
  accent: Rgb,
}

/// Monotonic cache identity. A pointer could be reused by the allocator after
/// a title change, which would let the GPU keep a stale card.
static NEXT_SPLASH_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
struct SplashKey {
  width: u32,
  height: u32,
  scale: u32,
  title: String,
  sub: String,
  bg: Rgb,
  accent: Rgb,
}

/// One animated underline in physical pixels and straight RGBA. The GPU
/// compositor draws this over the cached splash texture, which means the title
/// bitmap is uploaded once instead of every beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplashUnderline {
  pub rect: [f32; 4],
  pub color: [f32; 4],
}

impl SplashArt {
  /// The cached, fully painted card. The window uploads this once.
  #[must_use]
  pub fn pixmap(&self) -> &Pixmap {
    &self.pixmap
  }

  /// A stable identity for the cached texture. The GPU path compares it to the
  /// texture it already uploaded so a title or size change is the only reason
  /// to upload again.
  #[must_use]
  pub fn id(&self) -> u64 {
    self.id
  }

  /// The live underline for this frame, or `None` when it is off or faded out.
  #[must_use]
  pub fn underline(&self, view: &SplashView) -> Option<SplashUnderline> {
    if !view.underline || self.key.title.is_empty() {
      return None;
    }
    underline(
      self.key.width as f32,
      self.title_width,
      self.underline_y,
      self.scale,
      self.accent,
      view.beat_phase,
      view.opacity,
    )
  }

  /// Draws the cached card and the live underline at `view.opacity`.
  pub fn paint(&self, canvas: &mut Canvas, view: &SplashView) {
    let opacity = view.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
      return;
    }
    canvas.image_cover_alpha(&self.pixmap, opacity);
    if let Some(line) = self.underline(view) {
      let [r, g, b, a] = line.color;
      canvas.fill(
        Rect::new(line.rect[0], line.rect[1], line.rect[2], line.rect[3]),
        Rgb::rgba(
          (r * 255.0).round() as u8,
          (g * 255.0).round() as u8,
          (b * 255.0).round() as u8,
          (a * 255.0).round() as u8,
        ),
        (1.0 * self.scale).max(0.5),
      );
    }
  }
}

/// Paints one splash frame. The caller draws this either instead of the login
/// overlay (`covers()`) or over it during the crossfade.
pub fn paint(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  view: &SplashView,
  background: Rgb,
  accent: Rgb,
  scale: f32,
) {
  let width = canvas.width() as f32;
  let height = canvas.height() as f32;
  if width <= 0.0 || height <= 0.0 || view.opacity <= 0.0 {
    return;
  }
  let scale = normalized_scale(scale);
  let opacity = view.opacity.clamp(0.0, 1.0);
  let bg = view.bg.unwrap_or(background);
  let accent = view.accent.unwrap_or(accent);
  let (title_w, line_y) = render_base(
    canvas,
    text,
    &view.title,
    &view.sub,
    bg,
    accent,
    scale,
    opacity,
  );
  if view.underline && !view.title.is_empty() {
    draw_underline(
      canvas,
      width,
      title_w,
      line_y,
      scale,
      accent,
      view.beat_phase,
      opacity,
    );
  }
}

fn normalized_scale(scale: f32) -> f32 {
  if scale.is_finite() && scale > 0.25 {
    scale
  } else {
    1.0
  }
}

/// Fills the card and draws both text runs. Returns the measured title width
/// and the underline's y position so the cached frame can animate the line
/// without touching the text again.
#[allow(clippy::too_many_arguments)]
fn render_base(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  title: &str,
  sub: &str,
  bg: Rgb,
  accent: Rgb,
  scale: f32,
  opacity: f32,
) -> (f32, f32) {
  let width = canvas.width() as f32;
  let height = canvas.height() as f32;
  let alpha = (f32::from(bg.a) * opacity).round().clamp(0.0, 255.0) as u8;
  canvas.fill(
    Rect::new(0.0, 0.0, width, height),
    bg.with_alpha(alpha),
    0.0,
  );

  let ink = Rgb::solid(0xF7, 0xF3, 0xE9);
  let ink_alpha = (255.0 * opacity).round().clamp(0.0, 255.0) as u8;
  let title_size = (32.0 * scale).max(10.0);
  let sub_size = (16.0 * scale).max(8.0);
  let title_style = Style::new(title_size, ink.with_alpha(ink_alpha))
    .bold()
    .tracking(0.12);
  let sub_style = Style::new(sub_size, accent.with_alpha(ink_alpha)).tracking(0.08);
  let (title_w, title_h) = text.measure_with(
    title,
    title_size,
    width - 64.0 * scale,
    false,
    Weight::BOLD,
    0.12,
  );
  let (_, sub_h) = text.measure_with(
    sub,
    sub_size,
    width - 64.0 * scale,
    false,
    Weight::NORMAL,
    0.08,
  );
  let gap = 14.0 * scale;
  let block = title_h + gap + sub_h + gap * 1.4;
  let top = ((height - block) / 2.0).max(0.0);
  text.draw_line(
    canvas,
    title,
    title_style,
    Area::new(0.0, top, width)
      .align(Align::Center)
      .clip(Rect::new(0.0, 0.0, width, height)),
  );
  text.draw_line(
    canvas,
    sub,
    sub_style,
    Area::new(0.0, top + title_h + gap, width)
      .align(Align::Center)
      .clip(Rect::new(0.0, 0.0, width, height)),
  );
  (title_w, top + title_h + gap * 0.45)
}

/// The underline is the one place the beat clock shows up in the splash. It
/// is a pure function of the phase, so a paused or reduced-motion frame gets a
/// steady line instead of a frozen one at a random width.
#[allow(clippy::too_many_arguments)]
fn draw_underline(
  canvas: &mut Canvas,
  width: f32,
  title_w: f32,
  line_y: f32,
  scale: f32,
  accent: Rgb,
  beat_phase: f32,
  opacity: f32,
) {
  let Some(line) = underline(width, title_w, line_y, scale, accent, beat_phase, opacity) else {
    return;
  };
  let [r, g, b, a] = line.color;
  canvas.fill(
    Rect::new(line.rect[0], line.rect[1], line.rect[2], line.rect[3]),
    Rgb::rgba(
      (r * 255.0).round() as u8,
      (g * 255.0).round() as u8,
      (b * 255.0).round() as u8,
      (a * 255.0).round() as u8,
    ),
    (1.0 * scale).max(0.5),
  );
}

/// Pure geometry shared by the CPU paint and the GPU overlay.
#[allow(clippy::too_many_arguments)]
fn underline(
  width: f32,
  title_w: f32,
  line_y: f32,
  scale: f32,
  accent: Rgb,
  beat_phase: f32,
  opacity: f32,
) -> Option<SplashUnderline> {
  if opacity <= 0.0 || width <= 0.0 {
    return None;
  }
  let pulse = 1.0 - beat_phase.clamp(0.0, 1.0);
  let underline_w = (title_w.max(64.0 * scale) * (1.0 + 0.12 * pulse)).min(width - 32.0);
  if underline_w <= 0.0 {
    return None;
  }
  let pulse_alpha = (90.0 + 165.0 * pulse) / 255.0;
  let x = (width - underline_w) / 2.0;
  Some(SplashUnderline {
    rect: [x, line_y, underline_w, (2.0 * scale).max(1.0)],
    color: [
      f32::from(accent.r) / 255.0,
      f32::from(accent.g) / 255.0,
      f32::from(accent.b) / 255.0,
      (pulse_alpha * opacity).clamp(0.0, 1.0),
    ],
  })
}

#[cfg(test)]
mod tests {
  use super::{Phase, SplashState};
  use crate::app::Key;
  use crate::config::{SplashConfig, SplashDismiss};
  use std::time::{Duration, Instant};

  fn config(dismiss_on: SplashDismiss) -> SplashConfig {
    SplashConfig {
      enabled: true,
      ms: Duration::from_millis(1000),
      title: "ENCORE".to_string(),
      sub: "loading the stage".to_string(),
      fade: Duration::from_millis(200),
      dismiss_on,
      ..SplashConfig::default()
    }
  }

  #[test]
  fn timeout_holds_then_fades_then_returns_to_login() {
    let start = Instant::now();
    let splash = SplashState::new(&config(SplashDismiss::Timeout), start).expect("enabled");
    assert_eq!(splash.phase(start), Phase::Splash);
    assert_eq!(
      splash.phase(start + Duration::from_millis(999)),
      Phase::Splash
    );
    assert_eq!(
      splash.phase(start + Duration::from_millis(1000)),
      Phase::SplashFade
    );
    assert!(splash.opacity(start + Duration::from_millis(1100)) < 0.6);
    assert_eq!(
      splash.phase(start + Duration::from_millis(1200)),
      Phase::Login
    );
    assert!(
      splash
        .view(start + Duration::from_millis(1200), 0.0, false)
        .is_none()
    );
  }

  #[test]
  fn enter_dismissal_only_accepts_enter() {
    let start = Instant::now();
    let mut splash = SplashState::new(&config(SplashDismiss::Enter), start).expect("enabled");
    assert!(!splash.dismiss(Key::Char('x'), start + Duration::from_millis(10)));
    assert_eq!(
      splash.phase(start + Duration::from_millis(10)),
      Phase::Splash
    );
    assert!(splash.dismiss(Key::Enter, start + Duration::from_millis(20)));
    assert_eq!(
      splash.phase(start + Duration::from_millis(20)),
      Phase::SplashFade
    );
    // A second key during the fade is no longer the splash's business.
    assert!(!splash.dismiss(Key::Enter, start + Duration::from_millis(30)));
  }

  #[test]
  fn any_key_dismisses_but_escape_is_left_to_the_form() {
    let start = Instant::now();
    let mut splash = SplashState::new(&config(SplashDismiss::Any), start).expect("enabled");
    assert!(!splash.dismiss(Key::Escape, start + Duration::from_millis(10)));
    assert_eq!(
      splash.phase(start + Duration::from_millis(10)),
      Phase::Splash
    );
    assert!(splash.dismiss(Key::Char('a'), start + Duration::from_millis(20)));
    assert_eq!(
      splash.phase(start + Duration::from_millis(20)),
      Phase::SplashFade
    );
  }

  #[test]
  fn timeout_mode_ignores_keys_until_the_clock_expires() {
    let start = Instant::now();
    let mut splash = SplashState::new(&config(SplashDismiss::Timeout), start).expect("enabled");
    assert!(!splash.dismiss(Key::Enter, start + Duration::from_millis(10)));
    assert!(!splash.dismiss(Key::Char('a'), start + Duration::from_millis(20)));
    assert_eq!(
      splash.phase(start + Duration::from_millis(20)),
      Phase::Splash
    );
  }

  #[test]
  fn the_static_card_is_prepared_once_and_reused() {
    use crate::color::Rgb;
    use crate::ui::text::TextEngine;

    let start = Instant::now();
    let mut splash = SplashState::new(&config(SplashDismiss::Timeout), start).expect("enabled");
    let mut text = TextEngine::new("");
    let first = splash
      .prepare(
        start,
        (320, 180),
        1.0,
        Rgb::solid(9, 6, 6),
        Rgb::solid(232, 0, 35),
        &mut text,
      )
      .expect("splash art");
    let first_key = first.key.clone();
    assert!(
      first
        .pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[3] > 0)
    );
    let second = splash
      .prepare(
        start + Duration::from_millis(50),
        (320, 180),
        1.0,
        Rgb::solid(9, 6, 6),
        Rgb::solid(232, 0, 35),
        &mut text,
      )
      .expect("splash art");
    assert_eq!(second.key, first_key, "same frame must not re-rasterise");
  }

  #[test]
  fn a_finished_splash_drops_its_cached_card() {
    use crate::color::Rgb;
    use crate::ui::text::TextEngine;

    let start = Instant::now();
    let mut splash = SplashState::new(&config(SplashDismiss::Timeout), start).expect("enabled");
    let mut text = TextEngine::new("");
    let _ = splash.prepare(
      start,
      (320, 180),
      1.0,
      Rgb::solid(9, 6, 6),
      Rgb::solid(232, 0, 35),
      &mut text,
    );
    assert!(splash.art().is_some());
    assert!(
      splash
        .prepare(
          start + Duration::from_millis(5000),
          (320, 180),
          1.0,
          Rgb::solid(9, 6, 6),
          Rgb::solid(232, 0, 35),
          &mut text,
        )
        .is_none()
    );
    assert!(splash.art().is_none());
  }

  #[test]
  fn disabled_splash_never_exists() {
    let config = SplashConfig::default();
    assert!(!config.enabled);
    assert!(SplashState::new(&config, Instant::now()).is_none());
  }

  #[test]
  fn active_covers_the_whole_fade_and_nothing_after_it() {
    let start = Instant::now();
    let splash = SplashState::new(&config(SplashDismiss::Timeout), start).expect("enabled");
    assert!(splash.active(start));
    assert!(splash.active(start + Duration::from_millis(1199)));
    assert!(!splash.active(start + Duration::from_millis(1200)));
  }
}
