//! The optional login cut sequence.
//!
//! The sequence is a tiny, pure state machine driven by elapsed time. It has no
//! side effects and no handle on the auth worker: the window decides when to
//! start it, asks it for a frame while it runs, and goes back to the normal
//! screen the moment [`Transition::frame`] returns `None`.
//!
//! The timeline is:
//!
//! 1. Phase A, the cuts. One opaque full-screen fill per colour in `cuts`.
//! 2. Phase B, the slam. The last cut stays as a colour field while one big
//!    line scales in with a shake and a zoom punch.
//! 3. Phase C, the release. The field and the line fade out together, then the
//!    window is normal again.
//!
//! Safety rule: a zero-length sequence, a disabled table, or an elapsed time
//! past the total all resolve to "draw nothing". The auth request already left
//! the UI thread before any of this paints, and the window only ever holds its
//! own exit for a bounded `Success` trigger. Nothing here can block a login.

use std::time::{Duration, Instant};

use cosmic_text::{Align, Weight};

use crate::color::Rgb;
use crate::config::{TransitionConfig, TransitionTrigger};
use crate::ui::anim::ease_out_cubic;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};

/// How long the slam spends growing from its overshoot down to its final size.
const INTRO_MS: f64 = 140.0;
/// How long the field and line spend fading out at the end.
const FADE_MS: f64 = 180.0;
/// Letter spacing of the slam, in em.
const SLAM_TRACKING: f32 = 0.02;

/// One frame of the sequence. `fill` covers the whole screen; `slam` is drawn
/// on top of it when present.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionFrame {
  /// Full-screen fill, already carrying the fade alpha for phase C.
  pub fill: Rgb,
  pub slam: Option<SlamFrame>,
  /// Index of the active cut, or `None` during the slam. Kept for tests and
  /// debugging; the painter does not need it.
  pub cut_index: Option<usize>,
}

/// The big line, in logical pixels. The window multiplies size and offsets by
/// the output scale when it paints.
#[derive(Debug, Clone, PartialEq)]
pub struct SlamFrame {
  pub text: String,
  pub color: Rgb,
  pub size: f32,
  pub opacity: f32,
  /// Shake offset in logical pixels. Decays to zero over the phase.
  pub offset: (f32, f32),
}

/// A running sequence. Construction is fallible on purpose: a disabled table
/// or a zero-length timeline gives `None`, which is exactly today's behaviour.
#[derive(Debug, Clone)]
pub struct Transition {
  config: TransitionConfig,
  started: Instant,
}

impl Transition {
  /// Starts a sequence at `now`, or returns `None` when it cannot produce a
  /// single frame.
  #[must_use]
  pub fn start(config: &TransitionConfig, now: Instant) -> Option<Self> {
    if !config.enabled || config.total().is_zero() {
      return None;
    }
    Some(Self {
      config: config.clone(),
      started: now,
    })
  }

  #[must_use]
  pub fn trigger(&self) -> TransitionTrigger {
    self.config.trigger
  }

  /// Total configured length. The window also clamps its own exit hold, so a
  /// pathological `slam_ms` cannot keep the greeter on screen for minutes.
  #[must_use]
  pub fn total(&self) -> Duration {
    self.config.total()
  }

  #[must_use]
  pub fn elapsed(&self, now: Instant) -> Duration {
    now.saturating_duration_since(self.started)
  }

  #[must_use]
  pub fn finished(&self, now: Instant) -> bool {
    self.elapsed(now) >= self.total()
  }

  /// The frame at `now`, or `None` once the sequence is over or was never
  /// valid. Callers treat `None` as "paint the normal UI".
  #[must_use]
  pub fn frame(&self, now: Instant) -> Option<TransitionFrame> {
    self.frame_at(self.elapsed(now))
  }

  /// Pure timeline math, separated from `Instant` so tests can hit exact
  /// boundaries without sleeping.
  #[must_use]
  pub fn frame_at(&self, elapsed: Duration) -> Option<TransitionFrame> {
    let cuts = &self.config.cuts;
    let cut_ms = if self.config.cut_ms.is_finite() {
      f64::from(self.config.cut_ms.max(0.0))
    } else {
      0.0
    };
    let slam_ms = if self.config.slam_ms.is_finite() {
      f64::from(self.config.slam_ms.max(0.0))
    } else {
      0.0
    };
    let cut_total = cut_ms * cuts.len() as f64;
    let total = cut_total + slam_ms;
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    if total <= 0.0 || elapsed_ms >= total {
      return None;
    }

    // Phase A. A cut duration of zero is a valid way to skip straight to the
    // slam, so guard the division rather than inventing one millisecond.
    if cut_ms > 0.0 && elapsed_ms < cut_total {
      let index = ((elapsed_ms / cut_ms).floor() as usize).min(cuts.len().saturating_sub(1));
      return Some(TransitionFrame {
        fill: cuts[index],
        slam: None,
        cut_index: Some(index),
      });
    }

    // Phase B and C. The last cut becomes the field behind the line. The
    // fallback keeps a hand-built empty list safe even though validation
    // replaces it with the defaults.
    let field = cuts.last().copied().unwrap_or(Rgb::solid(0x0E, 0x0B, 0x10));
    let t = (elapsed_ms - cut_total).max(0.0);
    let fade_start = (slam_ms - FADE_MS.min(slam_ms)).max(0.0);
    let opacity = if slam_ms <= 0.0 {
      0.0
    } else if t < fade_start {
      1.0
    } else {
      ((slam_ms - t) / (slam_ms - fade_start).max(f64::MIN_POSITIVE)).clamp(0.0, 1.0)
    };

    // The intro shrinks from 1.15 to 1.0, and the zoom punch adds a small
    // extra scale that decays across the whole slam. Both are expressed as
    // text size, so the effect costs one rasterised line, not a transformed
    // full-screen scene.
    let intro_ms = INTRO_MS.min(slam_ms * 0.35).max(1.0);
    let intro_t = (t / intro_ms).clamp(0.0, 1.0) as f32;
    let intro_scale = 1.15 - 0.15 * ease_out_cubic(intro_t);
    let progress = (t / slam_ms.max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
    let decay = (1.0 - progress).powi(2);
    let zoom = if self.config.zoom.is_finite() {
      f64::from(self.config.zoom.max(0.0))
    } else {
      0.0
    };
    let zoom_scale = 1.0 + zoom * decay;
    let base_size = if self.config.slam_size.is_finite() {
      f64::from(self.config.slam_size.max(1.0))
    } else {
      1.0
    };
    let size = base_size * f64::from(intro_scale) * zoom_scale;

    let shake = if self.config.shake.is_finite() {
      f64::from(self.config.shake.max(0.0))
    } else {
      0.0
    };
    let amplitude = shake * decay;
    let offset = (
      ((t * std::f64::consts::TAU * 7.5).sin() * amplitude) as f32,
      ((t * std::f64::consts::TAU * 5.5).cos() * amplitude * 0.6) as f32,
    );

    let fill = field.with_alpha((f64::from(field.a) * opacity).round() as u8);
    let slam = (!self.config.slam.is_empty()).then(|| SlamFrame {
      text: self.config.slam.clone(),
      color: self.config.slam_color,
      size: size.max(1.0) as f32,
      opacity: opacity as f32,
      offset,
    });

    Some(TransitionFrame {
      fill,
      slam,
      cut_index: None,
    })
  }
}

/// Paints one sequence frame over a full screen. The fill is opaque during the
/// cuts and the held colour field, then translucent during the release, which
/// is what lets the login form come back underneath it.
pub fn paint(canvas: &mut Canvas, text: &mut TextEngine, frame: &TransitionFrame, scale: f32) {
  let width = canvas.width() as f32;
  let height = canvas.height() as f32;
  if width <= 0.0 || height <= 0.0 {
    return;
  }
  if frame.fill.a > 0 {
    canvas.fill(Rect::new(0.0, 0.0, width, height), frame.fill, 0.0);
  }
  let Some(slam) = &frame.slam else {
    return;
  };
  if slam.opacity <= 0.0 {
    return;
  }
  let scale = if scale.is_finite() && scale > 0.25 {
    scale
  } else {
    1.0
  };
  let size = slam.size * scale;
  if size <= 0.0 {
    return;
  }
  let style = Style::new(
    size,
    slam
      .color
      .with_alpha((f32::from(slam.color.a) * slam.opacity).round() as u8),
  )
  .bold()
  .tracking(SLAM_TRACKING);
  let (_, line_height) = text.measure_with(
    &slam.text,
    size,
    width - 64.0 * scale,
    false,
    Weight::BOLD,
    SLAM_TRACKING,
  );
  let area = Area::new(
    slam.offset.0 * scale,
    (height - line_height) / 2.0 + slam.offset.1 * scale,
    width,
  )
  .align(Align::Center)
  .clip(Rect::new(0.0, 0.0, width, height));
  text.draw_line(canvas, &slam.text, style, area);
}

#[cfg(test)]
mod tests {
  use super::Transition;
  use crate::color::Rgb;
  use crate::config::{TransitionConfig, TransitionTrigger};
  use std::time::{Duration, Instant};

  fn config() -> TransitionConfig {
    TransitionConfig {
      enabled: true,
      cuts: vec![
        Rgb::solid(0xFF, 0xFF, 0xFF),
        Rgb::solid(0xD4, 0x18, 0x30),
        Rgb::solid(0x0E, 0x0B, 0x10),
      ],
      cut_ms: 50.0,
      slam_ms: 100.0,
      ..TransitionConfig::default()
    }
  }

  fn at(ms: u64) -> Option<super::TransitionFrame> {
    let transition = Transition::start(&config(), Instant::now()).expect("enabled");
    transition.frame_at(Duration::from_millis(ms))
  }

  #[test]
  fn cuts_switch_on_exact_boundaries() {
    let first = at(0).expect("first cut");
    assert_eq!(first.cut_index, Some(0));
    assert_eq!(first.fill, Rgb::solid(0xFF, 0xFF, 0xFF));
    assert!(first.slam.is_none());

    let second = at(50).expect("second cut");
    assert_eq!(second.cut_index, Some(1));
    assert_eq!(second.fill, Rgb::solid(0xD4, 0x18, 0x30));

    let third = at(149).expect("third cut");
    assert_eq!(third.cut_index, Some(2));
    assert_eq!(third.fill, Rgb::solid(0x0E, 0x0B, 0x10));
    assert!(third.slam.is_none());
  }

  #[test]
  fn the_slam_starts_after_the_cuts_and_ends_at_the_total() {
    let slam = at(150).expect("slam starts");
    assert_eq!(slam.cut_index, None);
    let slam = slam.slam.expect("slam line");
    assert_eq!(slam.text, config().slam);
    assert!((slam.size - config().slam_size * 1.15 * 1.06).abs() < 0.5);
    assert_eq!(slam.opacity, 1.0);

    // 250 ms is exactly the configured total, so the normal UI is back.
    assert!(at(250).is_none());
  }

  #[test]
  fn the_fade_reveals_the_form_and_ends_cleanly() {
    let frame = at(240).expect("fading frame");
    let slam = frame.slam.expect("slam line");
    assert!(slam.opacity > 0.0 && slam.opacity < 1.0);
    assert!(frame.fill.a < 255);
    assert!(at(249).is_some());
  }

  #[test]
  fn a_disabled_or_zero_length_config_never_renders() {
    let now = Instant::now();
    let disabled = TransitionConfig::default();
    assert!(!disabled.enabled);
    assert!(Transition::start(&disabled, now).is_none());

    let zero = TransitionConfig {
      enabled: true,
      cuts: Vec::new(),
      cut_ms: 0.0,
      slam_ms: 0.0,
      ..TransitionConfig::default()
    };
    assert!(Transition::start(&zero, now).is_none());

    let zero_cuts = TransitionConfig {
      enabled: true,
      cuts: vec![Rgb::solid(0, 0, 0)],
      cut_ms: 0.0,
      slam_ms: 80.0,
      ..TransitionConfig::default()
    };
    let started = Transition::start(&zero_cuts, now).expect("slam still runs");
    assert!(started.frame_at(Duration::ZERO).is_some());
    assert!(started.finished(now + Duration::from_millis(80)));
    assert!(started.frame(now + Duration::from_millis(80)).is_none());
  }

  #[test]
  fn an_empty_slam_keeps_the_colour_field_only() {
    let config = TransitionConfig {
      slam: String::new(),
      ..config()
    };
    let transition = Transition::start(&config, Instant::now()).expect("enabled");
    let frame = transition
      .frame_at(Duration::from_millis(150))
      .expect("field");
    assert!(frame.slam.is_none());
    assert_eq!(frame.fill.a, 255);
  }

  #[test]
  fn triggers_match_the_configured_moment() {
    assert!(TransitionTrigger::Enter.accepts(TransitionTrigger::Enter));
    assert!(!TransitionTrigger::Enter.accepts(TransitionTrigger::Success));
    assert!(TransitionTrigger::Both.accepts(TransitionTrigger::Enter));
    assert!(TransitionTrigger::Both.accepts(TransitionTrigger::Success));
    assert!(TransitionTrigger::Success.accepts(TransitionTrigger::Success));

    // Boot and failure are opt-in only. In particular, `both` keeps its old
    // meaning so an existing config does not start cutting on failures.
    assert!(TransitionTrigger::Boot.accepts(TransitionTrigger::Boot));
    assert!(TransitionTrigger::Failure.accepts(TransitionTrigger::Failure));
    assert!(!TransitionTrigger::Both.accepts(TransitionTrigger::Boot));
    assert!(!TransitionTrigger::Both.accepts(TransitionTrigger::Failure));
    assert!(!TransitionTrigger::Boot.accepts(TransitionTrigger::Enter));
  }

  #[test]
  fn a_boot_sequence_plays_once_and_reaches_a_neutral_frame() {
    // The window starts the boot sequence once. This mirrors that call at
    // elapsed zero: a real frame, then a normal frame after the total.
    let transition = Transition::start(&config(), Instant::now()).expect("enabled");
    let first = transition
      .frame_at(Duration::ZERO)
      .expect("boot starts on the first frame");
    assert_eq!(first.cut_index, Some(0));
    assert!(transition.frame_at(transition.total()).is_none());
  }

  #[test]
  fn a_failure_trigger_can_use_a_single_short_cut_and_shake() {
    let config = TransitionConfig {
      enabled: true,
      trigger: TransitionTrigger::Failure,
      cuts: vec![Rgb::solid(0xD4, 0x18, 0x30)],
      cut_ms: 70.0,
      slam: String::new(),
      slam_ms: 0.0,
      shake: 10.0,
      zoom: 0.03,
      ..TransitionConfig::default()
    };
    let transition = Transition::start(&config, Instant::now()).expect("enabled");
    assert!(transition.trigger().accepts(TransitionTrigger::Failure));
    assert!(!transition.trigger().accepts(TransitionTrigger::Success));
    // One cut, then done. No held exit because finish is only ever gated on a
    // success trigger.
    let frame = transition
      .frame_at(Duration::from_millis(10))
      .expect("cut frame");
    assert_eq!(frame.fill, Rgb::solid(0xD4, 0x18, 0x30));
    assert!(frame.slam.is_none());
    assert_eq!(transition.total(), Duration::from_millis(70));
    assert!(transition.frame_at(Duration::from_millis(70)).is_none());
  }

  #[test]
  fn a_finished_or_missing_sequence_paints_nothing() {
    use crate::ui::canvas::Canvas;
    use crate::ui::text::TextEngine;

    let now = Instant::now();
    let transition = Transition::start(&config(), now).expect("enabled");
    let mut canvas = Canvas::new(96, 64).expect("canvas");
    let mut text = TextEngine::new("Inter");
    // This is exactly the window contract: ask for a frame and only paint when
    // there is one. At the total duration the answer is None.
    if let Some(frame) = transition.frame_at(transition.total()) {
      super::paint(&mut canvas, &mut text, &frame, 1.0);
    }
    assert!(
      canvas
        .pixmap()
        .pixels()
        .iter()
        .all(|pixel| pixel.alpha() == 0),
      "a finished sequence must leave the canvas untouched"
    );
    assert!(Transition::start(&TransitionConfig::default(), now).is_none());
  }

  #[test]
  fn a_live_sequence_actually_paints_the_field() {
    use crate::ui::canvas::Canvas;
    use crate::ui::text::TextEngine;

    let transition = Transition::start(&config(), Instant::now()).expect("enabled");
    let frame = transition.frame_at(Duration::ZERO).expect("first cut");
    let mut canvas = Canvas::new(96, 64).expect("canvas");
    let mut text = TextEngine::new("Inter");
    super::paint(&mut canvas, &mut text, &frame, 1.0);
    assert!(
      canvas
        .pixmap()
        .pixels()
        .iter()
        .any(|pixel| pixel.alpha() > 0),
      "the first cut should cover the canvas"
    );
  }
}
