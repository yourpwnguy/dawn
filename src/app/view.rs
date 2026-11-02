//! View methods for the login app.

use super::*;

impl App {
  pub fn view(&self) -> View {
    let visible_answer = self.prompt.is_some() && !self.answer_secret;
    let mask = &self.theme.login.password.mask;
    let now = Instant::now();
    let splash = self.splash.as_ref().and_then(|splash| {
      splash.view(
        now,
        self.motion.beat_phase(now),
        self.theme.motion.beat.bpm > 0.0,
      )
    });
    View {
      greeting: self.theme.greeting.clone(),
      username: self.username.value().to_string(),
      username_cursor: self.username.cursor_prefix().chars().count(),
      password: if visible_answer {
        self.answer.value().to_string()
      } else {
        self.answer.masked_with(mask)
      },
      password_cursor: if visible_answer {
        self.answer.cursor_prefix().chars().count()
      } else {
        self.answer.masked_cursor_prefix_with(mask).chars().count()
      },
      focus: self.focus,
      typed_at: self.typed_at,
      session_changed_at: self.session_changed_at,
      prompt: self.prompt.as_ref().map(|prompt| prompt.message.clone()),
      status: self.status.clone(),
      session: self
        .sessions
        .get(self.session_index)
        .map_or_else(String::new, |session| session.name.clone()),
      session_count: self.sessions.len(),
      picker: if self.focus == Focus::Picker {
        self
          .sessions
          .iter()
          .map(|session| session.name.clone())
          .collect()
      } else {
        Vec::new()
      },
      picker_selected: self.picker_index,
      dry_run: self.dry_run,
      warning: self.warning.clone(),
      busy: self.busy,
      authenticated: self.authenticated,
      splash,
      battery: self.battery.clone(),
      system: self.system.clone(),
    }
  }

  #[must_use]
  pub fn theme(&self) -> &Theme {
    &self.theme
  }

  #[must_use]
  pub fn plugins(&self) -> &[Plugin] {
    &self.plugins
  }

  #[must_use]
  pub fn session(&self) -> Option<&Session> {
    self.sessions.get(self.session_index)
  }
}
