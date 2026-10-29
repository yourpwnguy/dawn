//! The authentication state machine.
//!
//! greetd's protocol is a strict request/reply conversation with no fixed
//! number of steps: PAM can ask for a password, a one-time code, print an
//! informational message, or fail. Modelling that with booleans is exactly how
//! greeters end up sending a password into the wrong prompt, so this module
//! keeps an explicit state and validates every transition.
//!
//! The machine is synchronous and has no idea where replies come from.
//! [`crate::auth::worker`] feeds it a reply, executes the resulting
//! [`Outgoing`] request, and feeds the next reply back in. Tests do the same
//! with a scripted backend, which is why the security-critical part has no
//! socket or thread in it.

use crate::auth::model::{Failure, Launch, Notice, Prompt, PromptKind, Response};
use crate::secret::Secret;

/// Upper bound on consecutive non-question messages. A broken PAM stack could
/// otherwise keep the greeter answering forever while pretending to work.
const MAX_INFO_BURST: u8 = 8;

/// Requests the worker has to perform.
pub enum Outgoing {
  CreateSession {
    username: String,
  },
  /// `None` acknowledges an informational message without supplying input.
  Answer {
    response: Option<Secret>,
  },
  Start(Launch),
  Cancel,
}

/// The next thing that should happen after a transition.
pub enum Effect {
  Request(Outgoing),
  /// Show this prompt and wait for [`Flow::answer`].
  Ask(Prompt),
  /// `start_session` succeeded. greetd launches the session once the greeter
  /// process exits, so the frontend should now leave.
  Completed,
  Failed(Failure),
  Cancelled,
}

/// One transition result: the next effect plus any messages that should be
/// shown to the user before it happens (for example "Last login: ...").
pub struct Outcome {
  pub effect: Effect,
  pub notices: Vec<Notice>,
}

impl Outcome {
  fn request(outgoing: Outgoing) -> Self {
    Self {
      effect: Effect::Request(outgoing),
      notices: Vec::new(),
    }
  }

  fn failed(message: impl Into<String>, during_start: bool) -> Self {
    Self {
      effect: Effect::Failed(Failure {
        message: message.into(),
        during_start,
      }),
      notices: Vec::new(),
    }
  }
}

/// A bug in our own state handling. This is not a user error: the worker turns
/// it into a visible failure and resets the machine rather than panicking.
#[derive(Debug, thiserror::Error)]
pub enum FlowError {
  #[error("authentication state machine got an unexpected message: {0}")]
  Unexpected(&'static str),
}

#[derive(Debug, Default)]
enum State {
  #[default]
  Idle,
  /// A request is in flight; the next reply continues the flow.
  AwaitingReply,
  /// PAM asked a question and we are waiting for the user.
  AwaitingPrompt,
  /// Authentication succeeded and `start_session` is in flight.
  Starting,
}

/// The state machine itself.
#[derive(Debug, Default)]
pub struct Flow {
  state: State,
  launch: Option<Launch>,
  info_burst: u8,
}

impl Flow {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  /// Starts a login attempt. The caller supplies the session to launch so
  /// the user cannot change it after typing a password.
  pub fn begin(&mut self, username: &str, launch: Launch) -> Outcome {
    if !matches!(self.state, State::Idle) {
      return self.abort("an authentication attempt is already in progress");
    }
    if username.trim().is_empty() {
      return Outcome::failed("Enter a username", false);
    }
    if username.len() > 256 {
      return Outcome::failed("That username is too long (limit: 256 bytes)", false);
    }
    if username.chars().any(char::is_control) {
      return Outcome::failed("That username contains control characters", false);
    }
    if launch.cmd.is_empty() {
      return Outcome::failed(
        "No session is selected. Install a desktop session or set [sessions].dirs.",
        false,
      );
    }

    self.launch = Some(launch);
    self.info_burst = 0;
    self.state = State::AwaitingReply;
    Outcome::request(Outgoing::CreateSession {
      username: username.to_string(),
    })
  }

  /// Supplies the user's answer to the current prompt. The secret is moved
  /// into the outgoing request, so no copy stays in the state machine.
  pub fn answer(&mut self, response: Secret) -> Result<Outcome, FlowError> {
    match self.state {
      State::AwaitingPrompt => {
        self.state = State::AwaitingReply;
        // A real question ends any run of informational messages.
        self.info_burst = 0;
        Ok(Outcome::request(Outgoing::Answer {
          response: Some(response),
        }))
      }
      _ => Err(FlowError::Unexpected("answer arrived while not prompting")),
    }
  }

  /// Feeds a greetd reply back into the machine.
  pub fn on_response(&mut self, response: Response) -> Result<Outcome, FlowError> {
    match response {
      Response::Success => self.on_success(),
      Response::Error {
        auth_failure,
        message,
      } => Ok(self.on_error(auth_failure, message)),
      Response::Prompt { kind, message } => self.on_prompt(kind, message),
      Response::Notice(notice) => self.on_notice(notice),
    }
  }

  fn on_success(&mut self) -> Result<Outcome, FlowError> {
    match self.state {
      State::AwaitingReply => {
        let Some(launch) = self.launch.take() else {
          return Err(FlowError::Unexpected("success without a launch command"));
        };
        self.state = State::Starting;
        Ok(Outcome::request(Outgoing::Start(launch)))
      }
      State::Starting => {
        self.state = State::Idle;
        Ok(Outcome {
          effect: Effect::Completed,
          notices: Vec::new(),
        })
      }
      _ => Err(FlowError::Unexpected("unexpected success reply")),
    }
  }

  fn on_error(&mut self, auth_failure: bool, message: String) -> Outcome {
    let during_start = matches!(self.state, State::Starting);
    self.reset();
    let fallback = if auth_failure {
      "Login incorrect"
    } else if during_start {
      "greetd could not start the session"
    } else {
      "greetd reported an error"
    };
    let message = if message.trim().is_empty() {
      fallback.to_string()
    } else {
      message
    };
    Outcome::failed(message, during_start)
  }

  fn on_prompt(&mut self, kind: PromptKind, message: String) -> Result<Outcome, FlowError> {
    if !matches!(self.state, State::AwaitingReply) {
      return Err(FlowError::Unexpected(
        "prompt arrived while not awaiting a reply",
      ));
    }
    let prompt = Prompt { kind, message };
    self.state = State::AwaitingPrompt;
    self.info_burst = 0;
    Ok(Outcome {
      effect: Effect::Ask(prompt),
      notices: Vec::new(),
    })
  }

  /// Info and error messages are notifications, not questions. greetd still
  /// requires an ack, so we answer with no value and keep waiting for the
  /// real prompt.
  fn on_notice(&mut self, notice: Notice) -> Result<Outcome, FlowError> {
    if !matches!(self.state, State::AwaitingReply) {
      return Err(FlowError::Unexpected(
        "notice arrived while not awaiting a reply",
      ));
    }
    self.info_burst = self.info_burst.saturating_add(1);
    if self.info_burst > MAX_INFO_BURST {
      self.reset();
      return Ok(Outcome::failed(
        "greetd kept sending informational messages; is PAM configured correctly?",
        false,
      ));
    }
    Ok(Outcome {
      effect: Effect::Request(Outgoing::Answer { response: None }),
      notices: vec![notice],
    })
  }

  /// Turns a transport failure into a visible login failure and resets.
  pub fn on_transport_error(&mut self, message: &str) -> Outcome {
    let during_start = matches!(self.state, State::Starting);
    self.reset();
    Outcome::failed(format!("Cannot talk to greetd: {message}"), during_start)
  }

  /// Asks for the in-flight session to be cancelled. Returns `None` when
  /// there is nothing to cancel, which happens if the user presses Escape on
  /// an already idle screen.
  pub fn cancel(&mut self) -> Option<Outgoing> {
    let was_active = !matches!(self.state, State::Idle);
    self.reset();
    was_active.then_some(Outgoing::Cancel)
  }

  /// Resets after an internal error and returns a user-visible failure.
  pub fn abort(&mut self, message: impl Into<String>) -> Outcome {
    let during_start = matches!(self.state, State::Starting);
    self.reset();
    Outcome::failed(message, during_start)
  }

  fn reset(&mut self) {
    self.state = State::Idle;
    self.launch = None;
  }
}

#[cfg(test)]
mod tests {
  use super::{Effect, Flow, Outgoing};
  use crate::auth::model::{Launch, Notice, NoticeKind, PromptKind, Response};

  fn launch() -> Launch {
    Launch {
      cmd: vec!["start-hyprland".to_string()],
      env: vec!["DESKTOP_SESSION=hyprland".to_string()],
    }
  }

  fn expect_request(outcome: super::Outcome) -> Outgoing {
    match outcome.effect {
      Effect::Request(request) => request,
      _ => panic!("expected a request"),
    }
  }

  fn expect_ask(outcome: super::Outcome) -> super::Prompt {
    match outcome.effect {
      Effect::Ask(prompt) => prompt,
      _ => panic!("expected a prompt"),
    }
  }

  #[test]
  fn happy_path_asks_for_a_password_then_starts_a_session() {
    let mut flow = Flow::new();
    assert!(matches!(
      expect_request(flow.begin("alice", launch())),
      Outgoing::CreateSession { .. }
    ));

    let prompt = expect_ask(
      flow
        .on_response(Response::Prompt {
          kind: PromptKind::Secret,
          message: "Password:".to_string(),
        })
        .unwrap(),
    );
    assert_eq!(prompt.kind, PromptKind::Secret);

    let answer = expect_request(
      flow
        .answer(crate::secret::Secret::new("pw".into()))
        .unwrap(),
    );
    assert!(matches!(answer, Outgoing::Answer { response: Some(_) }));

    let start = expect_request(flow.on_response(Response::Success).unwrap());
    assert!(matches!(start, Outgoing::Start(_)));

    let done = flow.on_response(Response::Success).unwrap();
    assert!(matches!(done.effect, Effect::Completed));
  }

  #[test]
  fn visible_prompts_stay_visible() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    let prompt = expect_ask(
      flow
        .on_response(Response::Prompt {
          kind: PromptKind::Visible,
          message: "Username:".to_string(),
        })
        .unwrap(),
    );
    assert_eq!(prompt.kind, PromptKind::Visible);
  }

  #[test]
  fn notices_are_shown_and_acknowledged_without_input() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    let outcome = flow
      .on_response(Response::Notice(Notice {
        kind: NoticeKind::Info,
        message: "Last login: yesterday".to_string(),
      }))
      .unwrap();
    assert_eq!(outcome.notices.len(), 1);
    assert!(matches!(
      outcome.effect,
      Effect::Request(Outgoing::Answer { response: None })
    ));
  }

  #[test]
  fn notice_loops_are_cut_off() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    let mut saw_failure = false;
    for _ in 0..10 {
      let outcome = flow
        .on_response(Response::Notice(Notice {
          kind: NoticeKind::Info,
          message: "again".to_string(),
        }))
        .unwrap();
      if matches!(outcome.effect, Effect::Failed(_)) {
        saw_failure = true;
        break;
      }
    }
    assert!(saw_failure, "the flow never gave up on notices");
  }

  #[test]
  fn auth_errors_return_to_idle_for_a_retry() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    let _ = flow.on_response(Response::Prompt {
      kind: PromptKind::Secret,
      message: "Password:".to_string(),
    });
    let _ = flow.answer(crate::secret::Secret::new("wrong".into()));
    let outcome = flow
      .on_response(Response::Error {
        auth_failure: true,
        message: String::new(),
      })
      .unwrap();
    match outcome.effect {
      Effect::Failed(failure) => {
        assert_eq!(failure.message, "Login incorrect");
        assert!(!failure.during_start);
      }
      _ => panic!("expected failure"),
    }
    // The same machine must accept another attempt.
    assert!(matches!(
      expect_request(flow.begin("alice", launch())),
      Outgoing::CreateSession { .. }
    ));
  }

  #[test]
  fn start_errors_are_reported_as_session_problems() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    let _ = flow.on_response(Response::Prompt {
      kind: PromptKind::Secret,
      message: "Password:".to_string(),
    });
    let _ = flow.answer(crate::secret::Secret::new("pw".into()));
    let _ = flow.on_response(Response::Success);
    let outcome = flow
      .on_response(Response::Error {
        auth_failure: false,
        message: "exec failed".to_string(),
      })
      .unwrap();
    match outcome.effect {
      Effect::Failed(failure) => {
        assert!(failure.during_start);
        assert_eq!(failure.message, "exec failed");
      }
      _ => panic!("expected failure"),
    }
  }

  #[test]
  fn transport_errors_are_visible_and_reset_the_flow() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    let outcome = flow.on_transport_error("connection reset");
    match outcome.effect {
      Effect::Failed(failure) => assert!(failure.message.contains("connection reset")),
      _ => panic!("expected failure"),
    }
    assert!(flow.cancel().is_none());
  }

  #[test]
  fn answering_without_a_prompt_is_rejected() {
    let mut flow = Flow::new();
    let _ = flow.begin("alice", launch());
    assert!(
      flow
        .answer(crate::secret::Secret::new("pw".into()))
        .is_err()
    );
  }

  #[test]
  fn cancel_only_sends_a_request_when_active() {
    let mut flow = Flow::new();
    assert!(flow.cancel().is_none());
    let _ = flow.begin("alice", launch());
    assert!(matches!(flow.cancel(), Some(Outgoing::Cancel)));
  }

  #[test]
  fn empty_usernames_and_missing_sessions_never_reach_greetd() {
    let mut flow = Flow::new();
    assert!(matches!(
      flow.begin("   ", launch()).effect,
      Effect::Failed(_)
    ));
    assert!(matches!(
      flow
        .begin(
          "alice",
          Launch {
            cmd: Vec::new(),
            env: Vec::new()
          }
        )
        .effect,
      Effect::Failed(_)
    ));
  }
}
