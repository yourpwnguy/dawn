//! Turns the permissive [`RawConfig`] into a validated [`Config`], recording a
//! precise issue for every value that had to fall back.
//!
//! The rule here is "degrade one field at a time". A typo in the accent colour
//! must not take the clock down with it, and a bad clock format must not throw
//! away the session directories. Only a file that cannot be parsed at all
//! falls back wholesale (handled in `mod.rs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use toml::Value;

use crate::color::Rgb;
use crate::config::raw::{
  Field, RawBackgroundOverride, RawConfig, RawField, RawGpu, RawWidgetPlate,
};
use crate::config::{
  AuthConfig, BackgroundConfig, BackgroundKind, BackgroundOverride, BlurConfig, ButtonEffect,
  CaretStyle, ClockConfig, ClockStyle, Config, DecorMotion, EnterFrom, EnterStyle, FieldConfig,
  GpuBackend, GpuConfig, GpuDevice, GpuFallback, GpuPower, GpuPresent, HintConfig, HoverReveal,
  Issue, LayoutConfig, Location, LoginConfig, MediaArt, MediaConfig, MediaLayout, MediaLook,
  MediaProgress, MotionConfig, PlateStyle, PluginsConfig, PowerConfig, SecondHand,
  SessionRowConfig, SessionsConfig, SurfaceStyle, ThemeConfig, ThemePreset, UiConfig, WidgetLayer,
  WidgetLook, WidgetPlate, WidgetState,
};
use crate::layout::{Anchor, Fit, Placement};

const MAX_PATH_CHARS: usize = 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

pub struct Validator<'a> {
  path: &'a Path,
  source: &'a str,
  issues: Vec<Issue>,
}

impl<'a> Validator<'a> {
  pub fn new(path: &'a Path, source: &'a str) -> Self {
    Self {
      path,
      source,
      issues: Vec::new(),
    }
  }

  pub fn into_issues(self) -> Vec<Issue> {
    self.issues
  }

  pub fn build(&mut self, raw: RawConfig, base: Option<&Path>) -> Config {
    let preset = self.preset(raw.theme.preset.as_ref());
    let tokens = preset.tokens();
    let defaults = Config::default();

    let ui = UiConfig {
      font: self.text(
        raw.ui.font.as_ref(),
        "ui.font",
        &defaults.ui.font,
        "expected a font family name",
        64,
      ),
      font_size: self.range(
        raw.ui.font_size.as_ref(),
        "ui.font_size",
        f64::from(defaults.ui.font_size),
        10.0,
        72.0,
      ) as f32,
      card_width: self.range(
        raw.ui.card_width.as_ref(),
        "ui.card_width",
        f64::from(defaults.ui.card_width),
        260.0,
        900.0,
      ) as f32,
      fullscreen: self.boolean(
        raw.ui.fullscreen.as_ref(),
        "ui.fullscreen",
        defaults.ui.fullscreen,
      ),
      greeting: self.text(
        raw.ui.greeting.as_ref(),
        "ui.greeting",
        &defaults.ui.greeting,
        "expected a short greeting",
        64,
      ),
    };

    let login_defaults = defaults.login.clone();
    let shared_field = self.field_config(
      &raw.login.field,
      &RawField::default(),
      "login.field",
      &login_defaults.field,
    );
    // `show_session` at the top level is a convenience alias for the more
    // specific `[login.session].show`. The specific key wins if both are set.
    let show_session = self.boolean(
      raw.login.show_session.as_ref(),
      "login.show_session",
      login_defaults.session.show,
    );
    let login = LoginConfig {
      layout: self.login_layout(raw.login.layout.as_ref(), login_defaults.layout),
      placement: self.placement(
        raw.login.anchor.as_ref(),
        raw.login.offset.as_ref(),
        raw.login.monitor.as_ref(),
        None,
        "login",
        &login_defaults.placement,
      ),
      width: self.range(
        raw.login.width.as_ref(),
        "login.width",
        f64::from(ui.card_width),
        260.0,
        2400.0,
      ) as f32,
      height: self.range(
        raw.login.height.as_ref(),
        "login.height",
        f64::from(login_defaults.height),
        0.0,
        2000.0,
      ) as f32,
      padding: self.range(
        raw.login.padding.as_ref(),
        "login.padding",
        f64::from(login_defaults.padding),
        0.0,
        80.0,
      ) as f32,
      gap: self.range(
        raw.login.gap.as_ref(),
        "login.gap",
        f64::from(login_defaults.gap),
        0.0,
        48.0,
      ) as f32,
      radius: self.range(
        raw.login.radius.as_ref(),
        "login.radius",
        f64::from(tokens.corner_radius),
        0.0,
        64.0,
      ) as f32,
      panel: self.optional_color(raw.login.panel.as_ref(), "login.panel"),
      panel_fade: self.fade_pair(
        raw.login.panel_fade.as_ref(),
        "login.panel_fade",
        login_defaults.panel_fade,
      ),
      border: self.optional_color(raw.login.border.as_ref(), "login.border"),
      surface: self.surface_style(raw.login.surface.as_ref(), login_defaults.surface),
      show_shadow: self.boolean(
        raw.login.show_shadow.as_ref(),
        "login.show_shadow",
        login_defaults.show_shadow,
      ),
      show_border: self.boolean(
        raw.login.show_border.as_ref(),
        "login.show_border",
        login_defaults.show_border,
      ),
      rotation: self.rotation(raw.login.rotation.as_ref(), "login.rotation", 0.0),
      draggable: self.boolean(
        raw.login.draggable.as_ref(),
        "login.draggable",
        login_defaults.draggable,
      ),
      greeting: self
        .string_value(
          raw.login.greeting.as_ref(),
          "login.greeting",
          "expected a short greeting",
          128,
        )
        .or_else(|| raw.login.greeting.is_none().then(|| ui.greeting.clone()))
        .unwrap_or_else(|| login_defaults.greeting.clone()),
      show_hints: self.boolean(
        raw.login.show_hints.as_ref(),
        "login.show_hints",
        login_defaults.show_hints,
      ),
      show_picker: self.boolean(
        raw.login.show_picker.as_ref(),
        "login.show_picker",
        login_defaults.show_picker,
      ),
      picker_title: self
        .string_value(
          raw.login.picker_title.as_ref(),
          "login.picker_title",
          "expected a short title",
          128,
        )
        .unwrap_or_else(|| login_defaults.picker_title.clone()),
      busy_text: self
        .string_value(
          raw.login.busy_text.as_ref(),
          "login.busy_text",
          "expected a short message",
          128,
        )
        .unwrap_or_else(|| login_defaults.busy_text.clone()),
      dry_run_badge: self
        .string_value(
          raw.login.dry_run_badge.as_ref(),
          "login.dry_run_badge",
          "expected a short label",
          64,
        )
        .unwrap_or_else(|| login_defaults.dry_run_badge.clone()),
      show_dry_run_badge: self.boolean(
        raw.login.show_dry_run_badge.as_ref(),
        "login.show_dry_run_badge",
        login_defaults.show_dry_run_badge,
      ),
      no_sessions: self
        .string_value(
          raw.login.no_sessions.as_ref(),
          "login.no_sessions",
          "expected a short message",
          160,
        )
        .unwrap_or_else(|| login_defaults.no_sessions.clone()),
      field: shared_field,
      username: self.field_config(
        &raw.login.field,
        &raw.login.username,
        "login.username",
        &login_defaults.username,
      ),
      password: self.field_config(
        &raw.login.field,
        &raw.login.password,
        "login.password",
        &login_defaults.password,
      ),
      session: SessionRowConfig {
        label: self
          .string_value(
            raw.login.session.label.as_ref(),
            "login.session.label",
            "expected a short label",
            64,
          )
          .unwrap_or_else(|| login_defaults.session.label.clone()),
        change_hint: self
          .string_value(
            raw.login.session.change_hint.as_ref(),
            "login.session.change_hint",
            "expected a short hint",
            64,
          )
          .unwrap_or_else(|| login_defaults.session.change_hint.clone()),
        empty: self
          .string_value(
            raw.login.session.empty.as_ref(),
            "login.session.empty",
            "expected a short message",
            128,
          )
          .unwrap_or_else(|| login_defaults.session.empty.clone()),
        show: self.boolean(
          raw.login.session.show.as_ref(),
          "login.session.show",
          show_session,
        ),
      },
      hints: HintConfig {
        username: self
          .string_value(
            raw.login.hints.username.as_ref(),
            "login.hints.username",
            "expected a short hint",
            160,
          )
          .unwrap_or_else(|| login_defaults.hints.username.clone()),
        password: self
          .string_value(
            raw.login.hints.password.as_ref(),
            "login.hints.password",
            "expected a short hint",
            160,
          )
          .unwrap_or_else(|| login_defaults.hints.password.clone()),
        picker: self
          .string_value(
            raw.login.hints.picker.as_ref(),
            "login.hints.picker",
            "expected a short hint",
            160,
          )
          .unwrap_or_else(|| login_defaults.hints.picker.clone()),
        busy: self
          .string_value(
            raw.login.hints.busy.as_ref(),
            "login.hints.busy",
            "expected a short hint",
            160,
          )
          .unwrap_or_else(|| login_defaults.hints.busy.clone()),
      },
      tab: self
        .string_value(
          raw.login.tab.as_ref(),
          "login.tab",
          "expected a short tab label",
          64,
        )
        .unwrap_or_else(|| login_defaults.tab.clone()),
      form_offset: self.offset(
        raw.login.form_offset.as_ref(),
        "login.form_offset",
        login_defaults.form_offset,
      ),
      form_width: self.range(
        raw.login.form_width.as_ref(),
        "login.form_width",
        f64::from(login_defaults.form_width),
        0.0,
        2400.0,
      ) as f32,
      button: self.button_config(&raw.login.button, &login_defaults.button),
      decor: self.decor(&raw.login.decor, base),
    };

    let layout = LayoutConfig {
      save: self.boolean(
        raw.layout.save.as_ref(),
        "layout.save",
        defaults.layout.save,
      ),
      file: self.path(raw.layout.file.as_ref(), "layout.file", base),
    };

    let clock = ClockConfig {
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
    };

    let media = MediaConfig {
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
    };

    let background = BackgroundConfig {
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
    };

    // Accepted for old configs. A dual Kawase pyramid does not have a single
    // downsample factor, so the value is validated but otherwise ignored.
    let _ = self.range(
      raw.theme.blur_downsample.as_ref(),
      "theme.blur_downsample",
      4.0,
      1.0,
      16.0,
    );
    let theme = ThemeConfig {
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
    };

    let motion = MotionConfig {
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
    };

    let power = PowerConfig {
      low_power: self.boolean(
        raw.power.low_power.as_ref(),
        "power.low_power",
        defaults.power.low_power,
      ),
    };

    let sessions = SessionsConfig {
      dirs: self.dirs(
        raw.sessions.dirs.as_ref(),
        "sessions.dirs",
        &defaults.sessions.dirs,
        base,
        true,
      ),
      default: self.optional_text(
        raw.sessions.default.as_ref(),
        "sessions.default",
        "expected a .desktop id",
        128,
      ),
    };

    let plugins = PluginsConfig {
      enabled: self.boolean(
        raw.plugins.enabled.as_ref(),
        "plugins.enabled",
        defaults.plugins.enabled,
      ),
      dirs: self.dirs(
        raw.plugins.dirs.as_ref(),
        "plugins.dirs",
        &defaults.plugins.dirs,
        base,
        false,
      ),
    };

    let auth = AuthConfig {
      socket: self
        .optional_text(
          raw.auth.socket.as_ref(),
          "auth.socket",
          "expected a socket path",
          MAX_PATH_CHARS,
        )
        .map(PathBuf::from),
      timeout: Duration::from_secs(self.integer_range(
        raw.auth.timeout_secs.as_ref(),
        "auth.timeout_secs",
        defaults.auth.timeout.as_secs(),
        5,
        600,
      )),
    };

    let gpu = self.gpu(&raw.gpu);

    Config {
      ui,
      login,
      clock,
      media,
      background,
      theme,
      motion,
      power,
      gpu,
      layout,
      sessions,
      plugins,
      auth,
    }
  }

  fn location(&self, field: &Field) -> Option<Location> {
    let (line, column) = crate::config::location_of(self.source, field.span().start)?;
    Some(Location {
      file: self.path.to_path_buf(),
      line,
      column,
    })
  }

  fn issue(&mut self, key: impl Into<String>, field: Option<&Field>, message: impl Into<String>) {
    self.issues.push(Issue {
      key: key.into(),
      location: field.and_then(|field| self.location(field)),
      message: message.into(),
    });
  }

  /// Validates the shape of a string value. Returns `None` when the field is
  /// absent or invalid; an empty string is a valid value here so callers that
  /// allow blank settings can decide what it means.
  fn string_value(
    &mut self,
    field: Option<&Field>,
    key: &str,
    expectation: &str,
    max_chars: usize,
  ) -> Option<String> {
    let field = field?;
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), expectation);
      return None;
    };
    let value = value.trim();
    if value.chars().count() > max_chars {
      self.issue(
        key,
        Some(field),
        format!("must be at most {max_chars} characters"),
      );
      return None;
    }
    if value.chars().any(char::is_control) {
      self.issue(key, Some(field), "must not contain control characters");
      return None;
    }
    Some(value.to_string())
  }

  fn text(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: &str,
    expectation: &str,
    max_chars: usize,
  ) -> String {
    let Some(value) = self.string_value(field, key, expectation, max_chars) else {
      return default.to_string();
    };
    if value.is_empty() && !default.is_empty() {
      self.issue(key, field, "must not be empty");
      return default.to_string();
    }
    value
  }

  fn optional_text(
    &mut self,
    field: Option<&Field>,
    key: &str,
    expectation: &str,
    max_chars: usize,
  ) -> Option<String> {
    let value = self.text(field, key, "", expectation, max_chars);
    (!value.is_empty()).then_some(value)
  }

  fn boolean(&mut self, field: Option<&Field>, key: &str, default: bool) -> bool {
    let Some(field) = field else {
      return default;
    };
    match field.get_ref().as_bool() {
      Some(value) => value,
      None => {
        self.issue(key, Some(field), "expected true or false");
        default
      }
    }
  }

  fn number(&mut self, field: &Field, key: &str) -> Option<f64> {
    let value = match field.get_ref() {
      Value::Integer(number) => *number as f64,
      Value::Float(number) if number.is_finite() => *number,
      _ => {
        self.issue(key, Some(field), "expected a number");
        return None;
      }
    };
    Some(value)
  }

  fn range(&mut self, field: Option<&Field>, key: &str, default: f64, min: f64, max: f64) -> f64 {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = self.number(field, key) else {
      return default;
    };
    if !(min..=max).contains(&value) {
      self.issue(
        key,
        Some(field),
        format!("must be between {min} and {max} (got {value})"),
      );
      return default;
    }
    value
  }

  fn integer_range(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
  ) -> u64 {
    let Some(field) = field else {
      return default;
    };
    let value = match field.get_ref() {
      Value::Integer(number) => u64::try_from(*number).ok(),
      Value::Float(number) if number.is_finite() && number.fract() == 0.0 && *number >= 0.0 => {
        Some(*number as u64)
      }
      _ => None,
    };
    let Some(value) = value else {
      self.issue(key, Some(field), "expected a whole number");
      return default;
    };
    if !(min..=max).contains(&value) {
      self.issue(
        key,
        Some(field),
        format!("must be between {min} and {max} (got {value})"),
      );
      return default;
    }
    value
  }

  /// A panel needs both dimensions. Missing or invalid values fall back to a
  /// sane plate instead of dropping the item, so one typo does not remove the
  /// backdrop that other decor was positioned against.
  fn panel_size(&mut self, field: Option<&Field>, key: &str, default: u64) -> f32 {
    let Some(field) = field else {
      self.issue(key, None, "panel needs a whole number from 4 to 4000");
      return default as f32;
    };
    self.integer_range(Some(field), key, default, 4, 4000) as f32
  }

  fn color(&mut self, field: Option<&Field>, key: &'static str, default: Rgb) -> Rgb {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        key,
        Some(field),
        "expected a hex colour such as \"#4c8dff\"",
      );
      return default;
    };
    match Rgb::parse(value) {
      Ok(color) => color,
      Err(error) => {
        self.issue(key, Some(field), error.to_string());
        default
      }
    }
  }

  fn optional_color(&mut self, field: Option<&Field>, key: &str) -> Option<Rgb> {
    let field = field?;
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        key,
        Some(field),
        "expected a hex colour such as \"#4c8dff\"",
      );
      return None;
    };
    match Rgb::parse(value) {
      Ok(color) => Some(color),
      Err(error) => {
        self.issue(key, Some(field), error.to_string());
        None
      }
    }
  }

  fn preset(&mut self, field: Option<&Field>) -> ThemePreset {
    let Some(field) = field else {
      return ThemePreset::default();
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "theme.preset",
        Some(field),
        "expected glass-dark, glass-light, or minimal",
      );
      return ThemePreset::default();
    };
    ThemePreset::parse(value).unwrap_or_else(|| {
      self.issue(
        "theme.preset",
        Some(field),
        format!("unknown preset {value:?}; use glass-dark, glass-light, or minimal"),
      );
      ThemePreset::default()
    })
  }

  fn clock_style(&mut self, field: Option<&Field>, default: ClockStyle) -> ClockStyle {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "clock.style",
        Some(field),
        "expected \"digital\", \"analog\", \"flip\", or \"segment\"",
      );
      return default;
    };
    ClockStyle::parse(value).unwrap_or_else(|| {
      self.issue(
        "clock.style",
        Some(field),
        format!("unknown style {value:?}; use digital, analog, flip, or segment"),
      );
      default
    })
  }

  fn background_kind(
    &mut self,
    field: Option<&Field>,
    default: BackgroundKind,
    key: &str,
  ) -> BackgroundKind {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        key,
        Some(field),
        "expected auto, solid, gradient, image, animated, or video",
      );
      return default;
    };
    BackgroundKind::parse(value).unwrap_or_else(|| {
      self.issue(
        key,
        Some(field),
        format!("unknown kind {value:?}; use auto, solid, gradient, image, animated, or video"),
      );
      default
    })
  }

  fn fit(&mut self, field: Option<&Field>, default: Fit, key: &str) -> Fit {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected cover, contain, or fill");
      return default;
    };
    Fit::parse(value).unwrap_or_else(|| {
      self.issue(
        key,
        Some(field),
        format!("unknown fit {value:?}; use cover, contain, or fill"),
      );
      default
    })
  }

  fn anchor(&mut self, field: Option<&Field>, key: &str, default: Anchor) -> Anchor {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected an anchor name");
      return default;
    };
    Anchor::parse(value).unwrap_or_else(|| {
      self.issue(
        key,
        Some(field),
        format!(
          "unknown anchor {value:?}; use top-left, top-center, top-right, center, \
           bottom-left, bottom-center, or bottom-right"
        ),
      );
      default
    })
  }

  fn offset(&mut self, field: Option<&Field>, key: &str, default: (i32, i32)) -> (i32, i32) {
    let Some(field) = field else {
      return default;
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(key, Some(field), "expected [x, y]");
      return default;
    };
    if values.len() != 2 {
      self.issue(key, Some(field), "expected exactly two numbers, [x, y]");
      return default;
    }
    let mut pair = [0i32; 2];
    for (index, value) in values.iter().enumerate() {
      let Some(number) = value.as_integer() else {
        self.issue(key, Some(field), "offset components must be whole numbers");
        return default;
      };
      let Ok(number) = i32::try_from(number) else {
        self.issue(key, Some(field), "offset component is out of range");
        return default;
      };
      pair[index] = number;
    }
    if pair.iter().any(|value| !(-2048..=4096).contains(value)) {
      self.issue(
        key,
        Some(field),
        "offset components must be between -2048 and 4096",
      );
      return default;
    }
    (pair[0], pair[1])
  }

  #[allow(clippy::too_many_arguments)]
  fn placement(
    &mut self,
    anchor: Option<&Field>,
    offset: Option<&Field>,
    monitor: Option<&Field>,
    rotation: Option<&Field>,
    scope: &'static str,
    default: &Placement,
  ) -> Placement {
    Placement {
      // The key names are static so the issue type stays cheap. The scope is
      // already encoded in the caller's key name below.
      anchor: self.anchor(anchor, placement_key(scope, 0), default.anchor),
      offset: self.offset(offset, placement_key(scope, 1), default.offset),
      monitor: self.optional_text(
        monitor,
        placement_key(scope, 2),
        "expected an output name",
        64,
      ),
      rotation: self.rotation(rotation, placement_key(scope, 3), default.rotation),
    }
  }

  fn login_layout(
    &mut self,
    field: Option<&Field>,
    default: crate::config::LoginLayout,
  ) -> crate::config::LoginLayout {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue("login.layout", Some(field), "expected a layout name");
      return default;
    };
    match value {
      "card" => crate::config::LoginLayout::Card,
      "hero" => crate::config::LoginLayout::Hero,
      _ => {
        self.issue(
          "login.layout",
          Some(field),
          format!("unknown layout {value:?}; use card or hero"),
        );
        default
      }
    }
  }

  /// Builds the Hyprland-style blur block. The old flat `blur_radius` and
  /// `blur_refresh_ms` keys are still accepted as aliases, because a config
  /// written for dawn 0.2 should not start blurring with a different radius
  /// after this change.
  fn blur_config(
    &mut self,
    raw: &crate::config::raw::RawBlur,
    legacy_radius: Option<&Field>,
    legacy_refresh: Option<&Field>,
    default: &BlurConfig,
  ) -> BlurConfig {
    let enabled = self.boolean(raw.enabled.as_ref(), "theme.blur.enabled", default.enabled);
    let size = if let Some(field) = raw.size.as_ref() {
      self.range(
        Some(field),
        "theme.blur.size",
        f64::from(default.size),
        0.0,
        64.0,
      ) as f32
    } else {
      self.range(
        legacy_radius,
        "theme.blur_radius",
        f64::from(default.size),
        0.0,
        64.0,
      ) as f32
    };
    let refresh_ms = if let Some(field) = raw.refresh_ms.as_ref() {
      self.integer_range(
        Some(field),
        "theme.blur.refresh_ms",
        default.refresh.as_millis() as u64,
        50,
        5000,
      )
    } else {
      self.integer_range(
        legacy_refresh,
        "theme.blur_refresh_ms",
        default.refresh.as_millis() as u64,
        50,
        5000,
      )
    };
    // Validate the compositor-only switches and intentionally discard them.
    let _ = self.boolean(
      raw.ignore_opacity.as_ref(),
      "theme.blur.ignore_opacity",
      true,
    );
    let _ = self.boolean(
      raw.new_optimizations.as_ref(),
      "theme.blur.new_optimizations",
      true,
    );
    let _ = self.boolean(raw.xray.as_ref(), "theme.blur.xray", false);
    let _ = self.boolean(raw.special.as_ref(), "theme.blur.special", false);
    let _ = self.boolean(raw.popups.as_ref(), "theme.blur.popups", false);
    let _ = self.boolean(
      raw.popup_ignore_alpha.as_ref(),
      "theme.blur.popup_ignore_alpha",
      false,
    );
    BlurConfig {
      enabled,
      size,
      passes: self.integer_range(
        raw.passes.as_ref(),
        "theme.blur.passes",
        u64::from(default.passes),
        1,
        8,
      ) as u32,
      noise: self.range(
        raw.noise.as_ref(),
        "theme.blur.noise",
        f64::from(default.noise),
        0.0,
        1.0,
      ) as f32,
      contrast: self.range(
        raw.contrast.as_ref(),
        "theme.blur.contrast",
        f64::from(default.contrast),
        0.0,
        2.0,
      ) as f32,
      brightness: self.range(
        raw.brightness.as_ref(),
        "theme.blur.brightness",
        f64::from(default.brightness),
        0.0,
        2.0,
      ) as f32,
      vibrancy: self.range(
        raw.vibrancy.as_ref(),
        "theme.blur.vibrancy",
        f64::from(default.vibrancy),
        0.0,
        1.0,
      ) as f32,
      vibrancy_darkness: self.range(
        raw.vibrancy_darkness.as_ref(),
        "theme.blur.vibrancy_darkness",
        f64::from(default.vibrancy_darkness),
        0.0,
        1.0,
      ) as f32,
      refresh: Duration::from_millis(refresh_ms),
    }
  }

  fn surface_style(&mut self, field: Option<&Field>, default: SurfaceStyle) -> SurfaceStyle {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.surface",
        Some(field),
        "expected glass, flat, or none",
      );
      return default;
    };
    match SurfaceStyle::parse(value) {
      Some(style) => style,
      None => {
        self.issue(
          "login.surface",
          Some(field),
          format!("unknown surface {value:?}; use glass, flat, or none"),
        );
        default
      }
    }
  }

  fn hover_reveal(&mut self, field: Option<&Field>, default: HoverReveal) -> HoverReveal {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.button.hover_direction",
        Some(field),
        "expected fade, left, right, up, down, or center",
      );
      return default;
    };
    match HoverReveal::parse(value) {
      Some(reveal) => reveal,
      None => {
        self.issue(
          "login.button.hover_direction",
          Some(field),
          format!("unknown direction {value:?}; use fade, left, right, up, down, or center"),
        );
        default
      }
    }
  }

  fn button_effect(&mut self, field: Option<&Field>, default: ButtonEffect) -> ButtonEffect {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.button.effect",
        Some(field),
        "expected none, sweep, sheen, pulse, glow, or corners",
      );
      return default;
    };
    match ButtonEffect::parse(value) {
      Some(effect) => effect,
      None => {
        self.issue(
          "login.button.effect",
          Some(field),
          format!("unknown effect {value:?}; use none, sweep, sheen, pulse, glow, or corners"),
        );
        default
      }
    }
  }

  fn caret_style(&mut self, field: Option<&Field>, key: &str, default: CaretStyle) -> CaretStyle {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected bar or block");
      return default;
    };
    match CaretStyle::parse(value) {
      Some(style) => style,
      None => {
        self.issue(
          key,
          Some(field),
          format!("unknown caret style {value:?}; use bar or block"),
        );
        default
      }
    }
  }

  /// Parses the built-in widget draw layer. The two values are deliberately
  /// named after the paint order, not after a z-index: `behind` is the
  /// historical order and `front` puts the widget between decor and the form.
  fn widget_layer(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: WidgetLayer,
  ) -> WidgetLayer {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected behind or front");
      return default;
    };
    match WidgetLayer::parse(value) {
      Some(layer) => layer,
      None => {
        self.issue(
          key,
          Some(field),
          format!("unknown widget layer {value:?}; use behind or front"),
        );
        default
      }
    }
  }

  /// Parses one widget plate table. Every key falls back independently, so a
  /// single typo recolours or reshapes one property instead of dropping the
  /// whole backplate. The defaults are the old scrim values.
  fn plate(&mut self, raw: &RawWidgetPlate, prefix: &str) -> WidgetPlate {
    let default = WidgetPlate::default();
    let key = |name: &str| format!("{prefix}.{name}");
    let style = match raw.style.as_ref() {
      None => default.style,
      Some(field) => match field.get_ref().as_str() {
        Some(text) => PlateStyle::parse(text).unwrap_or_else(|| {
          self.issue(
            key("style"),
            Some(field),
            format!("unknown plate style {text:?}; use none, tint, solid, or gradient"),
          );
          default.style
        }),
        None => {
          self.issue(key("style"), Some(field), "expected a plate style name");
          default.style
        }
      },
    };
    WidgetPlate {
      style,
      color: self.optional_color(raw.color.as_ref(), &key("color")),
      color_end: self.optional_color(raw.color_end.as_ref(), &key("color_end")),
      gradient_angle: self.range(
        raw.gradient_angle.as_ref(),
        &key("gradient_angle"),
        0.0,
        -360.0,
        360.0,
      ) as f32,
      radius: self.range(
        raw.radius.as_ref(),
        &key("radius"),
        f64::from(default.radius),
        0.0,
        64.0,
      ) as f32,
      border: self.optional_color(raw.border.as_ref(), &key("border")),
      padding: self.range(
        raw.padding.as_ref(),
        &key("padding"),
        f64::from(default.padding),
        0.0,
        64.0,
      ) as f32,
      glow: self.optional_color(raw.glow.as_ref(), &key("glow")),
      glow_size: self.range(raw.glow_size.as_ref(), &key("glow_size"), 0.0, 0.0, 48.0) as f32,
      shadow: self.boolean(raw.shadow.as_ref(), &key("shadow"), false),
      opacity: self.range(
        raw.opacity.as_ref(),
        &key("opacity"),
        f64::from(default.opacity),
        0.0,
        1.0,
      ) as f32,
    }
  }

  /// Parses `show_on` or `hide_on`.
  ///
  /// The lists are small and names are cheap, so duplicates are removed here
  /// rather than at draw time. Unknown names are reported once each and then
  /// skipped: one typo should not hide the whole widget. An empty list is a
  /// valid "always" for `show_on` and a valid "never hide" for `hide_on`.
  fn widget_states(&mut self, field: Option<&Field>, key: &str) -> Vec<WidgetState> {
    let Some(field) = field else {
      return Vec::new();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(
        key,
        Some(field),
        "expected an array of state names such as [\"typing\", \"busy\"]",
      );
      return Vec::new();
    };
    if values.len() > 8 {
      self.issue(key, Some(field), "accepts at most 8 states");
    }
    let mut states = Vec::new();
    let mut reported = Vec::new();
    for value in values.iter().take(8) {
      let Some(text) = value.as_str() else {
        self.issue(key, Some(field), "every state must be a string");
        continue;
      };
      match WidgetState::parse(text) {
        Some(state) => {
          if !states.contains(&state) {
            states.push(state);
          }
        }
        None => {
          if !reported.contains(&text) {
            self.issue(
              key,
              Some(field),
              format!("unknown state {text:?}; use idle, typing, busy, error, or authenticated"),
            );
            reported.push(text);
          }
        }
      }
    }
    states
  }

  /// Resolves the whole presentation block for one built-in widget.
  fn widget_look(
    &mut self,
    plate: &RawWidgetPlate,
    motion: Option<&crate::config::raw::RawDecorMotion>,
    show_on: Option<&Field>,
    hide_on: Option<&Field>,
    prefix: &str,
  ) -> WidgetLook {
    WidgetLook {
      plate: self.plate(plate, &format!("{prefix}.plate")),
      motion: self.decor_motion(motion, &format!("{prefix}.motion")),
      show_on: self.widget_states(show_on, &format!("{prefix}.show_on")),
      hide_on: self.widget_states(hide_on, &format!("{prefix}.hide_on")),
    }
  }

  fn gpu(&mut self, raw: &RawGpu) -> GpuConfig {
    let defaults = GpuConfig::default();
    GpuConfig {
      enabled: self.boolean(raw.enabled.as_ref(), "gpu.enabled", defaults.enabled),
      backend: self.choice(
        raw.backend.as_ref(),
        "gpu.backend",
        defaults.backend,
        GpuBackend::parse,
        "auto, vulkan, gl, or software",
      ),
      adapter: self.gpu_adapter(raw.adapter.as_ref(), &defaults.adapter),
      device: self.choice(
        raw.device.as_ref(),
        "gpu.device",
        defaults.device,
        GpuDevice::parse,
        "auto, discrete, integrated, virtual, or cpu",
      ),
      power: self.choice(
        raw.power.as_ref(),
        "gpu.power",
        defaults.power,
        GpuPower::parse,
        "low, high, or default",
      ),
      present: self.choice(
        raw.present.as_ref(),
        "gpu.present",
        defaults.present,
        GpuPresent::parse,
        "fifo, mailbox, or immediate",
      ),
      fallback: self.choice(
        raw.fallback.as_ref(),
        "gpu.fallback",
        defaults.fallback,
        GpuFallback::parse,
        "chain, software, cpu, or none",
      ),
    }
  }

  /// The shared shape of every enumerated choice key, not just `[gpu]`.
  /// Keeping the issue key caller-provided is what makes a typo report
  /// `gpu.present` or `media.layout` rather than a generic table error.
  fn choice<T: Copy>(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: T,
    parse: fn(&str) -> Option<T>,
    expected: &str,
  ) -> T {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), format!("expected {expected}"));
      return default;
    };
    match parse(value) {
      Some(choice) => choice,
      None => {
        self.issue(
          key,
          Some(field),
          format!("unknown value {value:?}; use {expected}"),
        );
        default
      }
    }
  }

  /// Adapter selection is intentionally looser than the other keys: a name may
  /// only exist on the machine running the greeter, so an unknown name is not a
  /// config error. Only the `index:N` syntax can be checked here.
  fn gpu_adapter(&mut self, field: Option<&Field>, default: &str) -> String {
    let Some(field) = field else {
      return default.to_string();
    };
    let Some(value) = self.string_value(
      Some(field),
      "gpu.adapter",
      "expected auto, index:N, or an adapter name",
      64,
    ) else {
      return default.to_string();
    };
    if value.is_empty() || value.eq_ignore_ascii_case("auto") {
      return "auto".to_string();
    }
    if let Some(rest) = value.strip_prefix("index:") {
      let rest = rest.trim();
      return match rest.parse::<u32>() {
        Ok(index) => format!("index:{index}"),
        Err(_) => {
          self.issue(
            "gpu.adapter",
            Some(field),
            format!("expected index:N with a whole number, got {value:?}"),
          );
          "auto".to_string()
        }
      };
    }
    value
  }

  fn rotation(&mut self, field: Option<&Field>, key: &str, default: f32) -> f32 {
    self.range(field, key, f64::from(default), -360.0, 360.0) as f32
  }

  fn fade_pair(&mut self, field: Option<&Field>, key: &str, default: (f32, f32)) -> (f32, f32) {
    let Some(field) = field else {
      return default;
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(key, Some(field), "expected [start, end] fractions");
      return default;
    };
    if values.len() != 2 {
      self.issue(
        key,
        Some(field),
        "expected exactly two numbers, [start, end]",
      );
      return default;
    }
    let mut pair = [0.0f32; 2];
    for (index, value) in values.iter().enumerate() {
      let Some(number) = value
        .as_float()
        .or_else(|| value.as_integer().map(|number| number as f64))
      else {
        self.issue(key, Some(field), "fade fractions must be numbers");
        return default;
      };
      if !(0.0..=1.0).contains(&number) {
        self.issue(key, Some(field), "fade fractions must be between 0 and 1");
        return default;
      }
      pair[index] = number as f32;
    }
    if pair[0] > pair[1] {
      self.issue(key, Some(field), "fade start must not be after fade end");
      return default;
    }
    (pair[0], pair[1])
  }

  fn button_config(
    &mut self,
    raw: &crate::config::raw::RawButton,
    default: &crate::config::ButtonConfig,
  ) -> crate::config::ButtonConfig {
    crate::config::ButtonConfig {
      show: self.boolean(raw.show.as_ref(), "login.button.show", default.show),
      text: self
        .string_value(
          raw.text.as_ref(),
          "login.button.text",
          "expected button text",
          64,
        )
        .unwrap_or_else(|| default.text.clone()),
      gap: self.range(
        raw.gap.as_ref(),
        "login.button.gap",
        f64::from(default.gap),
        0.0,
        160.0,
      ) as f32,
      height: self.range(
        raw.height.as_ref(),
        "login.button.height",
        f64::from(default.height),
        20.0,
        160.0,
      ) as f32,
      radius: self.range(
        raw.radius.as_ref(),
        "login.button.radius",
        f64::from(default.radius),
        0.0,
        64.0,
      ) as f32,
      font_size: self.range(
        raw.font_size.as_ref(),
        "login.button.font_size",
        f64::from(default.font_size),
        0.0,
        96.0,
      ) as f32,
      background: self.optional_color(raw.background.as_ref(), "login.button.background"),
      background_end: self
        .optional_color(raw.background_end.as_ref(), "login.button.background_end"),
      gradient_angle: self.range(
        raw.gradient_angle.as_ref(),
        "login.button.gradient_angle",
        f64::from(default.gradient_angle),
        -360.0,
        360.0,
      ) as f32,
      text_color: self.optional_color(raw.text_color.as_ref(), "login.button.text_color"),
      bracket: self.optional_color(raw.bracket.as_ref(), "login.button.bracket"),
      bracket_gap: self.range(
        raw.bracket_gap.as_ref(),
        "login.button.bracket_gap",
        f64::from(default.bracket_gap),
        0.0,
        64.0,
      ) as f32,
      bold: self.boolean(raw.bold.as_ref(), "login.button.bold", default.bold),
      tracking: self.range(
        raw.tracking.as_ref(),
        "login.button.tracking",
        f64::from(default.tracking),
        0.0,
        0.5,
      ) as f32,
      hover_background: self.optional_color(
        raw.hover_background.as_ref(),
        "login.button.hover_background",
      ),
      hover_background_end: self.optional_color(
        raw.hover_background_end.as_ref(),
        "login.button.hover_background_end",
      ),
      hover_text_color: self.optional_color(
        raw.hover_text_color.as_ref(),
        "login.button.hover_text_color",
      ),
      hover_direction: self.hover_reveal(raw.hover_direction.as_ref(), default.hover_direction),
      hover_ms: self.range(
        raw.hover_ms.as_ref(),
        "login.button.hover_ms",
        f64::from(default.hover_ms),
        0.0,
        5000.0,
      ) as f32,
      hover_scale: self.range(
        raw.hover_scale.as_ref(),
        "login.button.hover_scale",
        f64::from(default.hover_scale),
        0.5,
        2.0,
      ) as f32,
      hover_lift: self.range(
        raw.hover_lift.as_ref(),
        "login.button.hover_lift",
        f64::from(default.hover_lift),
        0.0,
        64.0,
      ) as f32,
      glow: self.optional_color(raw.glow.as_ref(), "login.button.glow"),
      glow_size: self.range(
        raw.glow_size.as_ref(),
        "login.button.glow_size",
        f64::from(default.glow_size),
        0.0,
        64.0,
      ) as f32,
      effect: self.button_effect(raw.effect.as_ref(), default.effect),
      effect_color: self.optional_color(raw.effect_color.as_ref(), "login.button.effect_color"),
      effect_ms: self.range(
        raw.effect_ms.as_ref(),
        "login.button.effect_ms",
        f64::from(default.effect_ms),
        100.0,
        20000.0,
      ) as f32,
      effect_angle: self.range(
        raw.effect_angle.as_ref(),
        "login.button.effect_angle",
        f64::from(default.effect_angle),
        -360.0,
        360.0,
      ) as f32,
      effect_strength: self.range(
        raw.effect_strength.as_ref(),
        "login.button.effect_strength",
        f64::from(default.effect_strength),
        0.0,
        1.0,
      ) as f32,
      rotation: self.rotation(
        raw.rotation.as_ref(),
        "login.button.rotation",
        default.rotation,
      ),
    }
  }

  fn decor(
    &mut self,
    raw: &[crate::config::raw::RawDecor],
    base: Option<&Path>,
  ) -> Vec<crate::config::DecorSpec> {
    const MAX_DECOR: usize = 32;
    if raw.len() > MAX_DECOR {
      self.issue(
        "login.decor",
        None,
        format!("at most {MAX_DECOR} decor items are supported"),
      );
    }
    let mut items = Vec::new();
    for (index, item) in raw.iter().take(MAX_DECOR).enumerate() {
      let prefix = format!("login.decor[{index}]");
      let kind = self
        .string_value(
          item.kind.as_ref(),
          &format!("{prefix}.kind"),
          "expected text, image, or panel",
          16,
        )
        .unwrap_or_default();
      match kind.as_str() {
        "text" => {
          let text = self
            .string_value(
              item.text.as_ref(),
              &format!("{prefix}.text"),
              "expected decor text",
              256,
            )
            .unwrap_or_default();
          if text.is_empty() {
            continue;
          }
          let stroke = self.optional_color(item.stroke.as_ref(), &format!("{prefix}.stroke"));
          let stroke_width = self.integer_range(
            item.stroke_width.as_ref(),
            &format!("{prefix}.stroke_width"),
            0,
            0,
            12,
          ) as f32;
          // A stroke colour without a width would look like a no-op in the
          // config. Give it a visible two-pixel default; `stroke_width` can
          // still raise it up to 12.
          let stroke_width = if stroke.is_some() && stroke_width <= 0.0 {
            2.0
          } else {
            stroke_width
          };
          items.push(crate::config::DecorSpec::Text {
            text,
            size: self.range(
              item.size.as_ref(),
              &format!("{prefix}.size"),
              14.0,
              6.0,
              200.0,
            ) as f32,
            color: self.optional_color(item.color.as_ref(), &format!("{prefix}.color")),
            bold: self.boolean(item.bold.as_ref(), &format!("{prefix}.bold"), false),
            tracking: self.range(
              item.tracking.as_ref(),
              &format!("{prefix}.tracking"),
              0.0,
              0.0,
              0.5,
            ) as f32,
            stroke,
            stroke_width,
            anchor: self.anchor(
              item.anchor.as_ref(),
              &format!("{prefix}.anchor"),
              Anchor::TopLeft,
            ),
            offset: self.offset(item.offset.as_ref(), &format!("{prefix}.offset"), (0, 0)),
            rotation: self.rotation(item.rotation.as_ref(), &format!("{prefix}.rotation"), 0.0),
            motion: self.decor_motion(item.motion.as_ref(), &format!("{prefix}.motion")),
          });
        }
        "image" => {
          let Some(path) = self.image(item.image.as_ref(), &format!("{prefix}.image"), base) else {
            continue;
          };
          items.push(crate::config::DecorSpec::Image {
            path,
            width: self.range(
              item.width.as_ref(),
              &format!("{prefix}.width"),
              64.0,
              4.0,
              2000.0,
            ) as f32,
            anchor: self.anchor(
              item.anchor.as_ref(),
              &format!("{prefix}.anchor"),
              Anchor::TopLeft,
            ),
            offset: self.offset(item.offset.as_ref(), &format!("{prefix}.offset"), (0, 0)),
            opacity: self.range(
              item.opacity.as_ref(),
              &format!("{prefix}.opacity"),
              1.0,
              0.0,
              1.0,
            ) as f32,
            rotation: self.rotation(item.rotation.as_ref(), &format!("{prefix}.rotation"), 0.0),
            motion: self.decor_motion(item.motion.as_ref(), &format!("{prefix}.motion")),
          });
        }
        "panel" => {
          // A panel is a plate, not an image or a text run. Accept either key
          // so an item can be switched from one kind to another without the
          // parser rejecting it, but tell the author exactly once that the
          // payload is being ignored and draw a bare plate.
          if item.text.is_some() || item.image.is_some() {
            self.issue(
              format!("{prefix}.kind"),
              item.kind.as_ref(),
              "panel ignores text and image; drawing a bare panel",
            );
          }
          items.push(crate::config::DecorSpec::Panel {
            width: self.panel_size(item.width.as_ref(), &format!("{prefix}.width"), 320),
            height: self.panel_size(item.height.as_ref(), &format!("{prefix}.height"), 120),
            color: self.optional_color(item.color.as_ref(), &format!("{prefix}.color")),
            color_end: self.optional_color(item.color_end.as_ref(), &format!("{prefix}.color_end")),
            gradient_angle: self.range(
              item.gradient_angle.as_ref(),
              &format!("{prefix}.gradient_angle"),
              0.0,
              -360.0,
              360.0,
            ) as f32,
            radius: self.range(
              item.radius.as_ref(),
              &format!("{prefix}.radius"),
              0.0,
              0.0,
              64.0,
            ) as f32,
            border: self.optional_color(item.border.as_ref(), &format!("{prefix}.border")),
            opacity: self.range(
              item.opacity.as_ref(),
              &format!("{prefix}.opacity"),
              1.0,
              0.0,
              1.0,
            ) as f32,
            anchor: self.anchor(
              item.anchor.as_ref(),
              &format!("{prefix}.anchor"),
              Anchor::TopLeft,
            ),
            offset: self.offset(item.offset.as_ref(), &format!("{prefix}.offset"), (0, 0)),
            rotation: self.rotation(item.rotation.as_ref(), &format!("{prefix}.rotation"), 0.0),
            motion: self.decor_motion(item.motion.as_ref(), &format!("{prefix}.motion")),
          });
        }
        _ => {
          self.issue(
            format!("{prefix}.kind"),
            item.kind.as_ref(),
            "expected text, image, or panel",
          );
        }
      }
    }
    items
  }

  /// Parses one decor item's optional motion table. Periods have usable
  /// defaults even when the config only sets an amplitude, so a sparse table
  /// like `float = [8, 4]` still works.
  fn decor_motion(
    &mut self,
    raw: Option<&crate::config::raw::RawDecorMotion>,
    prefix: &str,
  ) -> DecorMotion {
    let default = DecorMotion::default();
    let Some(raw) = raw else {
      return default;
    };
    let key = |name: &str| format!("{prefix}.{name}");
    let motion = DecorMotion {
      spin: self.range(
        raw.spin.as_ref(),
        &key("spin"),
        f64::from(default.spin),
        -720.0,
        720.0,
      ) as f32,
      sway: self.range(
        raw.sway.as_ref(),
        &key("sway"),
        f64::from(default.sway),
        0.0,
        45.0,
      ) as f32,
      sway_ms: self.range(
        raw.sway_ms.as_ref(),
        &key("sway_ms"),
        f64::from(default.sway_ms),
        200.0,
        20000.0,
      ) as f32,
      float: self.offset(raw.float.as_ref(), &key("float"), default.float),
      float_ms: self.range(
        raw.float_ms.as_ref(),
        &key("float_ms"),
        f64::from(default.float_ms),
        200.0,
        30000.0,
      ) as f32,
      pulse: self.range(
        raw.pulse.as_ref(),
        &key("pulse"),
        f64::from(default.pulse),
        0.0,
        0.5,
      ) as f32,
      pulse_ms: self.range(
        raw.pulse_ms.as_ref(),
        &key("pulse_ms"),
        f64::from(default.pulse_ms),
        200.0,
        20000.0,
      ) as f32,
      fade: self.range(
        raw.fade.as_ref(),
        &key("fade"),
        f64::from(default.fade),
        0.0,
        1.0,
      ) as f32,
      fade_ms: self.range(
        raw.fade_ms.as_ref(),
        &key("fade_ms"),
        f64::from(default.fade_ms),
        200.0,
        20000.0,
      ) as f32,
      phase: self.range(
        raw.phase.as_ref(),
        &key("phase"),
        f64::from(default.phase),
        0.0,
        1.0,
      ) as f32,
      typer_ms: self.range(
        raw.typer_ms.as_ref(),
        &key("typer_ms"),
        f64::from(default.typer_ms),
        0.0,
        20000.0,
      ) as f32,
      blink_ms: self.range(
        raw.blink_ms.as_ref(),
        &key("blink_ms"),
        f64::from(default.blink_ms),
        0.0,
        20000.0,
      ) as f32,
      ticker_ms: self.range(
        raw.ticker_ms.as_ref(),
        &key("ticker_ms"),
        f64::from(default.ticker_ms),
        0.0,
        120000.0,
      ) as f32,
      ticker_width: self.integer_range(
        raw.ticker_width.as_ref(),
        &key("ticker_width"),
        default.ticker_width as u64,
        0,
        4000,
      ) as i32,
      ticker_gap: self.integer_range(
        raw.ticker_gap.as_ref(),
        &key("ticker_gap"),
        default.ticker_gap as u64,
        0,
        2000,
      ) as i32,
      parallax_depth: self.range(
        raw.parallax_depth.as_ref(),
        &key("parallax_depth"),
        f64::from(default.parallax_depth),
        0.0,
        1.0,
      ) as f32,
      enter: self.choice(
        raw.enter.as_ref(),
        &key("enter"),
        default.enter,
        EnterStyle::parse,
        "none, fade, slide, or scale",
      ),
      enter_ms: self.range(
        raw.enter_ms.as_ref(),
        &key("enter_ms"),
        f64::from(default.enter_ms),
        0.0,
        20000.0,
      ) as f32,
      enter_delay_ms: self.range(
        raw.enter_delay_ms.as_ref(),
        &key("enter_delay_ms"),
        f64::from(default.enter_delay_ms),
        0.0,
        20000.0,
      ) as f32,
      enter_from: self.choice(
        raw.enter_from.as_ref(),
        &key("enter_from"),
        default.enter_from,
        EnterFrom::parse,
        "left, right, top, or bottom",
      ),
      wave: self.range(
        raw.wave.as_ref(),
        &key("wave"),
        f64::from(default.wave),
        0.0,
        40.0,
      ) as f32,
      wave_ms: self.range(
        raw.wave_ms.as_ref(),
        &key("wave_ms"),
        f64::from(default.wave_ms),
        200.0,
        20000.0,
      ) as f32,
      wave_step: self.range(
        raw.wave_step.as_ref(),
        &key("wave_step"),
        f64::from(default.wave_step),
        0.0,
        1.0,
      ) as f32,
      scramble_ms: self.range(
        raw.scramble_ms.as_ref(),
        &key("scramble_ms"),
        f64::from(default.scramble_ms),
        0.0,
        20000.0,
      ) as f32,
      glitch: self.range(
        raw.glitch.as_ref(),
        &key("glitch"),
        f64::from(default.glitch),
        0.0,
        1.0,
      ) as f32,
      glitch_ms: self.range(
        raw.glitch_ms.as_ref(),
        &key("glitch_ms"),
        f64::from(default.glitch_ms),
        200.0,
        20000.0,
      ) as f32,
    };
    // A scrolling ticker needs a window. Without one the item would silently
    // paint at its text width, which looks like a config typo rather than a
    // deliberate marquee, so tell the author exactly which key is missing.
    if motion.ticker_ms > 0.0 && motion.ticker_width <= 0 {
      self.issue(
        key("ticker_width"),
        raw.ticker_width.as_ref(),
        "ticker_ms needs ticker_width greater than 0",
      );
    }
    motion
  }

  /// Merges the shared `[login.field]` style with one field's overrides.
  ///
  /// Both tables are sparse. A value in `own` wins, otherwise the shared value
  /// is used, otherwise the built-in default. That is what lets a config say
  /// `placeholder = "User"` and inherit every pixel measurement.
  fn icon_name(&mut self, field: Option<&Field>, key: &str, default: &str) -> String {
    let Some(field) = field else {
      return default.to_string();
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), "expected user, lock, or none");
      return default.to_string();
    };
    match value {
      "" | "none" => String::new(),
      "user" | "lock" => value.to_string(),
      _ => {
        self.issue(key, Some(field), "expected user, lock, or none");
        default.to_string()
      }
    }
  }

  fn field_config(
    &mut self,
    shared: &RawField,
    own: &RawField,
    prefix: &str,
    default: &FieldConfig,
  ) -> FieldConfig {
    let key = |name: &str| format!("{prefix}.{name}");
    let text =
      |this: &mut Self, own: Option<&Field>, shared: Option<&Field>, name: &str, fallback: &str| {
        this
          .string_value(own.or(shared), &key(name), "expected a short string", 128)
          .unwrap_or_else(|| fallback.to_string())
      };
    FieldConfig {
      label: text(
        self,
        own.label.as_ref(),
        shared.label.as_ref(),
        "label",
        &default.label,
      ),
      label_color: self.optional_color(
        own.label_color.as_ref().or(shared.label_color.as_ref()),
        &key("label_color"),
      ),
      placeholder: text(
        self,
        own.placeholder.as_ref(),
        shared.placeholder.as_ref(),
        "placeholder",
        &default.placeholder,
      ),
      font_size: self.range(
        own.font_size.as_ref().or(shared.font_size.as_ref()),
        &key("font_size"),
        f64::from(default.font_size),
        0.0,
        96.0,
      ) as f32,
      height: self.range(
        own.height.as_ref().or(shared.height.as_ref()),
        &key("height"),
        f64::from(default.height),
        0.0,
        160.0,
      ) as f32,
      radius: self.range(
        own.radius.as_ref().or(shared.radius.as_ref()),
        &key("radius"),
        f64::from(default.radius),
        0.0,
        64.0,
      ) as f32,
      padding: self.range(
        own.padding.as_ref().or(shared.padding.as_ref()),
        &key("padding"),
        f64::from(default.padding),
        0.0,
        64.0,
      ) as f32,
      background: self.optional_color(
        own.background.as_ref().or(shared.background.as_ref()),
        &key("background"),
      ),
      background_end: self.optional_color(
        own
          .background_end
          .as_ref()
          .or(shared.background_end.as_ref()),
        &key("background_end"),
      ),
      border: self.optional_color(
        own.border.as_ref().or(shared.border.as_ref()),
        &key("border"),
      ),
      underline: self.optional_color(
        own.underline.as_ref().or(shared.underline.as_ref()),
        &key("underline"),
      ),
      underline_size: self.range(
        own
          .underline_size
          .as_ref()
          .or(shared.underline_size.as_ref()),
        &key("underline_size"),
        f64::from(default.underline_size),
        0.0,
        12.0,
      ) as f32,
      bracket: self.optional_color(
        own.bracket.as_ref().or(shared.bracket.as_ref()),
        &key("bracket"),
      ),
      bracket_gap: self.range(
        own.bracket_gap.as_ref().or(shared.bracket_gap.as_ref()),
        &key("bracket_gap"),
        f64::from(default.bracket_gap),
        0.0,
        64.0,
      ) as f32,
      text: self.optional_color(own.text.as_ref().or(shared.text.as_ref()), &key("text")),
      placeholder_color: self.optional_color(
        own
          .placeholder_color
          .as_ref()
          .or(shared.placeholder_color.as_ref()),
        &key("placeholder_color"),
      ),
      focus: self.optional_color(own.focus.as_ref().or(shared.focus.as_ref()), &key("focus")),
      caret: self.optional_color(own.caret.as_ref().or(shared.caret.as_ref()), &key("caret")),
      caret_width: self.range(
        own.caret_width.as_ref().or(shared.caret_width.as_ref()),
        &key("caret_width"),
        f64::from(default.caret_width),
        0.0,
        16.0,
      ) as f32,
      caret_style: self.caret_style(
        own.caret_style.as_ref().or(shared.caret_style.as_ref()),
        &key("caret_style"),
        default.caret_style,
      ),
      icon: self.icon_name(
        own.icon.as_ref().or(shared.icon.as_ref()),
        &key("icon"),
        &default.icon,
      ),
      icon_size: self.range(
        own.icon_size.as_ref().or(shared.icon_size.as_ref()),
        &key("icon_size"),
        f64::from(default.icon_size),
        0.0,
        64.0,
      ) as f32,
      icon_gap: self.range(
        own.icon_gap.as_ref().or(shared.icon_gap.as_ref()),
        &key("icon_gap"),
        f64::from(default.icon_gap),
        0.0,
        64.0,
      ) as f32,
      icon_color: self.optional_color(
        own.icon_color.as_ref().or(shared.icon_color.as_ref()),
        &key("icon_color"),
      ),
      rotation: self.rotation(
        own.rotation.as_ref().or(shared.rotation.as_ref()),
        &key("rotation"),
        default.rotation,
      ),
      mask: text(
        self,
        own.mask.as_ref(),
        shared.mask.as_ref(),
        "mask",
        &default.mask,
      ),
    }
  }

  /// Resolves a path without requiring it to exist. Used for the layout save
  /// file, which is created on the first drag.
  fn path(&mut self, field: Option<&Field>, key: &str, base: Option<&Path>) -> Option<PathBuf> {
    let raw = self.optional_text(field, key, "expected a file path", MAX_PATH_CHARS)?;
    match expand_path(&raw, base) {
      Ok(path) => Some(path),
      Err(message) => {
        self.issue(key, field, message);
        None
      }
    }
  }

  fn timezone(&mut self, field: Option<&Field>) -> Option<String> {
    let name = self.optional_text(
      field,
      "clock.timezone",
      "expected an IANA time zone such as \"Europe/Berlin\"",
      64,
    )?;
    match jiff::tz::TimeZone::get(&name) {
      Ok(_) => Some(name),
      Err(error) => {
        self.issue(
          "clock.timezone",
          field,
          format!("cannot load time zone {name:?}: {error}"),
        );
        None
      }
    }
  }

  fn strftime(
    &mut self,
    field: Option<&Field>,
    key: &'static str,
    default: &str,
    allow_empty: bool,
  ) -> String {
    let Some(value) = self.string_value(field, key, "expected an strftime format string", 64)
    else {
      return default.to_string();
    };
    if value.is_empty() {
      if allow_empty {
        return value;
      }
      self.issue(key, field, "must not be empty");
      return default.to_string();
    }
    if let Err(error) = jiff::fmt::strtime::format(&value, &jiff::Zoned::now()) {
      self.issue(key, field, format!("not a valid strftime format: {error}"));
      return default.to_string();
    }
    value
  }

  fn image(&mut self, field: Option<&Field>, key: &str, base: Option<&Path>) -> Option<PathBuf> {
    let raw = self.optional_text(
      field,
      key,
      "expected a path to a PNG, JPEG, GIF, WebP, or MP4 file",
      MAX_PATH_CHARS,
    )?;
    let path = match expand_path(&raw, base) {
      Ok(path) => path,
      Err(message) => {
        self.issue(key, field, message);
        return None;
      }
    };
    // Video files are allowed to be much larger than still images. Keep the
    // still limit strict, because a 1 GiB PNG is almost always a mistake, but
    // a few minutes of 1080p MP4 is normal.
    let video = has_video_extension(&path);
    let limit = if video {
      8 * 1024 * 1024 * 1024
    } else {
      MAX_IMAGE_BYTES
    };
    match std::fs::metadata(&path) {
      Ok(metadata) if !metadata.is_file() => {
        self.issue(
          key,
          field,
          format!(
            "{} is not a regular file (fix: point {key} at an image or video file, or remove it)",
            path.display()
          ),
        );
        None
      }
      Ok(metadata) if metadata.len() > limit => {
        let what = if video { "video" } else { "image" };
        self.issue(
          key,
          field,
          format!(
            "{} is larger than the {} MiB {what} limit (fix: downscale or shorten it)",
            path.display(),
            limit / 1024 / 1024
          ),
        );
        None
      }
      Ok(_) => Some(path),
      Err(error) => {
        self.issue(
          key,
          field,
          format!(
            "cannot read {}: {error} (fix: correct {key} or make the file readable)",
            path.display()
          ),
        );
        None
      }
    }
  }

  fn background_monitors(
    &mut self,
    raw: &BTreeMap<String, RawBackgroundOverride>,
    base: Option<&Path>,
  ) -> BTreeMap<String, BackgroundOverride> {
    if raw.len() > 16 {
      self.issue(
        "background.monitors",
        None,
        "at most 16 per-monitor overrides are supported",
      );
    }
    let mut monitors = BTreeMap::new();
    for (name, over) in raw.iter().take(16) {
      if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        self.issue(
          "background.monitors",
          None,
          format!("ignoring output {name:?}: names must be 1-64 printable characters"),
        );
        continue;
      }
      let key = format!("background.monitors.{name}");
      let kind = over.kind.as_ref().map(|field| {
        self.background_kind(Some(field), BackgroundKind::Auto, &format!("{key}.kind"))
      });
      let color = self.optional_color(over.color.as_ref(), &format!("{key}.color"));
      let image = self.image(over.image.as_ref(), &format!("{key}.image"), base);
      let fit = over
        .fit
        .as_ref()
        .map(|field| self.fit(Some(field), Fit::Cover, &format!("{key}.fit")));
      monitors.insert(
        name.clone(),
        BackgroundOverride {
          kind,
          color,
          image,
          fit,
        },
      );
    }
    monitors
  }

  fn dirs(
    &mut self,
    field: Option<&Field>,
    key: &'static str,
    defaults: &[PathBuf],
    base: Option<&Path>,
    require_one: bool,
  ) -> Vec<PathBuf> {
    let Some(field) = field else {
      return defaults.to_vec();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(key, Some(field), "expected a list of directory paths");
      return defaults.to_vec();
    };
    if values.len() > 8 {
      self.issue(key, Some(field), "at most 8 directories are supported");
      return defaults.to_vec();
    }

    let mut resolved = Vec::with_capacity(values.len());
    for value in values {
      let Some(value) = value.as_str() else {
        self.issue(key, Some(field), "every entry must be a string");
        return defaults.to_vec();
      };
      match expand_path(value, base) {
        Ok(path) => resolved.push(path),
        Err(message) => {
          self.issue(key, Some(field), message);
          return defaults.to_vec();
        }
      }
    }

    if resolved.is_empty() && require_one {
      self.issue(key, Some(field), "must contain at least one directory");
      return defaults.to_vec();
    }
    resolved
  }
}

/// The issue key for a placement field. Keeping these in one place means a
/// typo in one section does not get reported under another section's name.
fn placement_key(scope: &str, index: usize) -> &'static str {
  match (scope, index) {
    ("clock", 0) => "clock.anchor",
    ("clock", 1) => "clock.offset",
    ("clock", 2) => "clock.monitor",
    ("clock", _) => "clock.rotation",
    ("media", 0) => "media.anchor",
    ("media", 1) => "media.offset",
    ("media", 2) => "media.monitor",
    ("media", _) => "media.rotation",
    ("login", 0) => "login.anchor",
    ("login", 1) => "login.offset",
    ("login", 2) => "login.monitor",
    _ => "login.rotation",
  }
}

/// Video container extensions. Kept here instead of calling into the UI so the
/// config layer stays independent of the renderer.
fn has_video_extension(path: &Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "mpg" | "mpeg"
      )
    })
}

/// Expands `~` and resolves relative paths against the config file's directory.
/// The path is not required to exist: desktop and plugin directories may be
/// created later, and that should be a quiet no-op rather than a config error.
fn expand_path(raw: &str, base: Option<&Path>) -> Result<PathBuf, String> {
  let raw = raw.trim();
  if raw.is_empty() {
    return Err("path must not be empty".to_string());
  }
  if raw.chars().count() > MAX_PATH_CHARS {
    return Err(format!("path must be at most {MAX_PATH_CHARS} characters"));
  }
  if raw.chars().any(char::is_control) {
    return Err("path must not contain control characters".to_string());
  }
  if let Some(rest) = raw.strip_prefix("~/") {
    let home = std::env::var_os("HOME")
      .filter(|value| !value.is_empty())
      .ok_or_else(|| "cannot expand '~': HOME is not set".to_string())?;
    return Ok(PathBuf::from(home).join(rest));
  }
  if raw == "~" {
    return std::env::var_os("HOME")
      .filter(|value| !value.is_empty())
      .map(PathBuf::from)
      .ok_or_else(|| "cannot expand '~': HOME is not set".to_string());
  }
  let path = PathBuf::from(raw);
  if path.is_relative()
    && let Some(base) = base
  {
    return Ok(base.join(path));
  }
  Ok(path)
}

#[cfg(test)]
mod tests {
  use super::expand_path;

  #[test]
  fn relative_paths_use_the_config_directory() {
    let base = std::path::Path::new("/etc/dawn");
    assert_eq!(
      expand_path("wallpapers/a.png", Some(base)).unwrap(),
      std::path::PathBuf::from("/etc/dawn/wallpapers/a.png")
    );
  }

  #[test]
  fn tilde_uses_home_when_present() {
    // HOME is set in the test environment; skip gracefully if it is not.
    if std::env::var_os("HOME").is_some() {
      let path = expand_path("~/x", None).unwrap();
      assert!(path.is_absolute());
      assert!(path.ends_with("x"));
    }
  }

  #[test]
  fn empty_paths_are_rejected() {
    assert!(expand_path("   ", None).is_err());
  }
}
