//! Build helper for the motion table.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn motion(&mut self, raw: &RawConfig, defaults: &Config) -> MotionConfig {
    MotionConfig {
      enabled: self.boolean(
        raw.motion.enabled.as_ref(),
        "motion.enabled",
        defaults.motion.enabled,
      ),
      reduced: self.boolean(
        raw.motion.reduced.as_ref(),
        "motion.reduced",
        defaults.motion.reduced,
      ),
      boot: Duration::from_millis(self.integer_range(
        raw.motion.boot_ms.as_ref(),
        "motion.boot_ms",
        defaults.motion.boot.as_millis() as u64,
        0,
        2000,
      )),
      focus: Duration::from_millis(self.integer_range(
        raw.motion.focus_ms.as_ref(),
        "motion.focus_ms",
        defaults.motion.focus.as_millis() as u64,
        0,
        1000,
      )),
      shake: Duration::from_millis(self.integer_range(
        raw.motion.shake_ms.as_ref(),
        "motion.shake_ms",
        defaults.motion.shake.as_millis() as u64,
        0,
        2000,
      )),
      drag: Duration::from_millis(self.integer_range(
        raw.motion.drag_ms.as_ref(),
        "motion.drag_ms",
        defaults.motion.drag.as_millis() as u64,
        0,
        1000,
      )),
      decor_fps: self.integer_range(
        raw.motion.decor_fps.as_ref(),
        "motion.decor_fps",
        u64::from(defaults.motion.decor_fps),
        10,
        60,
      ) as u32,
      parallax: self.range(
        raw.motion.parallax.as_ref(),
        "motion.parallax",
        f64::from(defaults.motion.parallax),
        0.0,
        2.0,
      ) as f32,
      beat: BeatConfig {
        bpm: self.beat_bpm(raw.motion.beat.bpm.as_ref(), defaults.motion.beat.bpm),
        offset: Duration::from_millis(self.integer_range(
          raw.motion.beat.offset_ms.as_ref(),
          "motion.beat.offset_ms",
          defaults.motion.beat.offset.as_millis() as u64,
          0,
          10000,
        )),
      },
      camera: CameraConfig {
        punch: self.range(
          raw.motion.camera.punch.as_ref(),
          "motion.camera.punch",
          f64::from(defaults.motion.camera.punch),
          0.0,
          0.2,
        ) as f32,
        punch_ms: self.range(
          raw.motion.camera.punch_ms.as_ref(),
          "motion.camera.punch_ms",
          f64::from(defaults.motion.camera.punch_ms),
          60.0,
          600.0,
        ) as f32,
        shake: self.range(
          raw.motion.camera.shake.as_ref(),
          "motion.camera.shake",
          f64::from(defaults.motion.camera.shake),
          0.0,
          40.0,
        ) as f32,
        shake_ms: self.range(
          raw.motion.camera.shake_ms.as_ref(),
          "motion.camera.shake_ms",
          f64::from(defaults.motion.camera.shake_ms),
          60.0,
          1000.0,
        ) as f32,
        breathing: self.range(
          raw.motion.camera.breathing.as_ref(),
          "motion.camera.breathing",
          f64::from(defaults.motion.camera.breathing),
          0.0,
          0.02,
        ) as f32,
        breath_ms: self.range(
          raw.motion.camera.breath_ms.as_ref(),
          "motion.camera.breath_ms",
          f64::from(defaults.motion.camera.breath_ms),
          1000.0,
          10000.0,
        ) as f32,
      },
      hitstop: HitstopConfig {
        submit: Duration::from_millis(self.integer_range(
          raw.motion.hitstop.submit_ms.as_ref(),
          "motion.hitstop.submit_ms",
          defaults.motion.hitstop.submit.as_millis() as u64,
          0,
          300,
        )),
        success: Duration::from_millis(self.integer_range(
          raw.motion.hitstop.success_ms.as_ref(),
          "motion.hitstop.success_ms",
          defaults.motion.hitstop.success.as_millis() as u64,
          0,
          500,
        )),
        failure: Duration::from_millis(self.integer_range(
          raw.motion.hitstop.failure_ms.as_ref(),
          "motion.hitstop.failure_ms",
          defaults.motion.hitstop.failure.as_millis() as u64,
          0,
          500,
        )),
      },
      post: PostConfig {
        speed_lines: self.range(
          raw.motion.post.speed_lines.as_ref(),
          "motion.post.speed_lines",
          f64::from(defaults.motion.post.speed_lines),
          0.0,
          1.0,
        ) as f32,
        speed_lines_on: self.choice(
          raw.motion.post.speed_lines_on.as_ref(),
          "motion.post.speed_lines_on",
          defaults.motion.post.speed_lines_on,
          SpeedLinesOn::parse,
          "beat, submit, or both",
        ),
        letterbox: self.range(
          raw.motion.post.letterbox.as_ref(),
          "motion.post.letterbox",
          f64::from(defaults.motion.post.letterbox),
          0.0,
          0.2,
        ) as f32,
        letterbox_animate_ms: self.range(
          raw.motion.post.letterbox_animate_ms.as_ref(),
          "motion.post.letterbox_animate_ms",
          f64::from(defaults.motion.post.letterbox_animate_ms),
          0.0,
          2000.0,
        ) as f32,
        vignette: self.range(
          raw.motion.post.vignette.as_ref(),
          "motion.post.vignette",
          f64::from(defaults.motion.post.vignette),
          0.0,
          1.0,
        ) as f32,
        chromatic: self.range(
          raw.motion.post.chromatic.as_ref(),
          "motion.post.chromatic",
          f64::from(defaults.motion.post.chromatic),
          0.0,
          1.0,
        ) as f32,
        glitch_ms: self.range(
          raw.motion.post.glitch_ms.as_ref(),
          "motion.post.glitch_ms",
          f64::from(defaults.motion.post.glitch_ms),
          0.0,
          600.0,
        ) as f32,
        glitch_on: self.choice(
          raw.motion.post.glitch_on.as_ref(),
          "motion.post.glitch_on",
          defaults.motion.post.glitch_on,
          GlitchOn::parse,
          "failure, submit, or both",
        ),
      },
    }
  }
}
