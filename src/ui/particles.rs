//! Procedural ambient particles.
//!
//! This module deliberately keeps no mutable simulation state. Each particle's
//! base position, speed variation, size, and phase come from a tiny integer
//! hash of its index, and the frame time is applied as a closed-form motion.
//! That buys three things a stateful emitter would not:
//!
//! - The CPU and GPU paths draw the exact same field from the same [`Scene`],
//!   so `--screenshot` is honest and a resize never changes the layout.
//! - There is nothing to advance, mutate, or allocate between frames. Turning
//!   particles off is a single boolean check at the top of [`draw`].
//! - Wrapping an ember that leaves the top of the screen back to the bottom is
//!   just `rem_euclid`, which is the "respawn" without a spawn table.
//!
//! The sprites are generated here too: a radial ember, two mirrored wing
//! triangles, or a plain dot. A theme never needs image files for them, and the
//! cost is one tiny-skia draw per particle per frame with a hard cap of
//! [`MAX_PARTICLES`] (96).
//!
//! [`Scene`]: crate::ui::render::Scene

use std::f32::consts::TAU;

use crate::color::Rgb;
use crate::config::{MAX_PARTICLES, ParticleDrift, ParticleLayer, ParticleSprite, ParticlesConfig};
use crate::ui::canvas::{Canvas, Rect};

/// One particle's resolved position and look for a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
  pub x: f32,
  pub y: f32,
  pub size: f32,
  /// Flicker and base opacity multiplied together, 0 to 1.
  pub opacity: f32,
  /// Butterfly wing span for this frame, about 0.15 to 1.0.
  pub flap: f32,
}

/// SplitMix64, used only to spread particle seeds. We do not need
/// cryptographic quality here, just values that do not line up in visible rows.
fn hash(mut value: u64) -> u64 {
  value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
  value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
  value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
  value ^ (value >> 31)
}

/// A deterministic 0..1 value for one particle and one seed slot.
fn unit(index: u32, salt: u64) -> f32 {
  let bits = hash((u64::from(index) << 32) | salt);
  // The top 24 bits are plenty for a visual position and avoid any float
  // rounding surprises at 1.0.
  ((bits >> 40) as f32) / 16_777_216.0
}

/// Wraps a coordinate into `0..max`. A zero-size screen collapses to zero
/// rather than dividing by it, which keeps a hand-built test config harmless.
fn wrap(value: f32, max: f32) -> f32 {
  if max <= 0.0 {
    0.0
  } else {
    value.rem_euclid(max)
  }
}

/// Resolves one particle at `elapsed` seconds.
///
/// The returned position is the sprite centre and is always inside the screen
/// rectangle. Motion is deterministic for a given config, index, and elapsed
/// time; the seed is stable across sessions on purpose so a screenshot or a
/// recording can be reproduced.
#[must_use]
pub fn sample(
  config: &ParticlesConfig,
  index: u32,
  elapsed: f32,
  screen: (f32, f32),
) -> ParticleSample {
  let elapsed = if elapsed.is_finite() {
    elapsed.max(0.0)
  } else {
    0.0
  };
  let (width, height) = (screen.0.max(0.0), screen.1.max(0.0));
  let base_x = unit(index, 0x11) * width;
  let base_y = unit(index, 0x22) * height;
  let speed_scale = 0.65 + unit(index, 0x33) * 0.7;
  let phase = unit(index, 0x44) * TAU;
  let size_scale = 0.7 + unit(index, 0x55) * 0.6;

  let travel = config.speed.max(0.0) * speed_scale * elapsed;
  let (x, y) = match config.drift {
    ParticleDrift::Up => (
      wrap(
        base_x + (elapsed * 0.9 + phase).sin() * config.size * 0.8,
        width,
      ),
      wrap(base_y - travel, height),
    ),
    ParticleDrift::Down => (
      wrap(
        base_x + (elapsed * 0.7 + phase).sin() * config.size * 0.8,
        width,
      ),
      wrap(base_y + travel, height),
    ),
    ParticleDrift::Float => (
      wrap(
        base_x + (elapsed * 0.45 + phase).sin() * config.size * 2.4,
        width,
      ),
      wrap(
        base_y + (elapsed * 0.37 + phase * 1.3).cos() * config.size * 1.8,
        height,
      ),
    ),
  };

  // Flames breathe and flicker. Butterflies keep a steadier body opacity but
  // flap much faster than a flame wobbles. A streak is a light ray, so it
  // stays steady and only fades at the very ends of its travel.
  let (flicker, flap) = match config.sprite {
    ParticleSprite::Flame => (
      0.72 + 0.28 * (elapsed * 6.5 + phase * 3.0).sin(),
      (elapsed * 4.0 + phase).sin(),
    ),
    ParticleSprite::Butterfly => (0.9, (elapsed * 5.2 + phase).sin()),
    ParticleSprite::Dot => (1.0, 0.0),
    ParticleSprite::Streak => (0.82 + 0.18 * (elapsed * 2.1 + phase).sin(), 0.0),
  };
  let size = match config.sprite {
    ParticleSprite::Flame => config.size * size_scale * (0.9 + 0.18 * flicker),
    ParticleSprite::Butterfly => config.size * size_scale,
    ParticleSprite::Dot => config.size * size_scale,
    // A streak is deliberately taller than it is wide. The draw routine
    // keeps the width thin, so this is the full length of the light ray.
    ParticleSprite::Streak => config.size * size_scale * 2.4,
  };

  ParticleSample {
    x,
    y,
    size: size.max(0.5),
    opacity: (config.opacity * flicker).clamp(0.0, 1.0),
    flap: 0.15 + 0.85 * flap.abs(),
  }
}

/// Resolves one sprite of the one-shot success burst.
///
/// This is a closed-form function of the time since the burst started, not a
/// simulation. At age zero every sprite sits at the centre, then it travels
/// outward with an exponential drag and fades. Because positions depend only
/// on (index, age) the burst cannot spawn twice for one transition and a
/// dropped frame cannot duplicate sprites. It returns `None` once the burst
/// has died or when the burst is disabled, which is the signal to stop
/// scheduling frames for it.
#[must_use]
pub fn burst_sample(
  config: &ParticlesConfig,
  index: u32,
  age: f32,
  screen: (f32, f32),
) -> Option<ParticleSample> {
  if config.burst_on == crate::config::BurstTrigger::None {
    return None;
  }
  let count = config.burst_count.min(crate::config::MAX_BURST_PARTICLES);
  if index >= count || !(0.0..crate::config::BURST_LIFETIME).contains(&age) {
    return None;
  }
  let (width, height) = (screen.0.max(1.0), screen.1.max(1.0));
  let (cx, cy) = (width / 2.0, height / 2.0);
  let angle = unit(index, 0x91) * TAU;
  let speed = 260.0 + unit(index, 0x92) * 320.0;
  // Exponential drag, no gravity: v(t) = v0 * exp(-k t), so distance is the
  // integral. k = 1.2 gives a wide launch that still visibly slows: the burst
  // has to clear the slam line, which sits across the middle of the stage.
  let drag = 1.2;
  let distance = speed / drag * (1.0 - (-drag * age).exp());
  // The vertical radius is slightly compressed so the burst reads as a
  // horizontal shockwave on a 16:9 stage rather than a perfect circle.
  let x = (cx + angle.cos() * distance).clamp(0.0, width);
  let y = (cy + angle.sin() * distance * 0.82).clamp(0.0, height);
  let fade = (1.0 - age / crate::config::BURST_LIFETIME).clamp(0.0, 1.0);
  let opacity = (config.opacity * fade.powf(1.4)).clamp(0.0, 1.0);
  let size_scale = 0.75 + unit(index, 0x93) * 0.6;
  let size = match config.burst_sprite {
    ParticleSprite::Streak => config.size * size_scale * 2.6,
    _ => config.size * size_scale * 1.35,
  };
  Some(ParticleSample {
    x,
    y,
    size: size.max(0.5),
    opacity,
    flap: 0.2 + 0.8 * (age * 9.0 + unit(index, 0x94) * TAU).sin().abs(),
  })
}

/// Paints every particle in one layer. Disabled emitters and zero counts return
/// before touching the canvas, which is what keeps an existing theme's pixels
/// byte-for-byte unchanged.
///
/// `burst_age` is the seconds since the success transition fired. `None` means
/// no burst is running, so nothing beyond the ambient field paints.
pub fn draw(
  canvas: &mut Canvas,
  config: &ParticlesConfig,
  screen: (f32, f32),
  elapsed: f32,
  layer: ParticleLayer,
  burst_age: Option<f32>,
) {
  if !config.enabled || config.layer != layer {
    return;
  }
  let count = config.count.min(MAX_PARTICLES);
  if count > 0 && screen.0 > 0.0 && screen.1 > 0.0 {
    for index in 0..count {
      let particle = sample(config, index, elapsed, screen);
      draw_sprite(
        canvas,
        config.sprite,
        &particle,
        config.color,
        config.color2,
      );
    }
  }

  // The burst is a separate one-shot field. It uses its own sprite and count,
  // so a theme can keep calm embers in the ambient layer and still fire a
  // butterfly shockwave on success.
  let Some(age) = burst_age else {
    return;
  };
  if screen.0 <= 0.0 || screen.1 <= 0.0 {
    return;
  }
  let burst_count = config.burst_count.min(crate::config::MAX_BURST_PARTICLES);
  for index in 0..burst_count {
    let Some(particle) = burst_sample(config, index, age, screen) else {
      continue;
    };
    draw_sprite(
      canvas,
      config.burst_sprite,
      &particle,
      config.color,
      config.color2,
    );
  }
}

/// Paints one already-resolved sprite. Keeping the dispatch in one place means
/// ambient and burst particles share the exact same look without duplicating
/// the match, and a new sprite only has to be added here.
fn draw_sprite(
  canvas: &mut Canvas,
  sprite: ParticleSprite,
  particle: &ParticleSample,
  color: Rgb,
  color2: Rgb,
) {
  let alpha = (particle.opacity * 255.0).round().clamp(0.0, 255.0) as u8;
  if alpha == 0 {
    return;
  }
  match sprite {
    ParticleSprite::Flame => draw_flame(canvas, particle, color, alpha),
    ParticleSprite::Butterfly => draw_butterfly(canvas, particle, color, color2, alpha),
    ParticleSprite::Dot => {
      canvas.fill_circle(
        (particle.x, particle.y),
        particle.size * 0.5,
        color.with_alpha(alpha),
      );
    }
    ParticleSprite::Streak => draw_streak(canvas, particle, color, alpha),
  }
}

/// The ember: a hot, slightly whitened core fading into the configured colour
/// and then into nothing. The three stops are enough to read as a soft glow
/// at 6 px without paying for a real gaussian.
fn draw_flame(canvas: &mut Canvas, particle: &ParticleSample, color: Rgb, alpha: u8) {
  let core = color.lighten(0.55).with_alpha(alpha);
  let middle = color.with_alpha((f32::from(alpha) * 0.55) as u8);
  let edge = color.with_alpha(0);
  canvas.radial_gradient(
    (particle.x, particle.y),
    particle.size,
    &[(0.0, core), (0.42, middle), (1.0, edge)],
  );
}

/// Two mirrored wings around a tiny body. The flap shrinks the horizontal wing
/// span, which is enough to read as motion at widget-scale sizes.
fn draw_butterfly(
  canvas: &mut Canvas,
  particle: &ParticleSample,
  color: Rgb,
  color2: Rgb,
  alpha: u8,
) {
  let (x, y) = (particle.x, particle.y);
  let wing_w = particle.size * 1.3 * particle.flap;
  let wing_h = particle.size;
  let left = color.with_alpha(alpha);
  let right = color2.with_alpha(alpha);
  canvas.fill_triangle(
    (x, y),
    (x - wing_w, y - wing_h * 0.7),
    (x - wing_w * 0.45, y + wing_h * 0.55),
    left,
  );
  canvas.fill_triangle(
    (x, y),
    (x + wing_w, y - wing_h * 0.7),
    (x + wing_w * 0.45, y + wing_h * 0.55),
    right,
  );
  canvas.fill_circle(
    (x, y),
    (particle.size * 0.14).max(0.5),
    color.lighten(0.4).with_alpha(alpha),
  );
}

/// A vertical light streak. It is a thin rectangle with a three-stop vertical
/// gradient (clear, hot, clear), which reads as a falling beam without a
/// texture or a shader. The width stays a fraction of the height so the same
/// sprite works at ambient ember scale and at burst scale.
fn draw_streak(canvas: &mut Canvas, particle: &ParticleSample, color: Rgb, alpha: u8) {
  let width = (particle.size * 0.16).max(1.0);
  let height = (particle.size * 1.6).max(2.0);
  let rect = Rect::new(
    particle.x - width / 2.0,
    particle.y - height / 2.0,
    width,
    height,
  );
  let hot = color.lighten(0.35).with_alpha(alpha);
  let edge = color.with_alpha(0);
  canvas.linear_gradient_stops(
    rect,
    width / 2.0,
    90.0,
    &[(0.0, edge), (0.5, hot), (1.0, edge)],
  );
}

#[cfg(test)]
mod tests {
  use super::{burst_sample, draw, sample};
  use crate::config::{
    BURST_LIFETIME, BurstTrigger, MAX_BURST_PARTICLES, MAX_PARTICLES, ParticleDrift, ParticleLayer,
    ParticleSprite, ParticlesConfig,
  };
  use crate::ui::canvas::Canvas;

  fn config(sprite: ParticleSprite, drift: ParticleDrift) -> ParticlesConfig {
    ParticlesConfig {
      enabled: true,
      count: 48,
      sprite,
      drift,
      ..ParticlesConfig::default()
    }
  }

  fn opaque_pixels(canvas: &Canvas) -> usize {
    canvas
      .pixmap()
      .pixels()
      .iter()
      .filter(|pixel| pixel.alpha() > 0)
      .count()
  }

  #[test]
  fn particles_stay_inside_the_screen_for_every_drift_and_sprite() {
    let screen = (320.0, 200.0);
    for sprite in [
      ParticleSprite::Flame,
      ParticleSprite::Butterfly,
      ParticleSprite::Dot,
      ParticleSprite::Streak,
    ] {
      for drift in [ParticleDrift::Up, ParticleDrift::Down, ParticleDrift::Float] {
        let config = config(sprite, drift);
        for step in 0..200 {
          let elapsed = step as f32 * 0.37;
          for index in 0..MAX_PARTICLES {
            let particle = sample(&config, index, elapsed, screen);
            assert!(
              particle.x >= 0.0 && particle.x <= screen.0,
              "{sprite:?} {drift:?} x={} at t={elapsed}",
              particle.x
            );
            assert!(
              particle.y >= 0.0 && particle.y <= screen.1,
              "{sprite:?} {drift:?} y={} at t={elapsed}",
              particle.y
            );
            assert!(particle.size.is_finite() && particle.size >= 0.5);
            assert!((0.0..=1.0).contains(&particle.opacity));
          }
        }
      }
    }
  }

  #[test]
  fn a_disabled_emitter_draws_nothing() {
    let config = ParticlesConfig {
      enabled: false,
      ..config(ParticleSprite::Flame, ParticleDrift::Up)
    };
    let mut canvas = Canvas::new(120, 90).expect("canvas");
    draw(
      &mut canvas,
      &config,
      (120.0, 90.0),
      4.0,
      ParticleLayer::Behind,
      None,
    );
    draw(
      &mut canvas,
      &config,
      (120.0, 90.0),
      4.0,
      ParticleLayer::Front,
      None,
    );
    assert_eq!(opaque_pixels(&canvas), 0);
  }

  #[test]
  fn the_cap_is_honoured_and_zero_draws_nothing() {
    let mut canvas = Canvas::new(120, 90).expect("canvas");
    let config = ParticlesConfig {
      count: MAX_PARTICLES,
      ..config(ParticleSprite::Dot, ParticleDrift::Float)
    };
    draw(
      &mut canvas,
      &config,
      (120.0, 90.0),
      2.0,
      ParticleLayer::Behind,
      None,
    );
    assert!(opaque_pixels(&canvas) > 0);

    let mut empty = Canvas::new(120, 90).expect("canvas");
    let zero = ParticlesConfig { count: 0, ..config };
    draw(
      &mut empty,
      &zero,
      (120.0, 90.0),
      2.0,
      ParticleLayer::Behind,
      None,
    );
    assert_eq!(opaque_pixels(&empty), 0);
  }

  #[test]
  fn a_bad_count_is_clamped_instead_of_overrunning_the_screen() {
    let config = ParticlesConfig {
      count: u32::MAX,
      ..config(ParticleSprite::Dot, ParticleDrift::Up)
    };
    let mut canvas = Canvas::new(60, 60).expect("canvas");
    draw(
      &mut canvas,
      &config,
      (60.0, 60.0),
      1.0,
      ParticleLayer::Behind,
      None,
    );
    // Reaching this point means the loop stopped at MAX_PARTICLES; also prove
    // the layer filter is real by drawing the front layer separately.
    let mut front_only = Canvas::new(60, 60).expect("canvas");
    let behind_only = ParticlesConfig {
      layer: ParticleLayer::Behind,
      ..config
    };
    draw(
      &mut front_only,
      &behind_only,
      (60.0, 60.0),
      1.0,
      ParticleLayer::Front,
      None,
    );
    assert_eq!(opaque_pixels(&front_only), 0);
  }

  #[test]
  fn a_streak_actually_paints_pixels() {
    let config = config(ParticleSprite::Streak, ParticleDrift::Up);
    let mut canvas = Canvas::new(160, 160).expect("canvas");
    draw(
      &mut canvas,
      &config,
      (160.0, 160.0),
      2.0,
      ParticleLayer::Behind,
      None,
    );
    assert!(
      opaque_pixels(&canvas) > 0,
      "the streak sprite must draw something"
    );
  }

  #[test]
  fn a_burst_spawns_exactly_its_count_once_and_then_dies() {
    let config = ParticlesConfig {
      enabled: true,
      burst_on: BurstTrigger::Success,
      burst_count: MAX_BURST_PARTICLES + 10,
      burst_sprite: ParticleSprite::Butterfly,
      ..ParticlesConfig::default()
    };
    let screen = (320.0, 200.0);
    // Exactly the capped count at age zero, and no more.
    let live = (0..MAX_BURST_PARTICLES + 20)
      .filter(|index| burst_sample(&config, *index, 0.0, screen).is_some())
      .count();
    assert_eq!(live as u32, MAX_BURST_PARTICLES);

    // Nothing is ever produced after the lifetime, and there is no hidden
    // per-frame accumulation to reset: the same age always gives the same
    // answer.
    assert!(burst_sample(&config, 0, BURST_LIFETIME, screen).is_none());
    assert!(burst_sample(&config, 0, BURST_LIFETIME + 5.0, screen).is_none());
    let first = burst_sample(&config, 7, 0.3, screen).expect("live sprite");
    let again = burst_sample(&config, 7, 0.3, screen).expect("same sprite");
    assert_eq!(first, again);
  }

  #[test]
  fn burst_sprites_stay_in_bounds_and_fade_out() {
    let config = ParticlesConfig {
      enabled: true,
      burst_on: BurstTrigger::Success,
      burst_count: 32,
      ..ParticlesConfig::default()
    };
    // A tiny screen forces the radial travel to clip at the edges.
    let screen = (48.0, 32.0);
    for step in 0..80 {
      let age = step as f32 * BURST_LIFETIME / 80.0;
      for index in 0..MAX_BURST_PARTICLES {
        let Some(particle) = burst_sample(&config, index, age, screen) else {
          continue;
        };
        assert!((0.0..=screen.0).contains(&particle.x));
        assert!((0.0..=screen.1).contains(&particle.y));
        assert!((0.0..=1.0).contains(&particle.opacity));
      }
    }
    // The last sample before the deadline is dimmer than the first one, so
    // the burst reads as a fade rather than a pop.
    let early = burst_sample(&config, 0, 0.01, screen).expect("early");
    let late = burst_sample(&config, 0, BURST_LIFETIME - 0.01, screen).expect("late");
    assert!(late.opacity < early.opacity);
  }

  #[test]
  fn an_off_burst_and_zero_count_are_silent() {
    let off = ParticlesConfig {
      enabled: true,
      burst_count: 32,
      ..ParticlesConfig::default()
    };
    assert!(burst_sample(&off, 0, 0.0, (100.0, 100.0)).is_none());

    let zero = ParticlesConfig {
      burst_on: BurstTrigger::Success,
      burst_count: 0,
      ..ParticlesConfig::default()
    };
    assert!(burst_sample(&zero, 0, 0.0, (100.0, 100.0)).is_none());
  }
}
