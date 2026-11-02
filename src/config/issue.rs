//! Config issues and the loaded-config wrapper.
//!
//! Every fallback records an [`Issue`] instead of failing the whole file. The
//! UI only shows a one-line summary; `--check-config` prints the details.

use super::*;

/// A problem with the config file. Field-level problems are recoverable: the
/// field uses its default and every other setting still applies.
#[derive(Debug, Clone)]
pub struct Issue {
  /// Dotted key path, for example `ui.font_size`. Per-output overrides append
  /// the output name, as in `background.monitors.DP-1.image`, so the message
  /// points at the exact table that has the problem.
  pub key: String,
  pub location: Option<Location>,
  pub message: String,
}

#[derive(Debug, Clone)]
pub struct Location {
  pub file: PathBuf,
  pub line: usize,
  pub column: usize,
}

impl fmt::Display for Issue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match &self.location {
      Some(location) => write!(
        f,
        "{} ({}:{}:{}): {}",
        self.key,
        location.file.display(),
        location.line,
        location.column,
        self.message
      ),
      None => write!(f, "{}: {}", self.key, self.message),
    }
  }
}

/// The result of loading config: always a usable value, plus diagnostics.
#[derive(Debug)]
pub struct LoadedConfig {
  pub config: Config,
  pub path: Option<PathBuf>,
  pub issues: Vec<Issue>,
  /// A `[theme].package` id from the parsed file. The caller resolves it so the
  /// config layer does not depend on the theme store.
  pub package: Option<String>,
}

impl LoadedConfig {
  /// A one-line summary suitable for the login screen. We intentionally do
  /// not paint the entire error there: it would push the password field
  /// around, and an admin reading this at boot can check the log.
  #[must_use]
  pub fn summary(&self) -> Option<String> {
    let first = self.issues.first()?;
    let more = self.issues.len().saturating_sub(1);
    let mut text = format!("Config warning: {first}");
    if more > 0 {
      text.push_str(&format!(" (+{more} more, see the log)"));
    }
    Some(text)
  }
}
