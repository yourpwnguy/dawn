use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{App, Finish, Key};
use crate::auth::backend::Backend;
use crate::auth::client::ClientError;
use crate::auth::flow::Outgoing;
use crate::auth::model::{Notice, NoticeKind, Prompt, PromptKind, Response};
use crate::auth::worker::{Mode, spawn_with_backend};
use crate::config::{Config, UsernamePrefill};
use crate::session::Session;

fn session(index: usize) -> Session {
  Session {
    id: format!("s{index}"),
    name: format!("Session {index}"),
    command: vec!["true".to_string()],
    desktop_names: Vec::new(),
  }
}

/// Tests must not depend on the machine's `/etc/passwd` or `last` history.
/// The live default is `auto`; here we pin the historical empty field.
fn config() -> Config {
  let mut config = Config::default();
  config.login.prefill = UsernamePrefill::None;
  config
}

fn app(sessions: usize) -> App {
  App::new(
    config(),
    (0..sessions).map(session).collect(),
    Vec::new(),
    None,
    None,
    true,
  )
}

fn app_with_worker(sessions: usize) -> App {
  let worker = crate::auth::worker::spawn(Mode::DryRun, |_| {}).expect("test worker should start");
  App::new(
    config(),
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

/// A backend that asks for a password and records every answer it gets.
/// `reask` makes it ask a second time after the first answer, which models a
/// PAM stack asking again inside the same conversation.
struct ScriptedBackend {
  reask: bool,
  answers: Arc<Mutex<Vec<String>>>,
}

impl Backend for ScriptedBackend {
  fn request(&mut self, request: Outgoing) -> Result<Response, ClientError> {
    match request {
      Outgoing::CreateSession { .. } => Ok(Response::Prompt {
        kind: PromptKind::Secret,
        message: "Password:".to_string(),
      }),
      Outgoing::Answer { response } => {
        let secret = response.expect("test backend expects a secret answer");
        let mut answers = self.answers.lock().expect("answer log poisoned");
        answers.push(secret.expose().to_string());
        if self.reask && answers.len() == 1 {
          Ok(Response::Prompt {
            kind: PromptKind::Secret,
            message: "Password again:".to_string(),
          })
        } else {
          Ok(Response::Success)
        }
      }
      Outgoing::Start(_) | Outgoing::Cancel => Ok(Response::Success),
    }
  }
}

fn app_with_scripted_worker(reask: bool) -> (App, Receiver<super::Event>, Arc<Mutex<Vec<String>>>) {
  let (tx, rx) = mpsc::channel();
  let answers = Arc::new(Mutex::new(Vec::new()));
  let worker = spawn_with_backend(
    Box::new(ScriptedBackend {
      reask,
      answers: Arc::clone(&answers),
    }),
    move |event| {
      let _ = tx.send(event);
    },
  )
  .expect("scripted worker should start");
  let app = App::new(
    config(),
    vec![session(0)],
    Vec::new(),
    Some(worker),
    None,
    true,
  );
  (app, rx, answers)
}

fn recv_auth(rx: &Receiver<super::Event>) -> super::Event {
  rx.recv_timeout(Duration::from_secs(2))
    .expect("worker did not emit an event")
}

/// Types a username and password, then presses Enter once. This is the
/// pre-typed path: the submit sends `create_session` and the password is
/// still sitting in the field when PAM asks for it.
fn submit_pre_typed_password(app: &mut App, rx: &Receiver<super::Event>) -> super::Event {
  type_text(app, "alice");
  app.key(Key::Tab);
  type_text(app, "pw");
  app.key(Key::Enter);
  recv_auth(rx)
}

#[test]
fn a_prefilled_username_starts_on_the_password() {
  let mut config = config();
  config.login.prefill = UsernamePrefill::Name("alice".to_string());
  let mut app = App::new(config, vec![session(0)], Vec::new(), None, None, true);
  assert_eq!(app.view().username, "alice");
  assert_eq!(app.view().focus, crate::ui::view::Focus::Password);
  type_text(&mut app, "pw");
  assert_eq!(app.view().password, "••");
}

#[test]
fn prefill_none_keeps_the_historical_empty_field() {
  let app = app(1);
  assert_eq!(app.view().username, "");
  assert_eq!(app.view().focus, crate::ui::view::Focus::Username);
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
fn submit_epoch_only_moves_when_a_request_is_sent() {
  let mut app = app_with_worker(1);
  // A focus move is not a submit, and the cut sequence must not fire.
  type_text(&mut app, "alice");
  app.key(Key::Enter);
  assert_eq!(app.submit_epoch(), 0);

  type_text(&mut app, "pw");
  app.key(Key::Enter);
  assert_eq!(app.submit_epoch(), 1);

  // Enter while busy is read-only, so it cannot replay the sequence.
  app.key(Key::Enter);
  assert_eq!(app.submit_epoch(), 1);
}

#[test]
fn a_submit_without_a_worker_does_not_count_as_sent() {
  let mut app = app(1);
  type_text(&mut app, "alice");
  app.key(Key::Enter);
  type_text(&mut app, "pw");
  app.key(Key::Enter);
  assert_eq!(app.submit_epoch(), 0);
}

#[test]
fn a_pre_typed_password_answers_the_pam_prompt_without_a_second_enter() {
  let (mut app, rx, answers) = app_with_scripted_worker(false);
  let prompt = submit_pre_typed_password(&mut app, &rx);
  // The submit is in flight and the typed password is masked.
  assert!(app.view().busy);
  assert_eq!(app.view().password, "\u{2022}\u{2022}");

  app.handle_auth(prompt);

  // One Enter was enough: the hidden PAM prompt took the typed value.
  assert!(app.view().busy);
  assert_eq!(app.view().prompt.as_deref(), Some("Password:"));
  // The secret moved out of the field, so nothing is left to display.
  assert!(app.view().password.is_empty());
  // Waiting for the next worker event proves the answer reached the backend.
  assert!(matches!(recv_auth(&rx), super::Event::Starting));
  assert_eq!(*answers.lock().unwrap(), vec!["pw".to_string()]);
}

#[test]
fn a_pam_reask_is_not_auto_answered() {
  let (mut app, rx, answers) = app_with_scripted_worker(true);
  let prompt = submit_pre_typed_password(&mut app, &rx);
  app.handle_auth(prompt);
  let reask = recv_auth(&rx);
  assert!(matches!(reask, super::Event::Prompt(_)));

  app.handle_auth(reask);

  // The one-shot guard leaves the second prompt for the user instead of
  // replaying the stale secret, which is the loop safety.
  assert!(!app.view().busy);
  assert!(app.view().prompt.is_some());
  assert_eq!(*answers.lock().unwrap(), vec!["pw".to_string()]);

  type_text(&mut app, "second");
  app.key(Key::Enter);
  // Draining to Started proves the worker processed every answer in order.
  while !matches!(recv_auth(&rx), super::Event::Started) {}
  assert_eq!(
    *answers.lock().unwrap(),
    vec!["pw".to_string(), "second".to_string()]
  );
}

#[test]
fn auto_answer_does_not_bump_the_submit_epoch() {
  let (mut app, rx, _answers) = app_with_scripted_worker(false);
  let prompt = submit_pre_typed_password(&mut app, &rx);
  assert_eq!(app.submit_epoch(), 1);

  app.handle_auth(prompt);

  // The automatic answer belongs to the same user intent as the Enter that
  // sent `create_session`, so the window's cut transition must not restart.
  assert_eq!(app.submit_epoch(), 1);
  assert!(matches!(recv_auth(&rx), super::Event::Starting));
  assert_eq!(app.submit_epoch(), 1);
}

#[test]
fn a_visible_prompt_still_clears_the_field() {
  let mut app = app(1);
  type_text(&mut app, "alice");
  app.key(Key::Tab);
  type_text(&mut app, "pw");
  assert_eq!(app.view().password, "\u{2022}\u{2022}");

  app.handle_auth(super::Event::Prompt(Prompt {
    kind: PromptKind::Visible,
    message: "Verification code:".to_string(),
  }));

  // Showing a pre-typed password in plain text would leak it.
  assert!(app.view().password.is_empty());
  assert_eq!(app.view().prompt.as_deref(), Some("Verification code:"));
}

#[test]
fn an_empty_password_field_waits_for_the_user() {
  let (mut app, rx, answers) = app_with_scripted_worker(false);
  type_text(&mut app, "alice");
  app.key(Key::Tab);
  app.key(Key::Enter);
  let prompt = recv_auth(&rx);
  app.handle_auth(prompt);

  // Nothing was typed, so there is nothing to send and the prompt waits.
  assert!(!app.view().busy);
  assert!(app.view().prompt.is_some());
  assert!(answers.lock().unwrap().is_empty());
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
