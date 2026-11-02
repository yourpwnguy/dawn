//! Sections validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn splash(&mut self, raw: &RawSplash) -> SplashConfig {
    let defaults = SplashConfig::default();
    let enabled = self.boolean(
      raw.enabled.as_ref(),
      "login.splash.enabled",
      defaults.enabled,
    );
    let title = self
      .string_value(
        raw.title.as_ref(),
        "login.splash.title",
        "expected a short title",
        64,
      )
      .unwrap_or_else(|| defaults.title.clone());
    if enabled && title.is_empty() {
      self.issue(
        "login.splash.title",
        raw.title.as_ref(),
        "enabled splash has no title, so it will show only the sub line",
      );
    }
    SplashConfig {
      enabled,
      ms: Duration::from_millis(self.integer_range(
        raw.ms.as_ref(),
        "login.splash.ms",
        defaults.ms.as_millis() as u64,
        500,
        10000,
      )),
      title,
      sub: self
        .string_value(
          raw.sub.as_ref(),
          "login.splash.sub",
          "expected a short subtitle",
          96,
        )
        .unwrap_or_else(|| defaults.sub.clone()),
      fade: Duration::from_millis(self.integer_range(
        raw.fade_ms.as_ref(),
        "login.splash.fade_ms",
        defaults.fade.as_millis() as u64,
        100,
        2000,
      )),
      dismiss_on: self.choice(
        raw.dismiss_on.as_ref(),
        "login.splash.dismiss_on",
        defaults.dismiss_on,
        SplashDismiss::parse,
        "timeout, enter, or any",
      ),
      bg: self.optional_color(raw.bg.as_ref(), "login.splash.bg"),
      accent: self.optional_color(raw.accent.as_ref(), "login.splash.accent"),
    }
  }

  /// Resolves `[login.transition]`.
  ///
  /// A cut list that ends up empty is replaced by the default three-colour
  /// flash rather than silently producing a sequence with no phase A. The slam
  /// text may be empty on purpose: that turns the sequence into a pure colour
  /// montage.
  pub(super) fn transition(&mut self, raw: &RawTransition) -> TransitionConfig {
    let defaults = TransitionConfig::default();
    TransitionConfig {
      enabled: self.boolean(
        raw.enabled.as_ref(),
        "login.transition.enabled",
        defaults.enabled,
      ),
      trigger: self.choice(
        raw.trigger.as_ref(),
        "login.transition.trigger",
        defaults.trigger,
        TransitionTrigger::parse,
        "enter, success, both, boot, or failure",
      ),
      cuts: self.color_list(
        raw.cuts.as_ref(),
        "login.transition.cuts",
        &defaults.cuts,
        8,
      ),
      cut_ms: self.range(
        raw.cut_ms.as_ref(),
        "login.transition.cut_ms",
        f64::from(defaults.cut_ms),
        0.0,
        1000.0,
      ) as f32,
      shake: self.range(
        raw.shake.as_ref(),
        "login.transition.shake",
        f64::from(defaults.shake),
        0.0,
        64.0,
      ) as f32,
      zoom: self.range(
        raw.zoom.as_ref(),
        "login.transition.zoom",
        f64::from(defaults.zoom),
        0.0,
        0.5,
      ) as f32,
      slam: self
        .string_value(
          raw.slam.as_ref(),
          "login.transition.slam",
          "expected a short line",
          96,
        )
        .unwrap_or_else(|| defaults.slam.clone()),
      slam_ms: self.range(
        raw.slam_ms.as_ref(),
        "login.transition.slam_ms",
        f64::from(defaults.slam_ms),
        0.0,
        10000.0,
      ) as f32,
      slam_color: self.color(
        raw.slam_color.as_ref(),
        "login.transition.slam_color",
        defaults.slam_color,
      ),
      slam_size: self.range(
        raw.slam_size.as_ref(),
        "login.transition.slam_size",
        f64::from(defaults.slam_size),
        8.0,
        240.0,
      ) as f32,
    }
  }

  /// Resolves `[particles]`. The count is clamped by the same cap the emitter
  /// enforces, so a huge value costs one warning and then stays sane.
  pub(super) fn particles(&mut self, raw: &RawParticles) -> ParticlesConfig {
    let defaults = ParticlesConfig::default();
    ParticlesConfig {
      enabled: self.boolean(raw.enabled.as_ref(), "particles.enabled", defaults.enabled),
      count: self.integer_range(
        raw.count.as_ref(),
        "particles.count",
        u64::from(defaults.count),
        0,
        u64::from(crate::config::MAX_PARTICLES),
      ) as u32,
      sprite: self.choice(
        raw.sprite.as_ref(),
        "particles.sprite",
        defaults.sprite,
        ParticleSprite::parse,
        "flame, butterfly, dot, or streak",
      ),
      color: self.color(raw.color.as_ref(), "particles.color", defaults.color),
      color2: self.color(raw.color2.as_ref(), "particles.color2", defaults.color2),
      drift: self.choice(
        raw.drift.as_ref(),
        "particles.drift",
        defaults.drift,
        ParticleDrift::parse,
        "up, down, or float",
      ),
      speed: self.range(
        raw.speed.as_ref(),
        "particles.speed",
        f64::from(defaults.speed),
        0.0,
        400.0,
      ) as f32,
      size: self.range(
        raw.size.as_ref(),
        "particles.size",
        f64::from(defaults.size),
        1.0,
        48.0,
      ) as f32,
      opacity: self.range(
        raw.opacity.as_ref(),
        "particles.opacity",
        f64::from(defaults.opacity),
        0.0,
        1.0,
      ) as f32,
      layer: self.choice(
        raw.layer.as_ref(),
        "particles.layer",
        defaults.layer,
        ParticleLayer::parse,
        "behind or front",
      ),
      burst_on: self.choice(
        raw.burst_on.as_ref(),
        "particles.burst_on",
        defaults.burst_on,
        crate::config::BurstTrigger::parse,
        "none or success",
      ),
      burst_sprite: self.choice(
        raw.burst_sprite.as_ref(),
        "particles.burst_sprite",
        defaults.burst_sprite,
        ParticleSprite::parse,
        "flame, butterfly, dot, or streak",
      ),
      burst_count: self.integer_range(
        raw.burst_count.as_ref(),
        "particles.burst_count",
        u64::from(defaults.burst_count),
        0,
        u64::from(crate::config::MAX_BURST_PARTICLES),
      ) as u32,
      burst_on_beat: self.boolean(
        raw.burst_on_beat.as_ref(),
        "particles.burst_on_beat",
        defaults.burst_on_beat,
      ),
    }
  }

  /// Resolves `[ticker]`. Items are capped before they reach the text engine;
  /// an enabled ticker with no usable lines is reported but stays harmless.
  pub(super) fn ticker(&mut self, raw: &RawTicker) -> TickerConfig {
    let defaults = TickerConfig::default();
    let enabled = self.boolean(raw.enabled.as_ref(), "ticker.enabled", defaults.enabled);
    let items = self.text_list(raw.items.as_ref(), "ticker.items", 8, 128);
    if enabled && items.is_empty() {
      self.issue(
        "ticker.items",
        raw.items.as_ref(),
        "enabled ticker has no usable items, so nothing will scroll",
      );
    }
    TickerConfig {
      enabled,
      items,
      separator: self
        .literal_value(
          raw.separator.as_ref(),
          "ticker.separator",
          "expected a short separator",
          32,
        )
        .unwrap_or_else(|| defaults.separator.clone()),
      anchor: self.choice(
        raw.anchor.as_ref(),
        "ticker.anchor",
        defaults.anchor,
        TickerAnchor::parse,
        "top or bottom",
      ),
      offset: self.offset(raw.offset.as_ref(), "ticker.offset", defaults.offset),
      size: self.range(
        raw.size.as_ref(),
        "ticker.size",
        f64::from(defaults.size),
        8.0,
        96.0,
      ) as f32,
      speed: self.range(
        raw.speed.as_ref(),
        "ticker.speed",
        f64::from(defaults.speed),
        0.0,
        400.0,
      ) as f32,
      color: self.color(raw.color.as_ref(), "ticker.color", defaults.color),
      bold: self.boolean(raw.bold.as_ref(), "ticker.bold", defaults.bold),
      tracking: self.range(
        raw.tracking.as_ref(),
        "ticker.tracking",
        f64::from(defaults.tracking),
        0.0,
        0.5,
      ) as f32,
      layer: self.widget_layer(raw.layer.as_ref(), "ticker.layer", defaults.layer),
      session_first: self.boolean(
        raw.session_first.as_ref(),
        "ticker.session_first",
        defaults.session_first,
      ),
      pulse_on_beat: self.boolean(
        raw.pulse_on_beat.as_ref(),
        "ticker.pulse_on_beat",
        defaults.pulse_on_beat,
      ),
      plate: self.plate(&raw.plate, "ticker.plate"),
    }
  }
}
