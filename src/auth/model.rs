//! Shared authentication vocabulary.
//!
//! These types are the boundary between the greetd wire protocol, the state
//! machine, and the UI. Keeping them free of transport details means the UI
//! can never see a raw JSON message, and the state machine can be tested
//! without a socket.

/// Whether the user's answer to the current prompt must be hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
  Visible,
  Secret,
}

/// A question from PAM that the user has to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
  pub kind: PromptKind,
  pub message: String,
}

/// The kind of non-question message PAM can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
  Info,
  Error,
}

/// An informational or error message that needs no answer from the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
  pub kind: NoticeKind,
  pub message: String,
}

/// Everything greetd needs to launch the chosen desktop session.
///
/// `env` is a list of `KEY=VALUE` entries. We deliberately keep it as plain
/// strings because greetd's protocol is plain strings; validating it further
/// would only move the failure into greetd's parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
  pub cmd: Vec<String>,
  pub env: Vec<String>,
}

/// A failure that should be shown on the login screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
  /// Message already phrased for a human. It never contains a credential.
  pub message: String,
  /// True when authentication succeeded but the session command failed.
  /// The UI uses this to say "could not start session" rather than
  /// "login failed", which is a much more useful hint for the admin.
  pub during_start: bool,
}

/// A normalised greetd reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
  /// The request was accepted. What that means depends on the request:
  /// after `create_session` / an answer it means "ready to start a session",
  /// after `start_session` it means the session is on its way.
  Success,
  /// greetd refused the request. `auth_failure` distinguishes a wrong
  /// password from a general problem such as PAM being unable to run.
  Error { auth_failure: bool, message: String },
  /// PAM wants a prompt.
  Prompt { kind: PromptKind, message: String },
  /// PAM is telling the user something. greetd still needs an ack, but the
  /// user does not type an answer.
  Notice(Notice),
}

/// Makes an untrusted message safe to paint.
///
/// PAM and greetd are outside our trust boundary even though they are local:
/// a broken PAM module can send newlines, terminal escapes, or megabytes. We
/// collapse every control character into a space, squeeze whitespace, and cap
/// the result so the layout cannot be blown up. The original message is never
/// logged verbatim by this function.
#[must_use]
pub fn clean_message(raw: &str) -> String {
  const LIMIT: usize = 300;

  let mut cleaned = String::with_capacity(raw.len().min(LIMIT + 3));
  let mut last_was_space = true;
  let mut truncated = false;
  let mut kept = 0usize;

  for ch in raw.chars() {
    let ch = if ch.is_control() { ' ' } else { ch };
    if ch == ' ' {
      if last_was_space {
        continue;
      }
      last_was_space = true;
    } else {
      last_was_space = false;
    }
    if kept >= LIMIT {
      truncated = true;
      break;
    }
    kept += 1;
    cleaned.push(ch);
  }

  if truncated {
    cleaned.push_str("...");
  }
  cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
  use super::clean_message;

  #[test]
  fn strips_control_characters_and_squeezes_space() {
    assert_eq!(clean_message("a\n\nb\t c\u{1b}[31m"), "a b c [31m");
  }

  #[test]
  fn bounds_long_messages_at_a_char_boundary() {
    let long = "é".repeat(500);
    let cleaned = clean_message(&long);
    assert!(cleaned.chars().count() <= 303);
    assert!(cleaned.ends_with("..."));
    // The result must still be valid UTF-8, which it is by construction.
    assert!(cleaned.is_char_boundary(cleaned.len()));
  }

  #[test]
  fn empty_messages_stay_empty() {
    assert_eq!(clean_message(""), "");
    assert_eq!(clean_message("   \n"), "");
  }
}
