//! Credential storage.
//!
//! A password is only ever held by this type. It wraps a [`Zeroizing<String>`]
//! so the heap buffer is wiped when the value is dropped, including when a
//! queued auth command is discarded because the window closed mid-login.
//!
//! There is deliberately no `Clone`, no `Display`, and the `Debug` impl prints
//! a redaction marker. The only way to read the bytes is [`Secret::expose`],
//! which is crate-private, so a plugin or a config structure cannot reach it.

use std::fmt;

use zeroize::Zeroizing;

/// A password or another credential answer that must not linger in memory.
pub struct Secret(Zeroizing<String>);

impl Secret {
  /// Takes ownership of the raw value, so callers must not keep a copy.
  #[must_use]
  pub fn new(value: String) -> Self {
    Self(Zeroizing::new(value))
  }

  /// Borrows the plaintext for the single place that has to send it to
  /// greetd. This is intentionally not `pub`: plugins and UI code live
  /// outside this module and must never call it.
  pub(crate) fn expose(&self) -> &str {
    &self.0
  }

  /// True when the user submitted an empty answer. greetd, not dawn, decides
  /// whether an empty password is acceptable (PAM configurations differ).
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
}

impl fmt::Debug for Secret {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Secret(<redacted>)")
  }
}

impl From<String> for Secret {
  fn from(value: String) -> Self {
    Self::new(value)
  }
}

#[cfg(test)]
mod tests {
  use super::Secret;

  #[test]
  fn debug_never_prints_the_value() {
    let secret = Secret::new("hunter2".to_string());
    let rendered = format!("{secret:?}");
    assert_eq!(rendered, "Secret(<redacted>)");
    assert!(!rendered.contains("hunter2"));
  }

  #[test]
  fn empty_is_detected() {
    assert!(Secret::new(String::new()).is_empty());
    assert!(!Secret::new("x".to_string()).is_empty());
  }

  #[test]
  fn expose_returns_the_exact_bytes() {
    let secret = Secret::new("pässwörd".to_string());
    assert_eq!(secret.expose(), "pässwörd");
  }
}
