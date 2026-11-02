//! Build helpers for widget tables.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn clock(&mut self, raw: &RawConfig, defaults: &Config) -> ClockConfig {
    ClockConfig {
      enabled: self.boolean(
        raw.clock.enabled.as_ref(),
        "clock.enabled",
        defaults.clock.enabled,
      ),
      style: self.clock_style(raw.clock.style.as_ref(), defaults.clock.style),
      layer: self.widget_layer(
        raw.clock.layer.as_ref(),
        "clock.layer",
        defaults.clock.layer,
      ),
      look: self.widget_look(
        &raw.clock.plate,
        raw.clock.motion.as_ref(),
        raw.clock.show_on.as_ref(),
        raw.clock.hide_on.as_ref(),
        "clock",
      ),
      time_format: self.strftime(
        raw.clock.time_format.as_ref(),
        "clock.time_format",
        &defaults.clock.time_format,
        false,
      ),
      // An empty date format is a supported way to hide the date line.
      date_format: self.strftime(
        raw.clock.date_format.as_ref(),
        "clock.date_format",
        &defaults.clock.date_format,
        true,
      ),
      hour12: self.boolean(
        raw.clock.hour12.as_ref(),
        "clock.hour12",
        defaults.clock.hour12,
      ),
      seconds: self.boolean(
        raw.clock.seconds.as_ref(),
        "clock.seconds",
        defaults.clock.seconds,
      ),
      timezone: self.timezone(raw.clock.timezone.as_ref()),
      posix_locale: self.boolean(
        raw.clock.posix_locale.as_ref(),
        "clock.posix_locale",
        defaults.clock.posix_locale,
      ),
      placement: self.placement(
        raw.clock.anchor.as_ref(),
        raw.clock.offset.as_ref(),
        raw.clock.monitor.as_ref(),
        raw.clock.rotation.as_ref(),
        "clock",
        &defaults.clock.placement,
      ),
      size: self.range(
        raw.clock.size.as_ref(),
        "clock.size",
        f64::from(defaults.clock.size),
        12.0,
        180.0,
      ) as f32,
      date_size: self.range(
        raw.clock.date_size.as_ref(),
        "clock.date_size",
        f64::from(defaults.clock.date_size),
        8.0,
        48.0,
      ) as f32,
      color: self.optional_color(raw.clock.color.as_ref(), "clock.color"),
      date_color: self.optional_color(raw.clock.date_color.as_ref(), "clock.date_color"),
      label: self
        .string_value(
          raw.clock.label.as_ref(),
          "clock.label",
          "expected a short label",
          64,
        )
        .unwrap_or_else(|| defaults.clock.label.clone()),
      label_color: self.optional_color(raw.clock.label_color.as_ref(), "clock.label_color"),
      label_size: self.range(
        raw.clock.label_size.as_ref(),
        "clock.label_size",
        f64::from(defaults.clock.label_size),
        0.0,
        48.0,
      ) as f32,
      label_tracking: self.range(
        raw.clock.label_tracking.as_ref(),
        "clock.label_tracking",
        f64::from(defaults.clock.label_tracking),
        0.0,
        0.5,
      ) as f32,
      ghost: self.boolean(
        raw.clock.ghost.as_ref(),
        "clock.ghost",
        defaults.clock.ghost,
      ),
      glow: self.optional_color(raw.clock.glow.as_ref(), "clock.glow"),
      glow_size: self.range(
        raw.clock.glow_size.as_ref(),
        "clock.glow_size",
        f64::from(defaults.clock.glow_size),
        0.0,
        32.0,
      ) as f32,
      colon_blink_ms: self.range(
        raw.clock.colon_blink_ms.as_ref(),
        "clock.colon_blink_ms",
        f64::from(defaults.clock.colon_blink_ms),
        0.0,
        5000.0,
      ) as f32,
      second_hand: self.choice(
        raw.clock.second_hand.as_ref(),
        "clock.second_hand",
        defaults.clock.second_hand,
        SecondHand::parse,
        "tick or sweep",
      ),
      draggable: self.boolean(
        raw.clock.draggable.as_ref(),
        "clock.draggable",
        defaults.clock.draggable,
      ),
    }
  }
  pub(super) fn media(&mut self, raw: &RawConfig, defaults: &Config) -> MediaConfig {
    MediaConfig {
      enabled: self.boolean(
        raw.media.enabled.as_ref(),
        "media.enabled",
        defaults.media.enabled,
      ),
      placement: self.placement(
        raw.media.anchor.as_ref(),
        raw.media.offset.as_ref(),
        raw.media.monitor.as_ref(),
        raw.media.rotation.as_ref(),
        "media",
        &defaults.media.placement,
      ),
      size: self.range(
        raw.media.size.as_ref(),
        "media.size",
        f64::from(defaults.media.size),
        8.0,
        48.0,
      ) as f32,
      show_art: self.boolean(
        raw.media.show_art.as_ref(),
        "media.show_art",
        defaults.media.show_art,
      ),
      layer: self.widget_layer(
        raw.media.layer.as_ref(),
        "media.layer",
        defaults.media.layer,
      ),
      presentation: self.widget_look(
        &raw.media.plate,
        raw.media.motion.as_ref(),
        raw.media.show_on.as_ref(),
        raw.media.hide_on.as_ref(),
        "media",
      ),
      look: MediaLook {
        layout: self.choice(
          raw.media.layout.as_ref(),
          "media.layout",
          defaults.media.look.layout,
          MediaLayout::parse,
          "card, compact, or vinyl",
        ),
        art: self.choice(
          raw.media.art.as_ref(),
          "media.art",
          defaults.media.look.art,
          MediaArt::parse,
          "cover, disc, or none",
        ),
        progress: self.choice(
          raw.media.progress.as_ref(),
          "media.progress",
          defaults.media.look.progress,
          MediaProgress::parse,
          "line, segments, or wave",
        ),
        controls: self.boolean(
          raw.media.controls.as_ref(),
          "media.controls",
          defaults.media.look.controls,
        ),
        tint_from_art: self.boolean(
          raw.media.tint_from_art.as_ref(),
          "media.tint_from_art",
          defaults.media.look.tint_from_art,
        ),
      },
      color: self.optional_color(raw.media.color.as_ref(), "media.color"),
      show_before_login: self.boolean(
        raw.media.show_before_login.as_ref(),
        "media.show_before_login",
        defaults.media.show_before_login,
      ),
      keep_last: self.boolean(
        raw.media.keep_last.as_ref(),
        "media.keep_last",
        defaults.media.keep_last,
      ),
      draggable: self.boolean(
        raw.media.draggable.as_ref(),
        "media.draggable",
        defaults.media.draggable,
      ),
    }
  }
  pub(super) fn calendar(&mut self, raw: &RawConfig, defaults: &Config) -> CalendarConfig {
    CalendarConfig {
      enabled: self.boolean(
        raw.calendar.enabled.as_ref(),
        "calendar.enabled",
        defaults.calendar.enabled,
      ),
      placement: self.placement(
        raw.calendar.anchor.as_ref(),
        raw.calendar.offset.as_ref(),
        raw.calendar.monitor.as_ref(),
        raw.calendar.rotation.as_ref(),
        "calendar",
        &defaults.calendar.placement,
      ),
      size: self.range(
        raw.calendar.size.as_ref(),
        "calendar.size",
        f64::from(defaults.calendar.size),
        8.0,
        48.0,
      ) as f32,
      width: self.range(
        raw.calendar.width.as_ref(),
        "calendar.width",
        f64::from(defaults.calendar.width),
        120.0,
        800.0,
      ) as f32,
      week_start: self.choice(
        raw.calendar.week_start.as_ref(),
        "calendar.week_start",
        defaults.calendar.week_start,
        WeekStart::parse,
        "monday or sunday",
      ),
      show_header: self.boolean(
        raw.calendar.show_header.as_ref(),
        "calendar.show_header",
        defaults.calendar.show_header,
      ),
      color: self.color(
        raw.calendar.color.as_ref(),
        "calendar.color",
        defaults.calendar.color,
      ),
      muted_color: self.color(
        raw.calendar.muted_color.as_ref(),
        "calendar.muted_color",
        defaults.calendar.muted_color,
      ),
      today_color: self.color(
        raw.calendar.today_color.as_ref(),
        "calendar.today_color",
        defaults.calendar.today_color,
      ),
      header_color: self.color(
        raw.calendar.header_color.as_ref(),
        "calendar.header_color",
        defaults.calendar.header_color,
      ),
      layer: self.widget_layer(
        raw.calendar.layer.as_ref(),
        "calendar.layer",
        defaults.calendar.layer,
      ),
      look: self.widget_look(
        &raw.calendar.plate,
        raw.calendar.motion.as_ref(),
        raw.calendar.show_on.as_ref(),
        raw.calendar.hide_on.as_ref(),
        "calendar",
      ),
      draggable: self.boolean(
        raw.calendar.draggable.as_ref(),
        "calendar.draggable",
        defaults.calendar.draggable,
      ),
    }
  }
  pub(super) fn battery(&mut self, raw: &RawConfig, defaults: &Config) -> BatteryConfig {
    BatteryConfig {
      enabled: self.boolean(
        raw.battery.enabled.as_ref(),
        "battery.enabled",
        defaults.battery.enabled,
      ),
      placement: self.placement(
        raw.battery.anchor.as_ref(),
        raw.battery.offset.as_ref(),
        raw.battery.monitor.as_ref(),
        raw.battery.rotation.as_ref(),
        "battery",
        &defaults.battery.placement,
      ),
      size: self.range(
        raw.battery.size.as_ref(),
        "battery.size",
        f64::from(defaults.battery.size),
        8.0,
        48.0,
      ) as f32,
      width: self.range(
        raw.battery.width.as_ref(),
        "battery.width",
        f64::from(defaults.battery.width),
        120.0,
        800.0,
      ) as f32,
      style: self.choice(
        raw.battery.style.as_ref(),
        "battery.style",
        defaults.battery.style,
        BatteryStyle::parse,
        "ring, bar, or pill",
      ),
      color: self.color(
        raw.battery.color.as_ref(),
        "battery.color",
        defaults.battery.color,
      ),
      low_color: self.color(
        raw.battery.low_color.as_ref(),
        "battery.low_color",
        defaults.battery.low_color,
      ),
      charging_color: self.color(
        raw.battery.charging_color.as_ref(),
        "battery.charging_color",
        defaults.battery.charging_color,
      ),
      low_at: self.integer_range(
        raw.battery.low_at.as_ref(),
        "battery.low_at",
        u64::from(defaults.battery.low_at),
        0,
        100,
      ) as u8,
      show_percent: self.boolean(
        raw.battery.show_percent.as_ref(),
        "battery.show_percent",
        defaults.battery.show_percent,
      ),
      layer: self.widget_layer(
        raw.battery.layer.as_ref(),
        "battery.layer",
        defaults.battery.layer,
      ),
      look: self.widget_look(
        &raw.battery.plate,
        raw.battery.motion.as_ref(),
        raw.battery.show_on.as_ref(),
        raw.battery.hide_on.as_ref(),
        "battery",
      ),
      draggable: self.boolean(
        raw.battery.draggable.as_ref(),
        "battery.draggable",
        defaults.battery.draggable,
      ),
    }
  }
  pub(super) fn system(&mut self, raw: &RawConfig, defaults: &Config) -> SystemConfig {
    SystemConfig {
      enabled: self.boolean(
        raw.system.enabled.as_ref(),
        "system.enabled",
        defaults.system.enabled,
      ),
      placement: self.placement(
        raw.system.anchor.as_ref(),
        raw.system.offset.as_ref(),
        raw.system.monitor.as_ref(),
        raw.system.rotation.as_ref(),
        "system",
        &defaults.system.placement,
      ),
      size: self.range(
        raw.system.size.as_ref(),
        "system.size",
        f64::from(defaults.system.size),
        8.0,
        48.0,
      ) as f32,
      width: self.range(
        raw.system.width.as_ref(),
        "system.width",
        f64::from(defaults.system.width),
        120.0,
        800.0,
      ) as f32,
      style: self.choice(
        raw.system.style.as_ref(),
        "system.style",
        defaults.system.style,
        SystemStyle::parse,
        "rows or sparkline",
      ),
      fields: self.system_fields(
        raw.system.fields.as_ref(),
        "system.fields",
        &defaults.system.fields,
      ),
      color: self.color(
        raw.system.color.as_ref(),
        "system.color",
        defaults.system.color,
      ),
      label_color: self.color(
        raw.system.label_color.as_ref(),
        "system.label_color",
        defaults.system.label_color,
      ),
      layer: self.widget_layer(
        raw.system.layer.as_ref(),
        "system.layer",
        defaults.system.layer,
      ),
      look: self.widget_look(
        &raw.system.plate,
        raw.system.motion.as_ref(),
        raw.system.show_on.as_ref(),
        raw.system.hide_on.as_ref(),
        "system",
      ),
      draggable: self.boolean(
        raw.system.draggable.as_ref(),
        "system.draggable",
        defaults.system.draggable,
      ),
    }
  }
}
