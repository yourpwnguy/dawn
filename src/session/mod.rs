//! Desktop session discovery.
//!
//! greetd does not know which desktops are installed; the greeter has to find
//! them. We read the two standard session directories, parse each `.desktop`
//! file, and hand the UI a stable, sorted list. Broken files are skipped with
//! a warning instead of failing the whole list: one bad package must not stop
//! the user from logging into a good session.

mod desktop;
mod exec;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use self::exec::parse_exec;

/// A graphical session the user can log into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
  /// Desktop file id, for example `hyprland`.
  pub id: String,
  /// Human readable name from `Name=`.
  pub name: String,
  /// argv for greetd's `start_session`.
  pub command: Vec<String>,
  /// `DesktopNames=`, split into individual names.
  pub desktop_names: Vec<String>,
}

impl Session {
  /// Environment that identifies the session to applications.
  ///
  /// We do not invent `XDG_SESSION_TYPE`: the session's own launcher knows
  /// whether it is Wayland or X11 and sets it. Guessing here has historically
  /// produced sessions that think they are the wrong type.
  #[must_use]
  pub fn env(&self) -> Vec<String> {
    let desktop = self
      .desktop_names
      .first()
      .map_or(self.id.as_str(), String::as_str);
    let mut env = vec![
      format!("DESKTOP_SESSION={}", self.id),
      format!("XDG_SESSION_DESKTOP={desktop}"),
    ];
    if !self.desktop_names.is_empty() {
      env.push(format!(
        "XDG_CURRENT_DESKTOP={}",
        self.desktop_names.join(":")
      ));
    }
    env
  }
}

/// Reads every configured directory and returns sessions sorted by name.
#[must_use]
pub fn discover(dirs: &[PathBuf]) -> Vec<Session> {
  const MAX_SESSIONS: usize = 256;
  const MAX_FILE_BYTES: u64 = 64 * 1024;

  let mut sessions = Vec::new();
  let mut seen = HashSet::new();

  for dir in dirs {
    let entries = match std::fs::read_dir(dir) {
      Ok(entries) => entries,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
        debug!(path = %dir.display(), "session directory does not exist");
        continue;
      }
      Err(error) => {
        warn!(path = %dir.display(), %error, "cannot read session directory");
        continue;
      }
    };

    for entry in entries.flatten() {
      if sessions.len() >= MAX_SESSIONS {
        warn!("session limit reached, ignoring the rest");
        break;
      }
      let path = entry.path();
      if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
        continue;
      }
      let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
        continue;
      };
      if !valid_id(id) || seen.contains(id) {
        debug!(path = %path.display(), "skipping session with an invalid or duplicate id");
        continue;
      }

      match load_entry(&path, id, MAX_FILE_BYTES) {
        Ok(Some(session)) => {
          debug!(id = %session.id, name = %session.name, "found session");
          seen.insert(session.id.clone());
          sessions.push(session);
        }
        Ok(None) => {}
        Err(message) => warn!(path = %path.display(), %message, "skipping session file"),
      }
    }
  }

  sessions.sort_by(|left, right| {
    left
      .name
      .to_lowercase()
      .cmp(&right.name.to_lowercase())
      .then_with(|| left.id.cmp(&right.id))
  });
  info!(count = sessions.len(), "loaded desktop sessions");
  sessions
}

/// Returns `Ok(None)` for files that are valid but intentionally hidden.
fn load_entry(path: &Path, id: &str, max_bytes: u64) -> Result<Option<Session>, String> {
  let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
  if !metadata.is_file() {
    return Ok(None);
  }
  if metadata.len() > max_bytes {
    return Err("file is too large".to_string());
  }
  let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
  let entry = desktop::parse(&content);

  if entry.hidden || entry.no_display {
    return Ok(None);
  }
  if let Some(entry_type) = entry.entry_type.as_deref()
    && !entry_type.eq_ignore_ascii_case("Application")
  {
    return Ok(None);
  }
  if let Some(try_exec) = entry.try_exec.as_deref() {
    let program = try_exec.trim();
    if program.is_empty() || program.chars().any(char::is_whitespace) || !program_exists(program) {
      debug!(id, program, "TryExec is not available, hiding session");
      return Ok(None);
    }
  }

  let Some(exec) = entry.exec.as_deref() else {
    return Err("missing Exec".to_string());
  };
  let command = parse_exec(exec).map_err(|error| error.to_string())?;

  let name = entry
    .name
    .as_deref()
    .map(str::trim)
    .filter(|name| !name.is_empty())
    .unwrap_or(id)
    .to_string();
  let desktop_names = parse_desktop_names(entry.desktop_names.as_deref());

  Ok(Some(Session {
    id: id.to_string(),
    name,
    command,
    desktop_names,
  }))
}

fn parse_desktop_names(raw: Option<&str>) -> Vec<String> {
  raw
    .unwrap_or_default()
    .split(';')
    .map(str::trim)
    .filter(|name| !name.is_empty())
    .filter(|name| name.chars().count() <= 64 && !name.chars().any(char::is_control))
    .take(16)
    .map(ToString::to_string)
    .collect()
}

fn valid_id(id: &str) -> bool {
  !id.is_empty()
    && id.len() <= 128
    && id
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// Resolves a program the same way the spec's TryExec hint intends: an
/// absolute/relative path with the executable bit, or a bare name found on
/// PATH. `std::process::Command` would do this later, but we need the answer
/// before showing the session.
fn program_exists(program: &str) -> bool {
  if program.contains('/') {
    return is_executable_file(Path::new(program));
  }
  let Some(path) = std::env::var_os("PATH") else {
    return false;
  };
  std::env::split_paths(&path).any(|dir| is_executable_file(&dir.join(program)))
}

fn is_executable_file(path: &Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  match std::fs::metadata(path) {
    Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
    Err(_) => false,
  }
}

#[cfg(test)]
mod tests {
  use super::{Session, discover, parse_desktop_names};
  use std::path::{Path, PathBuf};
  use std::sync::atomic::{AtomicU64, Ordering};

  static COUNTER: AtomicU64 = AtomicU64::new(0);

  struct TempDir(PathBuf);
  impl TempDir {
    fn new() -> Self {
      let id = COUNTER.fetch_add(1, Ordering::Relaxed);
      let path = std::env::temp_dir().join(format!("dawn-session-{}-{id}", std::process::id()));
      std::fs::create_dir_all(&path).unwrap();
      Self(path)
    }
    fn path(&self) -> &Path {
      &self.0
    }
    fn write(&self, name: &str, contents: &str) {
      std::fs::write(self.0.join(name), contents).unwrap();
    }
  }
  impl Drop for TempDir {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.0);
    }
  }

  #[test]
  fn discovers_and_sorts_sessions() {
    let dir = TempDir::new();
    dir.write(
      "hyprland.desktop",
      "[Desktop Entry]\nType=Application\nName=Hyprland\nExec=Hyprland --start\nDesktopNames=Hyprland;wlroots\n",
    );
    dir.write(
      "xfce.desktop",
      "[Desktop Entry]\nType=Application\nName=Xfce\nExec=startxfce4\n",
    );
    let sessions = discover(&[dir.path().to_path_buf()]);
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name, "Hyprland");
    assert_eq!(sessions[1].name, "Xfce");
    assert_eq!(
      sessions[0].env(),
      [
        "DESKTOP_SESSION=hyprland",
        "XDG_SESSION_DESKTOP=Hyprland",
        "XDG_CURRENT_DESKTOP=Hyprland:wlroots"
      ]
    );
  }

  #[test]
  fn skips_hidden_broken_and_non_desktop_entries() {
    let dir = TempDir::new();
    dir.write(
      "hidden.desktop",
      "[Desktop Entry]\nName=Hidden\nExec=x\nNoDisplay=true\n",
    );
    dir.write("broken.desktop", "[Desktop Entry]\nName=NoExec\n");
    dir.write("readme.txt", "not a session");
    dir.write(
      "good.desktop",
      "[Desktop Entry]\nName=Good\nExec=good --flag\n",
    );
    let sessions = discover(&[dir.path().to_path_buf()]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "good");
  }

  #[test]
  fn first_directory_wins_for_duplicate_ids() {
    let first = TempDir::new();
    let second = TempDir::new();
    first.write("same.desktop", "[Desktop Entry]\nName=First\nExec=first\n");
    second.write(
      "same.desktop",
      "[Desktop Entry]\nName=Second\nExec=second\n",
    );
    let sessions = discover(&[first.path().to_path_buf(), second.path().to_path_buf()]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "First");
  }

  #[test]
  fn missing_try_exec_hides_the_session() {
    let dir = TempDir::new();
    dir.write(
      "gone.desktop",
      "[Desktop Entry]\nName=Gone\nExec=gone\nTryExec=dawn-does-not-exist-anywhere\n",
    );
    assert!(discover(&[dir.path().to_path_buf()]).is_empty());
  }

  #[test]
  fn desktop_names_are_bounded_and_cleaned() {
    let names = parse_desktop_names(Some("A; ;B;D"));
    assert_eq!(names, ["A", "B", "D"]);
  }

  #[test]
  fn missing_directories_are_not_fatal() {
    let sessions = discover(&[PathBuf::from("/nonexistent/dawn/test/sessions")]);
    assert!(sessions.is_empty());
  }

  fn _assert_env_stays_stable(session: &Session) {
    let _ = session.env();
  }
}
