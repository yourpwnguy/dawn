//! Backends that can execute auth requests.
//!
//! The worker only needs "send this request, give me a reply". Having the
//! trait lets the dry-run mode use exactly the same state machine and worker
//! code as the real greeter, which is how `--dry-run` can be trusted to catch
//! UI and state bugs before a user relies on it at boot.

use std::path::PathBuf;
use std::time::Duration;

use crate::auth::client::{ClientError, GreetdClient};
use crate::auth::flow::Outgoing;
use crate::auth::model::{PromptKind, Response};

/// A transport for auth requests.
pub trait Backend: Send {
  fn request(&mut self, request: Outgoing) -> Result<Response, ClientError>;
}

/// The real greetd connection. It connects on first use and drops the socket
/// after any transport error, so a restarted greetd is picked up on the next
/// login attempt instead of leaving the greeter permanently broken.
pub struct GreetdBackend {
  socket: Option<PathBuf>,
  timeout: Duration,
  client: Option<GreetdClient>,
}

impl GreetdBackend {
  #[must_use]
  pub fn new(socket: Option<PathBuf>, timeout: Duration) -> Self {
    Self {
      socket,
      timeout,
      client: None,
    }
  }
}

impl Backend for GreetdBackend {
  fn request(&mut self, request: Outgoing) -> Result<Response, ClientError> {
    if self.client.is_none() {
      // Cancelling a session that was never created is a no-op. This
      // matters when the user presses Escape before the first login
      // attempt, or after the socket disappeared.
      if matches!(&request, Outgoing::Cancel) {
        return Ok(Response::Success);
      }
      self.client = Some(GreetdClient::connect(self.socket.as_deref(), self.timeout)?);
    }

    let Some(client) = self.client.as_mut() else {
      return Err(ClientError::MissingSocket);
    };
    let result = client.request(request);
    if result.is_err() {
      // A timed out read may have left us mid-frame, so this connection
      // can never be trusted again.
      self.client = None;
    }
    result
  }
}

/// Accepts any non-empty password. Used by `--dry-run` so the full UI and auth
/// flow can be exercised without PAM, a real user, or a logged-out session.
pub struct DryRunBackend {
  stage: Stage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
  Idle,
  AwaitingPassword,
  Authenticated,
}

impl DryRunBackend {
  #[must_use]
  pub fn new() -> Self {
    Self { stage: Stage::Idle }
  }
}

impl Default for DryRunBackend {
  fn default() -> Self {
    Self::new()
  }
}

impl Backend for DryRunBackend {
  fn request(&mut self, request: Outgoing) -> Result<Response, ClientError> {
    match request {
      Outgoing::CreateSession { username } => {
        self.stage = Stage::AwaitingPassword;
        Ok(Response::Prompt {
          kind: PromptKind::Secret,
          // The username is safe to echo: it is not a credential.
          message: format!("Password for {username} (dry run: anything non-empty works)"),
        })
      }
      Outgoing::Answer { response } => {
        let Some(secret) = response else {
          return Ok(Response::Error {
            auth_failure: false,
            message: "dry-run backend does not expect informational acknowledgements".to_string(),
          });
        };
        if self.stage != Stage::AwaitingPassword {
          return Ok(Response::Error {
            auth_failure: false,
            message: "dry-run backend got an answer out of order".to_string(),
          });
        }
        if secret.is_empty() {
          return Ok(Response::Error {
            auth_failure: true,
            message: "Empty password. Dry run accepts any non-empty password.".to_string(),
          });
        }
        self.stage = Stage::Authenticated;
        Ok(Response::Success)
      }
      Outgoing::Start(_) => {
        if self.stage == Stage::Authenticated {
          self.stage = Stage::Idle;
          Ok(Response::Success)
        } else {
          Ok(Response::Error {
            auth_failure: false,
            message: "dry-run backend refused to start before authentication".to_string(),
          })
        }
      }
      Outgoing::Cancel => {
        self.stage = Stage::Idle;
        Ok(Response::Success)
      }
    }
  }
}
