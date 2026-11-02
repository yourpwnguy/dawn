//! Build helpers for the UI and login tables.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn ui(&mut self, raw: &RawConfig, defaults: &Config) -> UiConfig {
    UiConfig {
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
    }
  }

  pub(super) fn login(
    &mut self,
    raw: &RawConfig,
    ui: &UiConfig,
    tokens: &ThemeConfig,
    defaults: &Config,
    base: Option<&Path>,
  ) -> LoginConfig {
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
    LoginConfig {
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
      prefill: self.prefill(raw.login.prefill.as_ref(), &login_defaults.prefill),
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
      splash: self.splash(&raw.login.splash),
      decor: self.decor(&raw.login.decor, base),
      transition: self.transition(&raw.login.transition),
    }
  }

  /// Reads `login.prefill`. The two keywords are `auto` and `none`; anything
  /// else is a literal username. A bad literal falls back to the default
  /// instead of guessing which account the author meant.
  fn prefill(&mut self, field: Option<&Field>, default: &UsernamePrefill) -> UsernamePrefill {
    let Some(field) = field else {
      return default.clone();
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "login.prefill",
        Some(field),
        "expected \"auto\", \"none\", or a username",
      );
      return default.clone();
    };
    match UsernamePrefill::parse(value) {
      Some(prefill) => prefill,
      None => {
        self.issue(
          "login.prefill",
          Some(field),
          "expected \"auto\", \"none\", or a username without control characters",
        );
        default.clone()
      }
    }
  }
}
