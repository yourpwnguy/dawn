use super::{
  Motion, MotionFrame, beat_index_at, beat_phase_at, camera_sample, decor_beat_scale,
  ticker_beat_speed,
};
use crate::config::{BeatConfig, CameraConfig, Config, MotionConfig};
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
  assert!(frame.camera.is_identity());
  assert_eq!(frame.post.speed_lines, 0.0);
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

#[test]
fn beat_phase_matches_known_bpm_and_offset() {
  let beat = BeatConfig {
    bpm: 120.0,
    offset: Duration::from_millis(250),
  };
  // 120 BPM is exactly 500 ms per beat. At the offset the phase is zero,
  // halfway between beats it is 0.5, and one full period later it wraps.
  assert!((beat_phase_at(&beat, 0.0) - 0.5).abs() < 1e-6);
  assert!(beat_phase_at(&beat, 250.0) < 1e-6);
  assert!((beat_phase_at(&beat, 500.0) - 0.5).abs() < 1e-6);
  assert!(beat_phase_at(&beat, 750.0) < 1e-6);
  assert_eq!(beat_index_at(&beat, 0.0), 0);
  assert_eq!(beat_index_at(&beat, 749.0), 0);
  assert_eq!(beat_index_at(&beat, 750.0), 1);
}

#[test]
fn a_zero_bpm_beat_is_a_no_op() {
  let beat = BeatConfig::default();
  assert_eq!(beat.period_ms(), 0.0);
  assert_eq!(beat_phase_at(&beat, 1234.0), 0.0);
  assert_eq!(beat_index_at(&beat, 1234.0), 0);
}

#[test]
fn beat_pulse_multipliers_stay_inside_their_bounds() {
  for step in 0..=100 {
    let phase = step as f32 / 100.0;
    let scale = decor_beat_scale(phase);
    assert!((1.0..=1.06).contains(&scale));
    let speed = ticker_beat_speed(phase);
    assert!((1.0..=1.25).contains(&speed));
  }
  assert!((decor_beat_scale(0.0) - 1.06).abs() < 1e-6);
  assert!((ticker_beat_speed(0.0) - 1.25).abs() < 1e-6);
}

#[test]
fn camera_punch_decays_back_to_identity() {
  let camera = CameraConfig {
    punch: 0.1,
    punch_ms: 200.0,
    shake: 20.0,
    shake_ms: 100.0,
    ..CameraConfig::default()
  };
  let start = camera_sample(&camera, 0.0, Some(Duration::ZERO), Some(Duration::ZERO));
  assert!((start.scale - 1.1).abs() < 1e-6);
  let mid = camera_sample(&camera, 0.0, Some(Duration::from_millis(100)), None);
  assert!(mid.scale > 1.0 && mid.scale < 1.1);
  let end = camera_sample(
    &camera,
    0.0,
    Some(Duration::from_millis(200)),
    Some(Duration::from_millis(100)),
  );
  assert!(end.is_identity(), "{end:?}");
}

#[test]
fn camera_breathing_stays_inside_the_configured_amplitude() {
  let camera = CameraConfig {
    breathing: 0.02,
    breath_ms: 6000.0,
    ..CameraConfig::default()
  };
  for step in 0..400 {
    let value = camera_sample(&camera, step as f32 * 0.05, None, None);
    assert!((value.scale - 1.0).abs() <= 0.020_001);
    assert_eq!(value.dx, 0.0);
    assert_eq!(value.dy, 0.0);
  }
}

#[test]
fn speed_lines_open_on_submit_and_close_at_their_window() {
  let config = MotionConfig {
    post: crate::config::PostConfig {
      speed_lines: 1.0,
      speed_lines_on: crate::config::SpeedLinesOn::Submit,
      ..crate::config::PostConfig::default()
    },
    ..MotionConfig::default()
  };
  let mut motion = Motion::new(&config, false);
  let start = Instant::now();
  motion.frame(start, 1.0);
  motion.notify_moment(crate::config::MotionMoment::Submit, start);
  let open = motion.frame(start + Duration::from_millis(100), 1.0);
  assert!(open.post.speed_lines > 0.0);
  // The submit window is 900 ms at full intensity, so this is long closed.
  let closed = motion.frame(start + Duration::from_millis(1000), 1.0);
  assert_eq!(closed.post.speed_lines, 0.0);
}

#[test]
fn beat_speed_lines_reopen_on_each_beat() {
  let config = MotionConfig {
    beat: BeatConfig {
      bpm: 120.0,
      offset: Duration::ZERO,
    },
    post: crate::config::PostConfig {
      speed_lines: 0.5,
      speed_lines_on: crate::config::SpeedLinesOn::Beat,
      ..crate::config::PostConfig::default()
    },
    ..MotionConfig::default()
  };
  let mut motion = Motion::new(&config, false);
  let start = Instant::now();
  let first = motion.frame(start, 1.0);
  assert!(first.post.speed_lines > 0.0, "the first beat is open");
  // Just before the next beat the window (650 ms at 0.5) has expired.
  let late = motion.frame(start + Duration::from_millis(490), 1.0);
  assert!(late.post.speed_lines < first.post.speed_lines);
  let next = motion.frame(start + Duration::from_millis(500), 1.0);
  assert!(next.post.speed_lines > late.post.speed_lines);
}

#[test]
fn letterbox_animates_in_on_boot_and_out_on_success() {
  let config = MotionConfig {
    post: crate::config::PostConfig {
      letterbox: 0.1,
      letterbox_animate_ms: 200.0,
      ..crate::config::PostConfig::default()
    },
    ..MotionConfig::default()
  };
  let mut motion = Motion::new(&config, false);
  let start = Instant::now();
  let at_boot = motion.frame(start, 1.0);
  assert!(at_boot.post.letterbox < 0.001);
  let settled = motion.frame(start + Duration::from_millis(200), 1.0);
  assert!((settled.post.letterbox - 0.1).abs() < 1e-6);
  motion.notify_moment(
    crate::config::MotionMoment::Success,
    start + Duration::from_millis(300),
  );
  let leaving = motion.frame(start + Duration::from_millis(400), 1.0);
  assert!(leaving.post.letterbox > 0.0 && leaving.post.letterbox < 0.1);
  let gone = motion.frame(start + Duration::from_millis(500), 1.0);
  assert!(gone.post.letterbox < 1e-6);
}

#[test]
fn glitch_opens_only_for_its_trigger_and_window() {
  let config = MotionConfig {
    post: crate::config::PostConfig {
      glitch_ms: 100.0,
      glitch_on: crate::config::GlitchOn::Failure,
      ..crate::config::PostConfig::default()
    },
    ..MotionConfig::default()
  };
  let mut motion = Motion::new(&config, false);
  let start = Instant::now();
  motion.frame(start, 1.0);
  // A submit is not a failure, so the slices stay closed.
  motion.notify_moment(crate::config::MotionMoment::Submit, start);
  assert_eq!(
    motion
      .frame(start + Duration::from_millis(20), 1.0)
      .post
      .glitch,
    0.0
  );
  motion.notify_moment(
    crate::config::MotionMoment::Failure,
    start + Duration::from_millis(50),
  );
  let open = motion.frame(start + Duration::from_millis(60), 1.0);
  assert_eq!(open.post.glitch, 1.0);
  let closed = motion.frame(start + Duration::from_millis(160), 1.0);
  assert_eq!(closed.post.glitch, 0.0);
}

#[test]
fn breathing_requests_frames_without_dirtying_the_overlay() {
  let config = MotionConfig {
    boot: Duration::ZERO,
    focus: Duration::ZERO,
    shake: Duration::ZERO,
    camera: CameraConfig {
      breathing: 0.004,
      ..CameraConfig::default()
    },
    ..MotionConfig::default()
  };
  let mut motion = Motion::new(&config, false);
  let now = Instant::now();
  let _ = motion.frame(now, 1.0);
  assert!(motion.active(), "breathing must keep presenting frames");
  assert!(
    !motion.overlay_active(),
    "a camera transform must not force a CPU text repaint"
  );
  assert!(
    !motion.fast_active(),
    "breathing alone can use the half-rate cadence"
  );
}

#[test]
fn a_beat_clock_opts_the_overlay_back_in() {
  let config = MotionConfig {
    boot: Duration::ZERO,
    focus: Duration::ZERO,
    shake: Duration::ZERO,
    beat: BeatConfig {
      bpm: 136.0,
      offset: Duration::ZERO,
    },
    ..MotionConfig::default()
  };
  let mut motion = Motion::new(&config, false);
  let _ = motion.frame(Instant::now(), 1.0);
  assert!(motion.overlay_active(), "beat pulses change overlay pixels");
  assert!(
    !motion.fast_active(),
    "beat pulses can use the half-rate overlay cadence"
  );
}

#[test]
fn default_config_has_an_identity_camera_and_no_post_effects() {
  let config = Config::default();
  let mut motion = Motion::new(&config.motion, false);
  let frame = motion.frame(Instant::now(), 1.0);
  assert!(frame.camera.is_identity());
  assert_eq!(frame.post, MotionFrame::settled().post);
  assert_eq!(frame.beat_phase, 0.0);
  assert!(!config.login.splash.enabled);
  assert!(!config.particles.burst_on_beat);
  assert!(!config.ticker.pulse_on_beat);
}
