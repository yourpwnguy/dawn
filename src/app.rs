//! UI state and the glue between keyboard input and the auth worker.
//!
//! This module owns the login form, the session picker, and the small amount of
//! policy that connects them: when to send a login attempt, when Escape cancels
//! one, and what to show when greetd answers. It deliberately has no window and
//! no drawing calls, so the entire interaction model is testable in-process.
//!
//! The one subtle rule here is the cancel guard. Pressing Escape while a request
//! is in flight sends `cancel_session` and then ignores any prompt that was
//! already on the wire. Without that, a late "Password:" reply would reopen the
//! form after the user asked to stop, and the next keystroke would be sent into
//! a session they thought they had abandoned.
//!
//! Keyboard map (pointer clicks and drags are translated by the window layer
//! into the same keys and focus requests):
//!
//! - `Tab` / `Shift+Tab`: move between username, password, and the picker.
//! - `Enter`: continue from the username, sign in from the password, or pick a
//!   session while the picker is open.
//! - `Up` / `Down`: choose a session in the picker.
//! - `Escape`: cancel a login in flight, otherwise clear the current field.

use std::sync::Arc;
use std::time::Instant;

use tracing::{info, warn};

use crate::auth::model::{Launch, NoticeKind, Prompt, PromptKind};
use crate::auth::worker::{Event, WorkerHandle};
use crate::config::Config;
use crate::plugin::{self, Plugin};
use crate::session::Session;
use crate::ui::input::TextInput;
use crate::ui::media::MediaState;
use crate::ui::motion::{Motion, MotionFrame};
use crate::ui::theme::Theme;
use crate::ui::view::{Focus, Line, View};

const MAX_USERNAME_CHARS: usize = 256;
const MAX_ANSWER_CHARS: usize = 1024;

/// A key the window or the TTY frontend recognised. Text keys are separate from
/// control keys so each frontend only has to map its own key codes once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
  Char(char),
  Enter,
  Tab,
  BackTab,
  Backspace,
  Delete,
  Left,
  Right,
  Home,
  End,
  Up,
  Down,
  Escape,
}

/// Why the frontend should stop. `Started` means greetd accepted
/// `start_session`, so the process must exit for the session to launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
  Started,
  Quit,
}

pub struct App {
  theme: Theme,
  sessions: Vec<Session>,
  session_index: usize,
  picker_index: usize,
  focus: Focus,
  username: TextInput,
  answer: TextInput,
  prompt: Option<Prompt>,
  /// Whether the current answer should be masked. Login passwords always are;
  /// a PAM "visible" prompt (an OTP, for example) is not.
  answer_secret: bool,
  status: Option<Line>,
  busy: bool,
  cancel_requested: bool,
  dry_run: bool,
  warning: Option<String>,
  worker: Option<WorkerHandle>,
  signature: String,
  plugins: Vec<Plugin>,
  media: Vec<Arc<MediaState>>,
  motion: Motion,
  authenticated: bool,
  finished: Option<Finish>,
}

impl App {
  /// Builds the login screen. A missing worker is allowed because the
  /// screenshot and `--check-config` paths only need the view.
  #[must_use]
  pub fn new(
    config: Config,
    sessions: Vec<Session>,
    mut plugins: Vec<Plugin>,
    worker: Option<WorkerHandle>,
    warning: Option<String>,
    dry_run: bool,
  ) -> Self {
    let theme = Theme::from(&config);
    let builtins = plugin::builtins(&config);
    // Collect third-party media state before appending the built-in plugin,
    // otherwise the built-in media widget would be counted twice and the
    // window would start two identical MPRIS pollers.
    let mut media: Vec<Arc<MediaState>> = plugins.iter().flat_map(Plugin::media_states).collect();
    plugins.extend(builtins.plugins);
    media.extend(builtins.media);

    let status = sessions
      .is_empty()
      .then(|| Line::error(config.login.no_sessions.clone()));
    let motion = Motion::new(&config.motion, config.power.low_power);

    Self {
      theme,
      sessions,
      session_index: 0,
      picker_index: 0,
      focus: Focus::Username,
      username: TextInput::new(MAX_USERNAME_CHARS),
      answer: TextInput::new(MAX_ANSWER_CHARS),
      prompt: None,
      answer_secret: true,
      status,
      busy: false,
      cancel_requested: false,
      dry_run,
      warning,
      worker,
      signature: String::new(),
      plugins,
      media,
      motion,
      authenticated: false,
      finished: None,
    }
  }

  #[must_use]
  pub fn finished(&self) -> Option<Finish> {
    self.finished
  }

  /// Moves keyboard focus when the pointer clicks a field. The fields are
  /// still fully keyboard-driven; this just removes the surprise of a click
  /// doing nothing.
  pub fn focus_field(&mut self, focus: Focus) -> bool {
    if self.busy || self.focus == focus {
      return false;
    }
    self.focus = focus;
    self.motion.notify_focus(Instant::now());
    true
  }

  /// Called when the window closes. The auth worker is stopped by dropping the
  /// handle, which also wipes any secret still queued for it.
  pub fn quit(&mut self) {
    self.finished = Some(Finish::Quit);
  }

  /// Recomputes every widget's visible signature. Returns true when something
  /// changed, which is the only reason an otherwise idle greeter redraws.
  pub fn tick(&mut self, now: &jiff::Zoned) -> bool {
    let mut signature = String::new();
    for plugin in &self.plugins {
      for widget in &plugin.widgets {
        signature.push('\u{1f}');
        signature.push_str(&widget.signature(now));
      }
    }
    let changed = signature != self.signature;
    self.signature = signature;
    changed
  }

  /// The motion values for this frame. Boot animation starts on the first
  /// call, which is the first presented frame.
  pub fn motion_frame(&mut self, now: Instant, scale: f32) -> MotionFrame {
    self.motion.frame(now, scale)
  }

  #[must_use]
  pub fn motion_active(&self) -> bool {
    self.motion.active()
  }

  /// Media states for the window to feed. The app never talks to D-Bus.
  #[must_use]
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
      Key::Backspace => self.edit(|input| input.backspace()),
      Key::Delete => self.edit(|input| input.delete()),
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
      self.motion.notify_focus(Instant::now());
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
        // Keep a password the user typed before PAM asked for one, so they do
        // not have to type it twice. A visible prompt clears the field instead,
        // because showing a pre-typed password in plain text would be a leak.
        // The user still has to press Enter, so the value is never sent into a
        // prompt they have not seen.
        if !self.answer_secret {
          self.answer.clear();
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
        self.prompt = None;
        self.status = None;
        self.answer.clear();
        self.busy = true;
      }
      Event::Started => {
        info!("session start accepted, exiting so greetd can launch it");
        self.authenticated = true;
        self.busy = true;
        self.finished = Some(Finish::Started);
      }
      Event::Failed(failure) => {
        self.motion.notify_error(Instant::now());
        self.busy = false;
        self.cancel_requested = false;
        self.prompt = None;
        self.answer_secret = true;
        self.answer.clear();
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

  /// The current frame description. Safe to call at any time; it never touches
  /// the worker or the filesystem.
  #[must_use]
  pub fn view(&self) -> View {
    let visible_answer = self.prompt.is_some() && !self.answer_secret;
    let mask = &self.theme.login.password.mask;
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

  fn insert_char(&mut self, ch: char) {
    match self.focus {
      Focus::Username => self.username.insert(ch),
      Focus::Password => self.answer.insert(ch),
      Focus::Picker => {}
    }
  }

  fn edit(&mut self, change: impl FnOnce(&mut TextInput)) {
    match self.focus {
      Focus::Username => change(&mut self.username),
      Focus::Password => change(&mut self.answer),
      Focus::Picker => {}
    }
  }

  fn enter(&mut self) {
    match self.focus {
      Focus::Picker => {
        if self.picker_index < self.sessions.len() {
          self.session_index = self.picker_index;
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

  fn submit(&mut self) {
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
      self.busy = true;
      self.status = None;
    } else {
      self.worker_lost();
    }
  }

  /// The worker thread is gone. This is not recoverable in-process, so say so
  /// plainly instead of leaving a spinner on screen forever.
  fn worker_lost(&mut self) {
    self.busy = false;
    self.status = Some(Line::error(
      "The authentication worker stopped. Restart dawn.",
    ));
  }

  fn cancel(&mut self) {
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

  fn escape(&mut self) {
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

  fn cycle(&mut self, forward: bool) {
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

  fn open_picker(&mut self) {
    self.picker_index = self.session_index;
    self.focus = Focus::Picker;
  }

  fn move_picker(&mut self, delta: isize) {
    if self.sessions.is_empty() {
      return;
    }
    let last = self.sessions.len() - 1;
    self.picker_index = if delta < 0 {
      self.picker_index.saturating_sub(delta.unsigned_abs())
    } else {
      (self.picker_index + delta as usize).min(last)
    };
  }
}

impl Drop for App {
  fn drop(&mut self) {
    // Dropping the input wipes any password still in it, even if the window
    // closed mid-typing. The worker handle wipes anything queued for greetd.
    self.answer.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::{App, Finish, Key};
  use crate::auth::model::{Notice, NoticeKind, Prompt, PromptKind};
  use crate::auth::worker::Mode;
  use crate::config::Config;
  use crate::session::Session;

  fn session(index: usize) -> Session {
    Session {
      id: format!("s{index}"),
      name: format!("Session {index}"),
      command: vec!["true".to_string()],
      desktop_names: Vec::new(),
    }
  }

  fn app(sessions: usize) -> App {
    App::new(
      Config::default(),
      (0..sessions).map(session).collect(),
      Vec::new(),
      None,
      None,
      true,
    )
  }

  fn app_with_worker(sessions: usize) -> App {
    let worker =
      crate::auth::worker::spawn(Mode::DryRun, |_| {}).expect("test worker should start");
    App::new(
      Config::default(),
      (0..sessions).map(session).collect(),
      Vec::new(),
      Some(worker),
      None,
      true,
    )
  }

  fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
      app.key(Key::Char(ch));
    }
  }

  #[test]
  fn tab_cycles_through_the_fields_and_the_picker() {
    let mut app = app(2);
    app.key(Key::Tab);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Password);
    app.key(Key::Tab);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Picker);
    app.key(Key::Tab);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Username);
    app.key(Key::BackTab);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Picker);
    app.key(Key::BackTab);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Password);
  }

  #[test]
  fn picker_selects_a_session_and_escape_does_not() {
    let mut app = app(3);
    app.key(Key::Tab);
    app.key(Key::Tab);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Picker);
    app.key(Key::Down);
    app.key(Key::Enter);
    assert_eq!(app.session().map(|s| s.id.as_str()), Some("s1"));
    assert_eq!(app.view().focus, crate::ui::view::Focus::Password);

    app.key(Key::Tab);
    app.key(Key::Down);
    app.key(Key::Escape);
    assert_eq!(app.session().map(|s| s.id.as_str()), Some("s1"));
    assert_eq!(app.view().focus, crate::ui::view::Focus::Password);
  }

  #[test]
  fn picker_arrows_clamp_at_the_ends() {
    let mut app = app(2);
    app.key(Key::Tab);
    app.key(Key::Tab);
    app.key(Key::Up);
    assert_eq!(app.view().picker_selected, 0);
    app.key(Key::Down);
    app.key(Key::Down);
    assert_eq!(app.view().picker_selected, 1);
  }

  #[test]
  fn typing_goes_to_the_focused_field_and_passwords_stay_masked() {
    let mut app = app(1);
    type_text(&mut app, "alice");
    assert_eq!(app.view().username, "alice");
    app.key(Key::Tab);
    type_text(&mut app, "pw");
    assert_eq!(app.view().password, "\u{2022}\u{2022}");
    assert_eq!(app.view().username, "alice");
  }

  #[test]
  fn a_visible_prompt_shows_what_is_typed() {
    let mut app = app(1);
    app.handle_auth(super::Event::Prompt(Prompt {
      kind: PromptKind::Visible,
      message: "Verification code:".to_string(),
    }));
    type_text(&mut app, "123456");
    assert_eq!(app.view().password, "123456");
  }

  #[test]
  fn enter_on_the_username_moves_to_the_password() {
    let mut app = app(1);
    type_text(&mut app, "alice");
    app.key(Key::Enter);
    assert_eq!(app.view().focus, crate::ui::view::Focus::Password);
  }

  #[test]
  fn a_full_login_is_sent_to_the_worker_only_after_a_password() {
    let mut app = app_with_worker(1);
    type_text(&mut app, "alice");
    app.key(Key::Enter);
    type_text(&mut app, "pw");
    app.key(Key::Enter);
    assert!(app.view().busy);
    // The password typed before the prompt is kept, still masked, so the user
    // does not have to type it again.
    assert_eq!(app.view().password, "\u{2022}\u{2022}");
    // Typing while busy must not change it.
    type_text(&mut app, "x");
    assert_eq!(app.view().password, "\u{2022}\u{2022}");
  }

  #[test]
  fn escape_cancels_a_pending_login_and_ignores_late_prompts() {
    let mut app = app_with_worker(1);
    app.handle_auth(super::Event::Starting);
    assert!(app.view().busy);
    app.key(Key::Escape);
    assert!(!app.view().busy);
    assert!(!app.handle_auth(super::Event::Prompt(Prompt {
      kind: PromptKind::Secret,
      message: "Password:".to_string(),
    })));
    assert!(app.view().prompt.is_none());
    app.handle_auth(super::Event::Cancelled);
    assert!(app.view().status.is_some());
  }

  #[test]
  fn failures_clear_the_answer_and_allow_a_retry() {
    let mut app = app(1);
    app.handle_auth(super::Event::Prompt(Prompt {
      kind: PromptKind::Secret,
      message: "Password:".to_string(),
    }));
    type_text(&mut app, "wrong");
    app.handle_auth(super::Event::Failed(crate::auth::model::Failure {
      message: "Login incorrect".to_string(),
      during_start: false,
    }));
    let view = app.view();
    assert!(view.password.is_empty());
    assert!(view.prompt.is_none());
    assert_eq!(
      view.status.as_ref().map(|line| line.text.as_str()),
      Some("Login incorrect")
    );
    assert_eq!(view.focus, crate::ui::view::Focus::Password);
  }

  #[test]
  fn notices_are_visible_without_unlocking_the_form() {
    let mut app = app(1);
    app.handle_auth(super::Event::Starting);
    app.handle_auth(super::Event::Notice(Notice {
      kind: NoticeKind::Info,
      message: "Last login: yesterday".to_string(),
    }));
    let view = app.view();
    assert!(view.busy);
    assert_eq!(
      view.status.as_ref().map(|line| line.text.as_str()),
      Some("Last login: yesterday")
    );
  }

  #[test]
  fn started_asks_the_frontend_to_exit() {
    let mut app = app(1);
    app.handle_auth(super::Event::Started);
    assert_eq!(app.finished(), Some(Finish::Started));
  }

  #[test]
  fn no_sessions_is_visibly_reported_and_enter_is_safe() {
    let mut app = app(0);
    assert!(app.view().status.is_some());
    app.key(Key::Enter);
    app.key(Key::Tab);
    app.key(Key::Tab);
    assert_ne!(app.view().focus, crate::ui::view::Focus::Picker);
    assert_eq!(app.finished(), None);
  }

  #[test]
  fn submit_without_a_worker_reports_why() {
    let mut app = app(1);
    type_text(&mut app, "alice");
    app.key(Key::Enter);
    type_text(&mut app, "pw");
    app.key(Key::Enter);
    let status = app.view().status.expect("status line");
    assert!(status.text.contains("unavailable"));
  }

  #[test]
  fn clock_tick_reports_only_visible_changes() {
    let mut app = app(1);
    let first = jiff::Timestamp::from_second(0)
      .unwrap()
      .in_tz("UTC")
      .unwrap();
    assert!(app.tick(&first));
    assert!(!app.tick(&first));
  }

  #[test]
  fn builtins_register_one_media_state() {
    // A duplicate state would start a second MPRIS poller, so this guards the
    // ordering bug where the built-in plugin was extended before media states
    // were collected.
    assert_eq!(app(1).media_states().len(), 1);
  }
}
