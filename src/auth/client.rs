//! The greetd IPC client.
//!
//! greetd speaks a very small framed protocol on a Unix socket: a native
//! endian `u32` payload length followed by a UTF-8 JSON document. A newline
//! delimited protocol would have been nicer, but this is the protocol greetd
//! actually implements (`greetd-ipc(7)`).
//!
//! This module owns framing, length limits, timeouts, and turning greetd's
//! JSON into [`Response`]. It does not own the auth flow; see
//! [`crate::auth::flow`].

use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::auth::flow::Outgoing;
use crate::auth::model::{Notice, NoticeKind, PromptKind, Response, clean_message};
use crate::secret::Secret;

/// A legitimate greetd reply is a few hundred bytes. One MiB leaves plenty of
/// room for a verbose PAM module while preventing a broken or hostile socket
/// from making the greeter allocate arbitrary amounts of memory.
const MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
  #[error(
    "greetd socket is not configured: GREETD_SOCK is unset and auth.socket is empty. \
         Start dawn from greetd, or set auth.socket for a dry-run socket."
  )]
  MissingSocket,

  #[error("cannot connect to greetd at {path}: {source}")]
  Connect {
    path: PathBuf,
    #[source]
    source: io::Error,
  },

  #[error("talking to greetd failed: {0}")]
  Io(#[from] io::Error),

  #[error("greetd did not answer within {0:?}")]
  Timeout(Duration),

  #[error("greetd sent a {size} byte message, over the {limit} byte limit")]
  FrameTooLarge { size: usize, limit: usize },

  #[error("greetd sent an unreadable message: {0}")]
  Decode(#[from] serde_json::Error),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireRequest<'a> {
  CreateSession {
    username: &'a str,
  },
  PostAuthMessageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<&'a str>,
  },
  StartSession {
    cmd: &'a [String],
    env: &'a [String],
  },
  CancelSession,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireResponse {
  Success,
  Error {
    error_type: WireErrorKind,
    #[serde(default)]
    description: String,
  },
  AuthMessage {
    auth_message_type: WireMessageKind,
    #[serde(default)]
    auth_message: String,
  },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireErrorKind {
  Error,
  AuthError,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireMessageKind {
  Visible,
  Secret,
  Info,
  Error,
}

/// A connected greetd session.
///
/// The socket has read and write timeouts because a hung PAM module would
/// otherwise freeze the auth worker forever. On any transport error the caller
/// drops the client; the next login attempt opens a fresh connection.
pub struct GreetdClient {
  stream: UnixStream,
  timeout: Duration,
}

impl GreetdClient {
  /// Connects using the configured socket override, falling back to
  /// `GREETD_SOCK` which greetd sets for every greeter it launches.
  pub fn connect(socket: Option<&Path>, timeout: Duration) -> Result<Self, ClientError> {
    let path = socket_path(socket)?;
    let stream = UnixStream::connect(&path).map_err(|source| ClientError::Connect {
      path: path.clone(),
      source,
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(Self { stream, timeout })
  }

  /// Performs one complete request/reply exchange.
  pub fn request(&mut self, request: Outgoing) -> Result<Response, ClientError> {
    // Build the wire view by borrowing the request. This is the whole reason
    // `Outgoing` is matched by reference: the password lives in the request
    // for exactly as long as the socket write, and nothing is copied into a
    // long-lived buffer.
    let wire = match &request {
      Outgoing::CreateSession { username } => WireRequest::CreateSession { username },
      Outgoing::Answer { response } => WireRequest::PostAuthMessageResponse {
        response: response.as_ref().map(Secret::expose),
      },
      Outgoing::Start(launch) => WireRequest::StartSession {
        cmd: &launch.cmd,
        env: &launch.env,
      },
      Outgoing::Cancel => WireRequest::CancelSession,
    };

    // The JSON buffer can contain a password, so it is wiped on drop.
    let body = Zeroizing::new(serde_json::to_vec(&wire)?);
    if body.len() > MAX_FRAME {
      return Err(ClientError::FrameTooLarge {
        size: body.len(),
        limit: MAX_FRAME,
      });
    }

    let length = u32::try_from(body.len()).map_err(|_| ClientError::FrameTooLarge {
      size: body.len(),
      limit: MAX_FRAME,
    })?;
    self.stream.write_all(&length.to_ne_bytes())?;
    self.stream.write_all(&body)?;

    let reply = self.read_frame()?;
    Ok(from_wire(reply))
  }

  fn read_frame(&mut self) -> Result<WireResponse, ClientError> {
    let mut length = [0u8; 4];
    self.read_exact(&mut length)?;
    let size = usize::try_from(u32::from_ne_bytes(length)).unwrap_or(usize::MAX);
    if size > MAX_FRAME {
      return Err(ClientError::FrameTooLarge {
        size,
        limit: MAX_FRAME,
      });
    }

    let mut body = Zeroizing::new(vec![0u8; size]);
    self.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
  }

  fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ClientError> {
    match self.stream.read_exact(buf) {
      Ok(()) => Ok(()),
      Err(error) if is_timeout(&error) => Err(ClientError::Timeout(self.timeout)),
      Err(error) => Err(ClientError::Io(error)),
    }
  }
}

/// Resolves the greetd socket path. An explicit path wins over the environment
/// so a test harness can point the greeter at a fake server.
pub fn socket_path(explicit: Option<&Path>) -> Result<PathBuf, ClientError> {
  if let Some(path) = explicit.filter(|path| !path.as_os_str().is_empty()) {
    return Ok(path.to_path_buf());
  }
  match env::var_os("GREETD_SOCK") {
    Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
    _ => Err(ClientError::MissingSocket),
  }
}

fn is_timeout(error: &io::Error) -> bool {
  matches!(
    error.kind(),
    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
  )
}

fn from_wire(reply: WireResponse) -> Response {
  match reply {
    WireResponse::Success => Response::Success,
    WireResponse::Error {
      error_type,
      description,
    } => Response::Error {
      auth_failure: matches!(error_type, WireErrorKind::AuthError),
      message: clean_message(&description),
    },
    WireResponse::AuthMessage {
      auth_message_type,
      auth_message,
    } => {
      let message = clean_message(&auth_message);
      match auth_message_type {
        WireMessageKind::Visible => Response::Prompt {
          kind: PromptKind::Visible,
          message,
        },
        WireMessageKind::Secret => Response::Prompt {
          kind: PromptKind::Secret,
          message,
        },
        WireMessageKind::Info => Response::Notice(Notice {
          kind: NoticeKind::Info,
          message,
        }),
        WireMessageKind::Error => Response::Notice(Notice {
          kind: NoticeKind::Error,
          message,
        }),
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::io::{Read, Write};
  use std::os::unix::net::UnixListener;
  use std::path::PathBuf;
  use std::thread;
  use std::time::{Duration, SystemTime, UNIX_EPOCH};

  use super::{ClientError, GreetdClient, MAX_FRAME};
  use crate::auth::flow::Outgoing;
  use crate::auth::model::{NoticeKind, PromptKind, Response};
  use crate::secret::Secret;

  /// A socket path that is cleaned up even if the test fails an assertion.
  struct TempSocket(PathBuf);
  impl TempSocket {
    fn new(tag: &str) -> Self {
      let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
      let path =
        std::env::temp_dir().join(format!("dawn-{tag}-{}-{unique}.sock", std::process::id()));
      let _ = std::fs::remove_file(&path);
      Self(path)
    }
  }
  impl Drop for TempSocket {
    fn drop(&mut self) {
      let _ = std::fs::remove_file(&self.0);
    }
  }

  fn read_frame(stream: &mut std::os::unix::net::UnixStream) -> serde_json::Value {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).unwrap();
    let mut body = vec![0u8; u32::from_ne_bytes(len) as usize];
    stream.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
  }

  fn write_frame(stream: &mut std::os::unix::net::UnixStream, body: &str) {
    stream
      .write_all(&(body.len() as u32).to_ne_bytes())
      .unwrap();
    stream.write_all(body.as_bytes()).unwrap();
  }

  #[test]
  fn sends_create_session_and_reads_a_prompt() {
    let temp = TempSocket::new("create");
    let listener = UnixListener::bind(&temp.0).unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let request = read_frame(&mut stream);
      assert_eq!(request["type"], "create_session");
      assert_eq!(request["username"], "alice");
      write_frame(
        &mut stream,
        r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password:"}"#,
      );
    });

    let mut client = GreetdClient::connect(Some(&temp.0), Duration::from_secs(1)).unwrap();
    let response = client
      .request(Outgoing::CreateSession {
        username: "alice".to_string(),
      })
      .unwrap();
    assert_eq!(
      response,
      Response::Prompt {
        kind: PromptKind::Secret,
        message: "Password:".to_string()
      }
    );
    server.join().unwrap();
  }

  #[test]
  fn serialises_answers_without_leaking_them_into_other_fields() {
    let temp = TempSocket::new("answer");
    let listener = UnixListener::bind(&temp.0).unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let request = read_frame(&mut stream);
      assert_eq!(request["type"], "post_auth_message_response");
      assert_eq!(request["response"], "hunter2");
      assert_eq!(request.as_object().unwrap().len(), 2);
      write_frame(&mut stream, r#"{"type":"success"}"#);
    });

    let mut client = GreetdClient::connect(Some(&temp.0), Duration::from_secs(1)).unwrap();
    let response = client
      .request(Outgoing::Answer {
        response: Some(Secret::new("hunter2".to_string())),
      })
      .unwrap();
    assert_eq!(response, Response::Success);
    server.join().unwrap();
  }

  #[test]
  fn acknowledges_notices_with_no_response_field() {
    let temp = TempSocket::new("ack");
    let listener = UnixListener::bind(&temp.0).unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let request = read_frame(&mut stream);
      assert_eq!(request["type"], "post_auth_message_response");
      assert!(request.get("response").is_none());
      write_frame(&mut stream, r#"{"type":"success"}"#);
    });

    let mut client = GreetdClient::connect(Some(&temp.0), Duration::from_secs(1)).unwrap();
    client.request(Outgoing::Answer { response: None }).unwrap();
    server.join().unwrap();
  }

  #[test]
  fn maps_info_and_auth_errors_to_domain_types() {
    let temp = TempSocket::new("types");
    let listener = UnixListener::bind(&temp.0).unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let _ = read_frame(&mut stream);
      write_frame(
        &mut stream,
        r#"{"type":"auth_message","auth_message_type":"info","auth_message":"hello\nworld"}"#,
      );
    });

    let mut client = GreetdClient::connect(Some(&temp.0), Duration::from_secs(1)).unwrap();
    let response = client
      .request(Outgoing::CreateSession {
        username: "alice".to_string(),
      })
      .unwrap();
    assert_eq!(
      response,
      Response::Notice(crate::auth::model::Notice {
        kind: NoticeKind::Info,
        message: "hello world".to_string()
      })
    );
    server.join().unwrap();
  }

  #[test]
  fn rejects_oversized_frames_before_allocating() {
    let temp = TempSocket::new("huge");
    let listener = UnixListener::bind(&temp.0).unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let _ = read_frame(&mut stream);
      stream
        .write_all(&((MAX_FRAME as u32) + 1).to_ne_bytes())
        .unwrap();
    });

    let mut client = GreetdClient::connect(Some(&temp.0), Duration::from_secs(1)).unwrap();
    let error = client
      .request(Outgoing::CreateSession {
        username: "alice".to_string(),
      })
      .unwrap_err();
    assert!(matches!(error, ClientError::FrameTooLarge { .. }));
    server.join().unwrap();
  }

  #[test]
  fn times_out_instead_of_hanging_the_worker() {
    let temp = TempSocket::new("timeout");
    let listener = UnixListener::bind(&temp.0).unwrap();
    let server = thread::spawn(move || {
      let (_stream, _) = listener.accept().unwrap();
      thread::sleep(Duration::from_millis(250));
    });

    let mut client = GreetdClient::connect(Some(&temp.0), Duration::from_millis(60)).unwrap();
    let error = client
      .request(Outgoing::CreateSession {
        username: "alice".to_string(),
      })
      .unwrap_err();
    assert!(matches!(error, ClientError::Timeout(_)));
    server.join().unwrap();
  }
}
