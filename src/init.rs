//! `dawn init`: write a commented first-run config.
//!
//! This is deliberately the only command that creates a config file. It never
//! runs as a wizard: it resolves one path, refuses to clobber an existing file
//! unless `--force` is given, and writes the embedded template atomically. A
//! failed write therefore leaves the previous config byte for byte intact,
//! which matters because a broken `/etc/dawn/config.toml` can outlive the
//! command that wrote it.
//!
//! The template lives in `src/default-config.toml` and is compiled in with
//! `include_str!`. Keeping it a real TOML file means it can be checked by the
//! same loader as any admin config, with no second Rust copy to maintain.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tempfile::Builder;

use crate::cli::{InitCommand, InitTarget};
use crate::theme::{is_permission_denied, shell_quote};

/// The system config a real greeter reads when the user has no `HOME`. The
/// loader owns the constant, because discovery and `dawn init` must agree on
/// the one default path.
pub use crate::config::SYSTEM_CONFIG;

/// The curated first-run config, checked into the repository as plain TOML.
pub const DEFAULT_CONFIG: &str = include_str!("default-config.toml");

/// Runs `dawn init` and turns every outcome into a process exit code.
pub fn run(command: InitCommand) -> ExitCode {
  let path = resolve_target(&command.target);

  // symlink_metadata catches a dangling symlink too, which `Path::exists`
  // would report as missing and then replace without asking.
  if !command.force && fs::symlink_metadata(&path).is_ok() {
    eprintln!("dawn: config already exists: {}", path.display());
    eprintln!("{}", force_hint(&command, &path));
    return ExitCode::FAILURE;
  }

  match write_atomic(&path, DEFAULT_CONFIG) {
    Ok(()) => {
      print_next_steps(&path);
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("dawn: {}", failure_message(&command, &path, error));
      ExitCode::FAILURE
    }
  }
}

/// Resolves the target against the real process environment.
///
/// The system path is the only default. `--config PATH` is the explicit seam
/// for tests and custom installs, and it is the only way to write anywhere
/// else now that the per-user mode is gone.
fn resolve_target(target: &InitTarget) -> PathBuf {
  match target {
    InitTarget::System => PathBuf::from(SYSTEM_CONFIG),
    InitTarget::Path(path) => path.clone(),
  }
}

/// Writes `contents` to `path` through a temporary sibling and a rename.
///
/// The temp file is created in the destination directory on purpose: a rename
/// across filesystems is a copy, not an atomic replace. On success the old file
/// is replaced in one step, so a reader never sees a half-written config.
fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
  let parent = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));
  fs::create_dir_all(parent)
    .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;

  let mut staging = Builder::new()
    .prefix(".dawn-init-")
    .tempfile_in(parent)
    .map_err(|error| {
      format!(
        "cannot create a staging file in {}: {error}",
        parent.display()
      )
    })?;

  staging
    .write_all(contents.as_bytes())
    .map_err(|error| format!("cannot write {}: {error}", staging.path().display()))?;
  // Flush and sync before the rename so a crash cannot leave an empty file
  // under the final name while the bytes are still in the page cache.
  staging
    .as_file()
    .sync_all()
    .map_err(|error| format!("cannot flush {}: {error}", staging.path().display()))?;
  set_config_mode(staging.path())?;

  staging.persist(path).map_err(|error| {
    format!(
      "cannot move the config into place at {}: {}",
      path.display(),
      error.error
    )
  })?;
  Ok(())
}

/// Config files are read by the unprivileged `greeter` user, so the staging
/// file must not keep tempfile's private 0600 mode.
fn set_config_mode(path: &Path) -> Result<(), String> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
      .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))?;
  }
  Ok(())
}

/// The four informational lines printed after a successful write.
fn print_next_steps(path: &Path) {
  println!("wrote {}", path.display());
  println!("next: install a theme with `sudo dawn add examples/themes/kinich`");
  println!("greetd: command = \"cage -s -- /usr/local/bin/dawn\"");
  println!("then: sudo systemctl enable greetd.service");
}

/// The target flags alone, so both the refusal and the sudo rerun describe the
/// same invocation the admin just ran.
fn target_flags(command: &InitCommand, path: &Path) -> String {
  match &command.target {
    InitTarget::System => String::new(),
    InitTarget::Path(_) => format!(" --config {}", shell_quote(path)),
  }
}

/// One line that tells an admin exactly how to replace the existing file.
fn force_hint(command: &InitCommand, path: &Path) -> String {
  format!(
    "overwrite it with: dawn init{} --force",
    target_flags(command, path)
  )
}

/// The exact rerun line for a permission failure. This never tries to escalate;
/// it only names the command that would work. `--force` is always present
/// because the previous attempt may well have created a file the admin cannot
/// see (a root-owned leftovers are the common case).
fn sudo_rerun(command: &InitCommand, path: &Path) -> String {
  format!("sudo dawn init{} --force", target_flags(command, path))
}

/// Appends the sudo hint once when the OS said the write was not permitted.
fn failure_message(command: &InitCommand, path: &Path, error: String) -> String {
  if is_permission_denied(&error) {
    format!("{error}\nrerun with sudo: {}", sudo_rerun(command, path))
  } else {
    error
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{
    DEFAULT_CONFIG, InitCommand, InitTarget, SYSTEM_CONFIG, failure_message, force_hint,
    resolve_target, run, write_atomic,
  };
  use crate::config;

  fn command(target: InitTarget) -> InitCommand {
    InitCommand {
      target,
      force: false,
    }
  }

  fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-init-{name}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
  }

  #[test]
  fn target_resolution_has_exactly_two_targets() {
    assert_eq!(
      resolve_target(&InitTarget::System),
      PathBuf::from(SYSTEM_CONFIG)
    );
    assert_eq!(
      resolve_target(&InitTarget::Path(PathBuf::from("/tmp/dawn.toml"))),
      PathBuf::from("/tmp/dawn.toml")
    );
    assert_eq!(SYSTEM_CONFIG, "/etc/dawn/config.toml");
  }

  #[test]
  fn refuses_to_overwrite_without_force_and_overwrites_with_it() {
    let dir = temp_dir("overwrite");
    let path = dir.join("config.toml");
    std::fs::write(&path, "original\n").expect("seed config");
    let target = InitTarget::Path(path.clone());

    let refused = run(command(target.clone()));
    assert_ne!(refused, std::process::ExitCode::SUCCESS);
    assert_eq!(
      std::fs::read_to_string(&path).expect("read refused"),
      "original\n"
    );
    let hint = force_hint(&command(target.clone()), &path);
    assert!(hint.contains("dawn init"), "{hint}");
    assert_eq!(hint.matches("--force").count(), 1, "{hint}");
    assert!(hint.contains(path.to_str().expect("utf8 path")), "{hint}");

    let forced = run(InitCommand {
      target,
      force: true,
    });
    assert_eq!(forced, std::process::ExitCode::SUCCESS);
    assert_eq!(
      std::fs::read_to_string(&path).expect("read forced"),
      DEFAULT_CONFIG
    );
  }

  #[test]
  fn written_file_passes_config_validation() {
    let dir = temp_dir("valid");
    let path = dir.join("config.toml");
    let status = run(command(InitTarget::Path(path.clone())));
    assert_eq!(status, std::process::ExitCode::SUCCESS);

    // This is the same entry point `dawn --check-config` uses before it prints
    // its report, minus the session and plugin discovery that a unit test
    // cannot assume on the host.
    let loaded = config::load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.path.as_deref(), Some(path.as_path()));
  }

  #[test]
  fn atomic_write_leaves_the_destination_untouched_on_failure() {
    let dir = temp_dir("atomic");
    // A directory in the destination slot makes the final rename fail after the
    // staging file was fully written, which is exactly the window atomic writes
    // exist to protect.
    let path = dir.join("config.toml");
    std::fs::create_dir(&path).expect("create destination directory");

    let error = write_atomic(&path, "replacement\n").expect_err("rename must fail");
    assert!(path.is_dir(), "{error}");
    assert!(
      std::fs::read_dir(&dir)
        .expect("read dir")
        .all(|entry| !entry
          .expect("entry")
          .file_name()
          .to_string_lossy()
          .starts_with(".dawn-init-")),
      "staging file was left behind"
    );
  }

  #[test]
  fn hint_lines_are_exact_and_single() {
    let path = PathBuf::from(SYSTEM_CONFIG);
    let target = command(InitTarget::System);
    assert_eq!(
      force_hint(&target, &path),
      "overwrite it with: dawn init --force"
    );

    let message = failure_message(
      &target,
      &path,
      "cannot create a staging file in /etc/dawn: Permission denied (os error 13)".to_string(),
    );
    assert_eq!(message.matches("sudo dawn init").count(), 1, "{message}");
    assert!(
      message.ends_with("\nrerun with sudo: sudo dawn init --force"),
      "{message}"
    );

    // Only permission errors earn an escalation hint; a plain error stays as is.
    let plain = failure_message(&target, &path, "not a regular file".to_string());
    assert_eq!(plain, "not a regular file");
  }

  #[cfg(unix)]
  #[test]
  fn permission_failure_adds_one_sudo_hint_and_keeps_the_old_file() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("locked");
    let path = dir.join("config.toml");
    fs::write(&path, "original\n").expect("seed config");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("lock dir");

    // A root test runner ignores mode bits, so there is no EACCES to reproduce.
    if fs::write(dir.join("probe"), b"x").is_ok() {
      fs::remove_file(dir.join("probe")).ok();
      fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("unlock");
      return;
    }

    let error = write_atomic(&path, "replacement\n").expect_err("locked dir must fail");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("unlock");
    assert_eq!(
      fs::read_to_string(&path).expect("old file survives"),
      "original\n"
    );

    let target = InitTarget::Path(path.clone());
    let message = failure_message(&command(target.clone()), &path, error);
    assert!(message.contains("sudo dawn init"), "{message}");
    assert_eq!(message.matches("sudo dawn init").count(), 1, "{message}");
    assert!(message.contains("--config"), "{message}");
  }
}
