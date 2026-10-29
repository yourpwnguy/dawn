//! Small, restrained motion for the login screen.
//!
//! The greeter is not a game. Motion exists to make the form feel deliberate:
//! the card slides in once, the focus ring settles quickly, and a failed login
//! shakes once. Everything is driven by `Instant` and can be disabled with
//! `[motion] reduced = true` or `[power] low_power = true`.

use std::time::Instant;

use crate::config::MotionConfig;
use crate::ui::anim::{ease_out_cubic, progress};

/// The values the renderer needs for one animated frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionFrame {
  pub card_dx: f32,
  pub card_dy: f32,
  /// Opacity of the card tint and shadow during the boot fade.
  pub opacity: f32,
  /// 0.0 right after a focus change, 1.0 when settled.
  pub focus_t: f32,
  pub settled: bool,
}

impl MotionFrame {
  /// Screenshot and reduced-motion state: no offsets, fully opaque, settled.
  #[must_use]
  pub fn settled() -> Self {
    Self {
      card_dx: 0.0,
      card_dy: 0.0,
      opacity: 1.0,
      focus_t: 1.0,
      settled: true,
    }
  }
}

pub struct Motion {
  config: MotionConfig,
  reduced: bool,
  boot_start: Option<Instant>,
  focus_start: Option<Instant>,
  shake_start: Option<Instant>,
}

impl Motion {
  #[must_use]
  pub fn new(config: &MotionConfig, low_power: bool) -> Self {
    Self {
      config: config.clone(),
      reduced: config.reduced || low_power,
      boot_start: None,
      focus_start: None,
      shake_start: None,
    }
  }

  /// Records that the focused field changed. The next frame animates from 0.
  pub fn notify_focus(&mut self, now: Instant) {
    if self.reduced || !self.config.enabled || self.config.focus.is_zero() {
      return;
    }
    self.focus_start = Some(now);
  }

  /// Records a failed login. The card shakes once, then stops.
  pub fn notify_error(&mut self, now: Instant) {
    if self.reduced || !self.config.enabled || self.config.shake.is_zero() {
      return;
    }
    self.shake_start = Some(now);
  }

  /// Computes this frame's values. The boot animation starts on the first
  /// call, which is the first presented frame rather than process start.
  pub fn frame(&mut self, now: Instant, scale: f32) -> MotionFrame {
    if self.reduced || !self.config.enabled {
      return MotionFrame::settled();
    }
    let boot_start = *self.boot_start.get_or_insert(now);
    let boot_t = progress(now, boot_start, self.config.boot);
    let focus_t = self
      .focus_start
      .map_or(1.0, |start| progress(now, start, self.config.focus));
    let shake_t = self
      .shake_start
      .map_or(1.0, |start| progress(now, start, self.config.shake));

    let boot_ease = ease_out_cubic(boot_t);
    let card_dy = (1.0 - boot_ease) * 18.0 * scale;
    let card_dx = if shake_t < 1.0 {
      let decay = (1.0 - shake_t).powi(2);
      (shake_t * std::f32::consts::TAU * 3.0).sin() * 9.0 * scale * decay
    } else {
      0.0
    };
    let opacity = 0.35 + 0.65 * boot_ease;
    let settled = boot_t >= 1.0 && focus_t >= 1.0 && shake_t >= 1.0;
    if settled {
      self.focus_start = None;
      self.shake_start = None;
    }

    MotionFrame {
      card_dx,
      card_dy,
      opacity,
      focus_t,
      settled,
    }
  }

  /// True while the window should request animation frames.
  pub fn active(&self) -> bool {
    !self.reduced
      && self.config.enabled
      && (self.boot_start.is_none()
        || self
          .focus_start
          .is_some_and(|start| start.elapsed() < self.config.focus)
        || self
          .shake_start
          .is_some_and(|start| start.elapsed() < self.config.shake)
        || self
          .boot_start
          .is_some_and(|start| start.elapsed() < self.config.boot))
  }
}

#[cfg(test)]
mod tests {
  use super::Motion;
  use crate::config::MotionConfig;
  use std::time::{Duration, Instant};

  #[test]
  fn reduced_motion_is_always_settled() {
    let config = MotionConfig {
      reduced: true,
      ..MotionConfig::default()
    };
    let mut motion = Motion::new(&config, false);
    let frame = motion.frame(Instant::now(), 1.0);
    assert!(frame.settled);
    assert_eq!(frame.card_dy, 0.0);
  }

  #[test]
  fn boot_animation_ends_settled() {
    let config = MotionConfig {
      boot: Duration::from_millis(10),
      ..MotionConfig::default()
    };
    let mut motion = Motion::new(&config, false);
    let start = Instant::now();
    let first = motion.frame(start, 1.0);
    assert!(first.card_dy > 0.0);
    let last = motion.frame(start + Duration::from_millis(20), 1.0);
    assert!(last.settled);
    assert_eq!(last.card_dy, 0.0);
    assert_eq!(last.opacity, 1.0);
  }

  #[test]
  fn error_shake_decays_to_zero() {
    let config = MotionConfig {
      shake: Duration::from_millis(10),
      ..MotionConfig::default()
    };
    let mut motion = Motion::new(&config, false);
    let start = Instant::now();
    motion.notify_error(start);
    // At t=0 the sine starts at zero; a fifth of the way in it must not be.
    let mid = motion.frame(start + Duration::from_millis(2), 1.0);
    let last = motion.frame(start + Duration::from_millis(20), 1.0);
    assert!(mid.card_dx.abs() > 0.0);
    assert_eq!(last.card_dx, 0.0);
  }
}
