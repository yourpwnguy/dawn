//! The auth worker thread.
//!
//! greetd calls are blocking and can take as long as PAM wants. Running them
//! on the UI thread would freeze the window and, worse, make the greeter
//! unresponsive exactly when the user is trying to recover from a failed
//! login. This module runs the state machine on a dedicated thread and ships
//! events back through a callback (the GUI uses winit's event-loop proxy, the
//! TTY frontend drives the flow synchronously and does not need this worker).

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use crate::auth::backend::{Backend, DryRunBackend, GreetdBackend};
use crate::auth::flow::{Effect, Flow, Outcome, Outgoing};
use crate::auth::model::{Failure, Launch, Notice, Prompt};
use crate::secret::Secret;

/// Used only to stop a runaway request/reply loop. The flow already caps
/// informational message bursts; this is the belt-and-braces limit.
const MAX_STEPS_PER_COMMAND: usize = 64;

/// Commands the UI sends to the worker.
enum Command {
  Begin { username: String, launch: Launch },
  Answer { response: Secret },
  Cancel,
  Shutdown,
}

/// Events the worker sends to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
  /// Show this prompt and wait for the user's answer.
  Prompt(Prompt),
  /// Display a PAM message. The worker has already acknowledged it.
  Notice(Notice),
  /// Authentication succeeded; the session command is being started.
  Starting,
  /// `start_session` succeeded. The frontend must exit so greetd can launch
  /// the session.
  Started,
  /// Something failed. The worker is idle again and accepts a new attempt.
  Failed(Failure),
  /// A pending login was cancelled.
  Cancelled,
}

/// The UI's handle to the worker. Dropping it asks the worker to stop.
pub struct WorkerHandle {
  tx: Sender<Command>,
}

impl WorkerHandle {
  /// Sends a login attempt. Returns false if the worker thread is gone, in
  /// which case the UI shows a failure rather than silently doing nothing.
  #[must_use]
  pub fn begin(&self, username: &str, launch: Launch) -> bool {
    self
      .tx
      .send(Command::Begin {
        username: username.to_string(),
        launch,
      })
      .is_ok()
  }

  /// Sends the user's answer. The secret is moved through the channel.
  #[must_use]
  pub fn answer(&self, response: Secret) -> bool {
    self.tx.send(Command::Answer { response }).is_ok()
  }

  /// Cancels whatever is in flight. Safe to call at any time.
  #[must_use]
  pub fn cancel(&self) -> bool {
    self.tx.send(Command::Cancel).is_ok()
  }
}

impl Drop for WorkerHandle {
  fn drop(&mut self) {
    let _ = self.tx.send(Command::Shutdown);
  }
}

/// Which auth backend the worker should use.
pub enum Mode {
  Greetd {
    socket: Option<PathBuf>,
    timeout: Duration,
  },
  DryRun,
}

/// Starts the worker. `emit` is called from the worker thread; it must be cheap
/// and must not block (winit's proxy and an mpsc sender both qualify).
pub fn spawn(mode: Mode, emit: impl Fn(Event) + Send + 'static) -> std::io::Result<WorkerHandle> {
  let backend: Box<dyn Backend> = match mode {
    Mode::Greetd { socket, timeout } => Box::new(GreetdBackend::new(socket, timeout)),
    Mode::DryRun => Box::new(DryRunBackend::new()),
  };
  spawn_with_backend(backend, emit)
}

/// Testable constructor: lets a test inject a scripted backend.
pub(crate) fn spawn_with_backend(
  mut backend: Box<dyn Backend>,
  mut emit: impl FnMut(Event) + Send + 'static,
) -> std::io::Result<WorkerHandle> {
  let (tx, rx) = mpsc::channel();
  thread::Builder::new()
    .name("dawn-auth".to_string())
    .spawn(move || {
      let mut flow = Flow::new();
      while let Ok(command) = rx.recv() {
        match command {
          Command::Begin { username, launch } => {
            let outcome = flow.begin(&username, launch);
            drive(backend.as_mut(), &mut flow, outcome, &mut emit);
          }
          Command::Answer { response } => match flow.answer(response) {
            Ok(outcome) => drive(backend.as_mut(), &mut flow, outcome, &mut emit),
            Err(error) => {
              let outcome = flow.abort(error.to_string());
              drive(backend.as_mut(), &mut flow, outcome, &mut emit);
            }
          },
          Command::Cancel => {
            if let Some(request) = flow.cancel() {
              // A failed cancel is not worth a scary message:
              // the user wanted to get out, and the flow is
              // already reset. The backend drops a broken socket.
              let _ = backend.request(request);
            }
            emit(Event::Cancelled);
          }
          Command::Shutdown => break,
        }
      }
    })
    .map(|_| WorkerHandle { tx })
}

/// Executes effects until the worker has to wait for the user or finish.
fn drive(
  backend: &mut dyn Backend,
  flow: &mut Flow,
  mut outcome: Outcome,
  emit: &mut impl FnMut(Event),
) {
  let mut steps = 0usize;

  loop {
    for notice in outcome.notices.drain(..) {
      emit(Event::Notice(notice));
    }

    match outcome.effect {
      Effect::Request(request) => {
        if matches!(&request, Outgoing::Start(_)) {
          emit(Event::Starting);
        }
        let result = backend.request(request);
        let next = match result {
          Ok(response) => flow.on_response(response),
          Err(error) => Ok(flow.on_transport_error(&error.to_string())),
        };
        match next {
          Ok(next_outcome) => outcome = next_outcome,
          Err(flow_error) => {
            let failed = flow.abort(flow_error.to_string());
            emit_failure(failed, emit);
            return;
          }
        }

        steps += 1;
        if steps > MAX_STEPS_PER_COMMAND {
          let failed = flow.abort("greetd request loop did not settle");
          emit_failure(failed, emit);
          return;
        }
      }
      Effect::Ask(prompt) => {
        emit(Event::Prompt(prompt));
        return;
      }
      Effect::Completed => {
        emit(Event::Started);
        return;
      }
      Effect::Failed(failure) => {
        emit(Event::Failed(failure));
        return;
      }
      Effect::Cancelled => {
        emit(Event::Cancelled);
        return;
      }
    }
  }
}

fn emit_failure(outcome: Outcome, emit: &mut impl FnMut(Event)) {
  for notice in outcome.notices {
    emit(Event::Notice(notice));
  }
  if let Effect::Failed(failure) = outcome.effect {
    emit(Event::Failed(failure));
  }
}

#[cfg(test)]
mod tests {
  use std::sync::mpsc::{self, Receiver};
  use std::time::Duration;

  use super::{Event, spawn_with_backend};
  use crate::auth::backend::Backend;
  use crate::auth::client::ClientError;
  use crate::auth::flow::Outgoing;
  use crate::auth::model::{Launch, PromptKind, Response};

  /// A tiny backend that always asks for a password and accepts "pw".
  struct Scripted;
  impl Backend for Scripted {
    fn request(&mut self, request: Outgoing) -> Result<Response, ClientError> {
      match request {
        Outgoing::CreateSession { .. } => Ok(Response::Prompt {
          kind: PromptKind::Secret,
          message: "Password:".to_string(),
        }),
        Outgoing::Answer { response } => {
          if response.is_some() {
            Ok(Response::Success)
          } else {
            Ok(Response::Error {
              auth_failure: false,
              message: "no ack expected".to_string(),
            })
          }
        }
        Outgoing::Start(_) => Ok(Response::Success),
        Outgoing::Cancel => Ok(Response::Success),
      }
    }
  }

  fn recv(rx: &Receiver<Event>) -> Event {
    rx.recv_timeout(Duration::from_secs(2))
      .expect("worker did not emit an event")
  }

  fn launch() -> Launch {
    Launch {
      cmd: vec!["true".to_string()],
      env: Vec::new(),
    }
  }

  /// A backend that fails every request, as a missing greetd socket would.
  struct Failing;
  impl Backend for Failing {
    fn request(&mut self, _request: Outgoing) -> Result<Response, ClientError> {
      Err(ClientError::MissingSocket)
    }
  }

  #[test]
  fn transport_errors_become_visible_failures() {
    let (tx, rx) = mpsc::channel();
    let worker = spawn_with_backend(Box::new(Failing), move |event| {
      let _ = tx.send(event);
    })
    .unwrap();

    assert!(worker.begin("alice", launch()));
    match recv(&rx) {
      Event::Failed(failure) => assert!(failure.message.contains("GREETD_SOCK")),
      other => panic!("expected a failure, got {other:?}"),
    }
  }

  #[test]
  fn cancelling_an_idle_conversation_reports_cancelled() {
    let (tx, rx) = mpsc::channel();
    let worker = spawn_with_backend(Box::new(Scripted), move |event| {
      let _ = tx.send(event);
    })
    .unwrap();

    assert!(worker.begin("alice", launch()));
    assert!(matches!(recv(&rx), Event::Prompt(_)));
    assert!(worker.cancel());
    assert!(matches!(recv(&rx), Event::Cancelled));
  }

  #[test]
  fn runs_a_full_login_on_the_worker_thread() {
    let (tx, rx) = mpsc::channel();
    let worker = spawn_with_backend(Box::new(Scripted), move |event| {
      let _ = tx.send(event);
    })
    .unwrap();

    assert!(worker.begin("alice", launch()));
    assert!(matches!(recv(&rx), Event::Prompt(_)));
    assert!(worker.answer(crate::secret::Secret::new("pw".to_string())));
    // The scripted backend returns success, so the worker starts the
    // session immediately and reports that it is starting it.
    assert!(matches!(recv(&rx), Event::Starting));
    assert!(matches!(recv(&rx), Event::Started));
  }
}
