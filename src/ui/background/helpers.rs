//! Helpers for the wallpaper engine.

use super::*;

pub(super) fn draw_gradient(canvas: &mut Canvas, color: Rgb) {
  canvas.vertical_gradient(color, shade(color, 0.72));
}

/// Darkens a colour by a factor, keeping alpha. Used for the default gradient
/// so a theme only has to specify one background colour.
pub(super) fn shade(color: Rgb, factor: f32) -> Rgb {
  let channel = |value: u8| (f32::from(value) * factor).round().clamp(0.0, 255.0) as u8;
  Rgb::rgba(
    channel(color.r),
    channel(color.g),
    channel(color.b),
    color.a,
  )
}

/// Duration does not implement remainder, so wrap nanosecond counts. All
/// wallpaper durations are far below u64 nanoseconds.
pub(super) fn wrap(value: Duration, modulus: Duration) -> Duration {
  if modulus.is_zero() {
    return Duration::ZERO;
  }
  Duration::from_nanos((value.as_nanos() % modulus.as_nanos()) as u64)
}
