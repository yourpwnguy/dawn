//! Small easing helpers shared by motion, button effects, and drag.
//!
//! Keeping these in one place matters because the login card, the drag easing,
//! and the button hover effects all have to agree on what "halfway" means.
//! They are intentionally tiny: the greeter does not need a full tween engine,
//! just predictable progress values driven by `Instant`.

use std::time::{Duration, Instant};

/// Linear 0..1 progress between `start` and `start + duration`.
#[must_use]
pub fn progress(now: Instant, start: Instant, duration: Duration) -> f32 {
  if duration.is_zero() {
    return 1.0;
  }
  (now.duration_since(start).as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

/// Decelerating cubic. Used by the boot slide and the button hover grow.
#[must_use]
pub fn ease_out_cubic(t: f32) -> f32 {
  1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Smoothstep, which makes a directional colour wipe look painted on rather
/// than like a hard rectangle sliding across the button.
#[must_use]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
  if (edge1 - edge0).abs() <= f32::EPSILON {
    return if x < edge0 { 0.0 } else { 1.0 };
  }
  let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
  t * t * (3.0 - 2.0 * t)
}

/// Repeating 0..1 phase for an effect with a fixed cycle length.
#[must_use]
pub fn cycle(elapsed: Duration, period: Duration) -> f32 {
  if period.is_zero() {
    return 0.0;
  }
  (elapsed.as_secs_f32() % period.as_secs_f32()) / period.as_secs_f32()
}
