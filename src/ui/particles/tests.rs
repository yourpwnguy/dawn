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
fn burst_on_beat_arms_the_same_capped_emitter() {
  let config = ParticlesConfig {
    enabled: true,
    burst_on: BurstTrigger::None,
    burst_on_beat: true,
    burst_count: 8,
    ..ParticlesConfig::default()
  };
  assert!(burst_sample(&config, 0, 0.0, (100.0, 100.0)).is_some());
  // The existing cap still applies even though no count was clamped.
  let over = ParticlesConfig {
    burst_count: MAX_BURST_PARTICLES + 12,
    ..config
  };
  let live = (0..MAX_BURST_PARTICLES + 20)
    .filter(|index| burst_sample(&over, *index, 0.0, (100.0, 100.0)).is_some())
    .count();
  assert_eq!(live as u32, MAX_BURST_PARTICLES);
  // And the trigger still dies on schedule.
  assert!(burst_sample(&config, 0, BURST_LIFETIME, (100.0, 100.0)).is_none());
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
