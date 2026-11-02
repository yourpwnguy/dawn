//! Build helpers for layout, wallpaper, and theme tables.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn layout(
    &mut self,
    raw: &RawConfig,
    defaults: &Config,
    base: Option<&Path>,
  ) -> LayoutConfig {
    LayoutConfig {
      save: self.boolean(
        raw.layout.save.as_ref(),
        "layout.save",
        defaults.layout.save,
      ),
      file: self.path(raw.layout.file.as_ref(), "layout.file", base),
    }
  }

  pub(super) fn background(
    &mut self,
    raw: &RawConfig,
    preset: ThemePreset,
    defaults: &Config,
    base: Option<&Path>,
  ) -> BackgroundConfig {
    BackgroundConfig {
      kind: self.background_kind(
        raw.background.kind.as_ref(),
        defaults.background.kind,
        "background.kind",
      ),
      // The preset supplies a matching base colour when the user did not set
      // one. A light glass theme with the dark default background would look
      // broken with zero config.
      color: self.color(
        raw.background.color.as_ref(),
        "background.color",
        preset.background_color(),
      ),
      gradient: self.boolean(
        raw.background.gradient.as_ref(),
        "background.gradient",
        defaults.background.gradient,
      ),
      image: self.image(raw.background.image.as_ref(), "background.image", base),
      audio: self.boolean(
        raw.background.audio.as_ref(),
        "background.audio",
        defaults.background.audio,
      ),
      fit: self.fit(
        raw.background.fit.as_ref(),
        defaults.background.fit,
        "background.fit",
      ),
      fps: self.integer_range(
        raw.background.fps.as_ref(),
        "background.fps",
        u64::from(defaults.background.fps),
        0,
        240,
      ) as u32,
      memory_limit: self.integer_range(
        raw.background.memory_limit_mb.as_ref(),
        "background.memory_limit_mb",
        defaults.background.memory_limit / 1024 / 1024,
        16,
        1024,
      ) * 1024
        * 1024,
      idle_pause: Duration::from_secs(self.integer_range(
        raw.background.idle_pause_secs.as_ref(),
        "background.idle_pause_secs",
        defaults.background.idle_pause.as_secs(),
        0,
        3600,
      )),
      monitors: self.background_monitors(&raw.background.monitors, base),
    }
  }

  pub(super) fn theme(&mut self, raw: &RawConfig, tokens: &ThemeConfig) -> ThemeConfig {
    // Accepted for old configs. A dual Kawase pyramid does not have a single
    // downsample factor, so the value is validated but otherwise ignored.
    let _ = self.range(
      raw.theme.blur_downsample.as_ref(),
      "theme.blur_downsample",
      4.0,
      1.0,
      16.0,
    );
    ThemeConfig {
      preset: tokens.preset,
      text: self.color(raw.theme.text.as_ref(), "theme.text", tokens.text),
      muted: self.color(raw.theme.muted.as_ref(), "theme.muted", tokens.muted),
      accent: self.color(raw.theme.accent.as_ref(), "theme.accent", tokens.accent),
      error: self.color(raw.theme.error.as_ref(), "theme.error", tokens.error),
      tint: self.color(
        raw.theme.tint.as_ref().or(raw.theme.card.as_ref()),
        "theme.tint",
        tokens.tint,
      ),
      border: self.color(raw.theme.border.as_ref(), "theme.border", tokens.border),
      shadow: self.color(raw.theme.shadow.as_ref(), "theme.shadow", tokens.shadow),
      focus_ring: self.color(
        raw.theme.focus_ring.as_ref(),
        "theme.focus_ring",
        tokens.focus_ring,
      ),
      blur: self.blur_config(
        &raw.theme.blur,
        raw.theme.blur_radius.as_ref(),
        raw.theme.blur_refresh_ms.as_ref(),
        &tokens.blur,
      ),
      corner_radius: self.range(
        raw.theme.corner_radius.as_ref(),
        "theme.corner_radius",
        f64::from(tokens.corner_radius),
        0.0,
        64.0,
      ) as f32,
      spacing: self.range(
        raw.theme.spacing.as_ref(),
        "theme.spacing",
        f64::from(tokens.spacing),
        0.75,
        2.0,
      ) as f32,
      noise: self.boolean(raw.theme.noise.as_ref(), "theme.noise", tokens.noise),
      min_contrast: self.range(
        raw.theme.min_contrast.as_ref(),
        "theme.min_contrast",
        f64::from(tokens.min_contrast),
        1.0,
        21.0,
      ) as f32,
      scanlines: self.range(
        raw.theme.scanlines.as_ref(),
        "theme.scanlines",
        f64::from(tokens.scanlines),
        0.0,
        1.0,
      ) as f32,
      scanline_gap: self.integer_range(
        raw.theme.scanline_gap.as_ref(),
        "theme.scanline_gap",
        u64::from(tokens.scanline_gap),
        2,
        12,
      ) as u32,
      scanline_ms: self.range(
        raw.theme.scanline_ms.as_ref(),
        "theme.scanline_ms",
        f64::from(tokens.scanline_ms),
        1000.0,
        20000.0,
      ) as f32,
    }
  }
}
