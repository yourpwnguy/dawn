//! Auth methods for the login app.

use super::*;

impl App {
  pub fn media_states(&self) -> Vec<Arc<MediaState>> {
    self.media.clone()
  }

  #[must_use]
  pub fn authenticated(&self) -> bool {
    self.authenticated
  }

  /// Handles one key. Returns true when the frame needs repainting.
  pub fn key(&mut self, key: Key) -> bool {
    if self.busy {
      if key == Key::Escape {
        self.cancel();
        return true;
      }
      // While a request is in flight the form is read-only. Typing into an
      // input whose value has already been sent would leave stray characters
      // behind for the next attempt.
      return false;
    }

    let previous_focus = self.focus;
    match key {
      Key::Char(ch) => self.insert_char(ch),
      Key::Enter => self.enter(),
      Key::Tab => self.cycle(true),
      Key::BackTab => self.cycle(false),
      Key::Backspace => {
        self.edit(|input| input.backspace());
        self.audio.play(AudioEvent::Backspace);
      }
      Key::Delete => {
        self.edit(|input| input.delete());
        self.audio.play(AudioEvent::Backspace);
      }
      Key::Left => self.edit(|input| input.move_left()),
      Key::Right => self.edit(|input| input.move_right()),
      Key::Home => self.edit(|input| input.move_home()),
      Key::End => self.edit(|input| input.move_end()),
      Key::Up if self.focus == Focus::Picker => self.move_picker(-1),
      Key::Down if self.focus == Focus::Picker => self.move_picker(1),
      Key::Up | Key::Down => {}
      Key::Escape => self.escape(),
    }
    if self.focus != previous_focus {
      self.typed_at = None;
      self.motion.notify_focus(Instant::now());
      self.audio.play(AudioEvent::FieldFocus);
    }
    true
  }

  /// Applies an event from the auth worker. Returns true when the frame needs
  /// repainting.
  pub fn handle_auth(&mut self, event: Event) -> bool {
    match event {
      Event::Prompt(prompt) => {
        if self.cancel_requested {
          // A reply that was already in flight when the user cancelled. Drop
          // it instead of reopening the form.
          return false;
        }
        self.answer_secret = prompt.kind == PromptKind::Secret;
        self.status = None;
        self.focus = Focus::Password;
        self.prompt = Some(prompt);
        self.busy = false;
        if !self.answer_secret {
          // A visible prompt clears the field instead of keeping the typed
          // value, because showing a pre-typed password in plain text would be
          // a leak. The user sees this prompt and types the answer themselves.
          self.answer.clear();
        } else if !self.answer.is_empty() && !self.secret_auto {
          // The user already typed the password before the first submit sent
          // `create_session`. PAM's hidden password prompt is the one they
          // meant, so answer it now instead of asking for a second Enter. The
          // `secret_auto` guard keeps this to one send per conversation.
          self.auto_answer();
        }
      }
      Event::Notice(notice) => {
        if self.cancel_requested {
          return false;
        }
        self.status = Some(match notice.kind {
          NoticeKind::Info => Line::info(notice.message),
          NoticeKind::Error => Line::error(notice.message),
        });
      }
      Event::Starting => {
        if self.cancel_requested {
          return false;
        }
        crate::rituals::on_success(&self.rituals);
        self.audio.play(AudioEvent::Success);
        self.prompt = None;
        self.status = None;
        self.answer.clear();
        self.busy = true;
        self.secret_auto = false;
      }
      Event::Started => {
        info!("session start accepted, exiting so greetd can launch it");
        self.authenticated = true;
        self.busy = true;
        self.finished = Some(Finish::Started);
      }
      Event::Failed(failure) => {
        self.audio.play(AudioEvent::Failure);
        self.motion.notify_error(Instant::now());
        self.busy = false;
        self.cancel_requested = false;
        self.prompt = None;
        self.answer_secret = true;
        self.answer.clear();
        self.secret_auto = false;
        self.focus = Focus::Password;
        self.status = Some(Line::error(failure.message));
        if failure.during_start {
          warn!("greetd accepted authentication but could not start the session");
        }
      }
      Event::Cancelled => {
        self.busy = false;
        self.cancel_requested = false;
        self.prompt = None;
        self.answer_secret = true;
        self.answer.clear();
        self.status = Some(Line::info("Login cancelled"));
      }
    }
    true
  }

  /// Sends a password the user typed before PAM asked for one.
  ///
  /// This mirrors the prompt branch of [`Self::submit`], including moving the
  /// secret out with [`TextInput::take_secret`] (which wipes the field) and
  /// reporting a dead worker the same way. It deliberately does NOT touch
  /// `submit_epoch`: the window starts its cut transition on the user's real
  /// Enter, and bumping again here would restart that animation late.
  pub(super) fn auto_answer(&mut self) {
    let Some(worker) = self.worker.as_ref() else {
      // Screenshot and `--check-config` runs have no worker. Leave the typed
      // value alone and let the user press Enter as before.
      return;
    };
    let secret = self.answer.take_secret();
    self.busy = true;
    self.status = None;
    self.secret_auto = true;
    if !worker.answer(secret) {
      self.worker_lost();
    }
  }
}
