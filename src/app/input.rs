//! Input methods for the login app.

use super::*;

impl App {
  pub(super) fn insert_char(&mut self, ch: char) {
    match self.focus {
      Focus::Username => {
        self.username.insert(ch);
        self.typed_at = Some(Instant::now());
        self.audio.play(AudioEvent::Key);
      }
      Focus::Password => {
        self.answer.insert(ch);
        self.typed_at = Some(Instant::now());
        self.audio.play(AudioEvent::Key);
      }
      Focus::Picker => {}
    }
  }

  pub(super) fn edit(&mut self, change: impl FnOnce(&mut TextInput)) {
    match self.focus {
      Focus::Username => change(&mut self.username),
      Focus::Password => change(&mut self.answer),
      Focus::Picker => {}
    }
  }

  pub(super) fn enter(&mut self) {
    match self.focus {
      Focus::Picker => {
        if self.picker_index < self.sessions.len() && self.picker_index != self.session_index {
          self.session_index = self.picker_index;
          self.session_changed_at = Some(Instant::now());
          self.audio.play(AudioEvent::SessionChange);
        }
        self.focus = Focus::Password;
        self.status = None;
      }
      Focus::Username => {
        if self.answer.is_empty() {
          self.focus = Focus::Password;
        } else {
          self.submit();
        }
      }
      Focus::Password => self.submit(),
    }
  }

  pub(super) fn submit(&mut self) {
    let Some(worker) = self.worker.as_ref() else {
      self.status = Some(Line::error(
        "Authentication is unavailable in this mode (no greetd socket).",
      ));
      return;
    };

    if self.prompt.is_some() {
      let secret = self.answer.take_secret();
      self.busy = true;
      self.status = None;
      self.submit_epoch = self.submit_epoch.wrapping_add(1);
      if !worker.answer(secret) {
        self.worker_lost();
      }
      return;
    }

    let Some(session) = self.sessions.get(self.session_index) else {
      self.status = Some(Line::error(
        "No session is selected. Install a desktop session or set [sessions].dirs.",
      ));
      self.focus = Focus::Username;
      return;
    };
    if self.username.value().trim().is_empty() {
      self.status = Some(Line::error("Enter a username"));
      self.focus = Focus::Username;
      return;
    }

    let launch = Launch {
      cmd: session.command.clone(),
      env: session.env(),
    };
    let session_id = session.id.clone();
    if worker.begin(self.username.value(), launch) {
      info!(session = %session_id, "login attempt started");
      self.audio.play(AudioEvent::Submit);
      self.busy = true;
      self.status = None;
      self.secret_auto = false;
      self.submit_epoch = self.submit_epoch.wrapping_add(1);
    } else {
      self.worker_lost();
    }
  }

  /// The worker thread is gone. This is not recoverable in-process, so say so
  /// plainly instead of leaving a spinner on screen forever.
  pub(super) fn worker_lost(&mut self) {
    self.busy = false;
    self.status = Some(Line::error(
      "The authentication worker stopped. Restart dawn.",
    ));
  }

  pub(super) fn cancel(&mut self) {
    self.audio.play(AudioEvent::Cancel);
    self.busy = false;
    self.answer.clear();
    self.prompt = None;
    self.answer_secret = true;
    let sent = self.worker.as_ref().is_some_and(WorkerHandle::cancel);
    self.cancel_requested = sent;
    self.status = Some(if sent {
      Line::info("Cancelling\u{2026}")
    } else {
      Line::info("Login cancelled")
    });
  }

  pub(super) fn escape(&mut self) {
    match self.focus {
      Focus::Picker => {
        // Escape leaves the picker without changing the session.
        self.picker_index = self.session_index;
        self.focus = Focus::Password;
      }
      Focus::Password => {
        if self.answer.is_empty() {
          self.focus = Focus::Username;
        } else {
          self.answer.clear();
        }
        self.status = None;
      }
      Focus::Username => {
        if !self.answer.is_empty() {
          self.answer.clear();
        } else if !self.username.is_empty() {
          self.username.clear();
        }
        self.status = None;
      }
    }
  }

  pub(super) fn cycle(&mut self, forward: bool) {
    if self.focus == Focus::Picker {
      self.picker_index = self.session_index;
      self.focus = if forward {
        Focus::Username
      } else {
        Focus::Password
      };
      return;
    }

    let has_picker = self.sessions.len() > 1 && self.theme.login.show_picker;
    self.focus = match (self.focus, forward, has_picker) {
      (Focus::Username, true, _) => Focus::Password,
      (Focus::Username, false, true) => {
        self.open_picker();
        self.focus
      }
      (Focus::Username, false, false) => Focus::Password,
      (Focus::Password, true, true) => {
        self.open_picker();
        self.focus
      }
      (Focus::Password, true, false) => Focus::Username,
      (Focus::Password, false, _) => Focus::Username,
      (Focus::Picker, _, _) => Focus::Password,
    };
  }

  pub(super) fn open_picker(&mut self) {
    self.picker_index = self.session_index;
    self.focus = Focus::Picker;
  }

  pub(super) fn move_picker(&mut self, delta: isize) {
    if self.sessions.is_empty() {
      return;
    }
    let last = self.sessions.len() - 1;
    self.picker_index = if delta < 0 {
      self.picker_index.saturating_sub(delta.unsigned_abs())
    } else {
      (self.picker_index + delta as usize).min(last)
    };
    if self.picker_index != self.session_index {
      self.audio.play(AudioEvent::SessionChange);
    }
  }
}
