//! Asking for root when a command has to write a system path.
//!
//! `dawn init`, `dawn add`, `dawn remove`, and `dawn use` normally write
//! `/etc/dawn` or `/usr/share/dawn/themes`. Rather than make every user type
//! `sudo` (and risk running the greeter itself as root), the CLI checks whether
//! the target directory is writable. If it is not, and the process has a
//! terminal, it re-runs the same command through `sudo` and exits with that
//! status.
//!
//! Two hard rules keep this from becoming a surprise:
//!
//! - escalation only happens on a terminal, so scripts and CI get the old
//!   "rerun with sudo" message instead of a hidden password prompt;
//! - `DAWN_NO_SUDO=1` disables it entirely.
//!
//! We never pass the command through a shell. The argv is forwarded exactly as
//! received, with `--` before the program so nothing is read as a sudo option.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::{Command, InitTarget, ThemeCommand};
use crate::config::SYSTEM_CONFIG;
use crate::theme::select;

/// Re-runs the command under `sudo` when its write target is not writable.
///
/// Returns `Some(code)` when an escalated child ran (or could not be started),
/// meaning the caller should exit with that code and do nothing else.
pub fn maybe_reexec(command: &Command) -> Option<ExitCode> {
  let target = writable_target(command)?;
  if dir_writable(&target) {
    return None;
  }
  if std::env::var_os("DAWN_NO_SUDO").is_some() {
    return None;
  }
  if !std::io::stdin().is_terminal() {
    return None;
  }

  let program = std::env::current_exe().ok()?;
  let args: Vec<OsString> = std::env::args_os().skip(1).collect();
  let argv = escalation_argv(&program, args);

  eprintln!(
    "dawn: writing to {} needs root; asking for sudo",
    target.display()
  );
  match std::process::Command::new(&argv[0])
    .args(&argv[1..])
    .status()
  {
    Ok(status) => Some(ExitCode::from(
      status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1),
    )),
    Err(error) => {
      eprintln!("dawn: cannot run sudo: {error}");
      eprintln!("run the same command as root, or set DAWN_NO_SUDO=1 to print the hint");
      Some(ExitCode::FAILURE)
    }
  }
}

/// The path whose writability decides whether this command needs root.
///
/// `--user` store operations are explicitly out of scope: they write under
/// `$XDG_DATA_HOME`, which the caller already owns.
fn writable_target(command: &Command) -> Option<PathBuf> {
  match command {
    Command::Init(init) => Some(match &init.target {
      InitTarget::System => PathBuf::from(SYSTEM_CONFIG),
      InitTarget::Path(path) => path.clone(),
    }),
    Command::Theme(ThemeCommand::Use { config, .. }) => {
      Some(select::target_path(config.as_deref()))
    }
    Command::Theme(ThemeCommand::Add { system: true, .. })
    | Command::Theme(ThemeCommand::Remove { system: true, .. }) => {
      Some(crate::theme::system_root())
    }
    Command::Theme(_) | Command::Dev(_) | Command::Completions(_) | Command::Run(_) => None,
  }
}

/// The exact argv used for the child: `sudo -- <current exe> <original args>`.
fn escalation_argv(program: &Path, args: Vec<OsString>) -> Vec<OsString> {
  let mut argv = Vec::with_capacity(args.len() + 3);
  argv.push(OsString::from("sudo"));
  argv.push(OsString::from("--"));
  argv.push(program.as_os_str().to_os_string());
  argv.extend(args);
  argv
}

/// True when a file can be created in `path` (or its nearest existing parent).
///
/// This is a probe rather than a uid check so it also catches read-only
/// mounts, immutable directories, and ACLs. The probe file is removed when it
/// drops.
fn dir_writable(path: &Path) -> bool {
  let mut probe = path;
  while !probe.exists() {
    match probe.parent() {
      Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
      _ => return false,
    }
  }
  if !probe.is_dir() {
    probe = probe.parent().unwrap_or_else(|| Path::new("/"));
  }
  match tempfile::Builder::new()
    .prefix(".dawn-write-probe-")
    .tempfile_in(probe)
  {
    Ok(file) => {
      drop(file);
      true
    }
    Err(_) => false,
  }
}

#[cfg(test)]
mod tests {
  use std::ffi::OsString;
  use std::path::{Path, PathBuf};

  use crate::cli::{Cli, Command, InitCommand, InitTarget, ThemeCommand};

  use super::{dir_writable, escalation_argv, writable_target};

  fn theme_command(command: ThemeCommand) -> Command {
    Command::Theme(command)
  }

  #[test]
  fn init_targets_the_system_config_unless_a_path_was_given() {
    assert_eq!(
      writable_target(&Command::Init(InitCommand {
        target: InitTarget::System,
        force: false
      })),
      Some(PathBuf::from("/etc/dawn/config.toml"))
    );
    assert_eq!(
      writable_target(&Command::Init(InitCommand {
        target: InitTarget::Path("/tmp/dawn.toml".into()),
        force: false
      })),
      Some(PathBuf::from("/tmp/dawn.toml"))
    );
  }

  #[test]
  fn theme_commands_target_the_store_they_write() {
    assert_eq!(
      writable_target(&theme_command(ThemeCommand::Add {
        paths: vec!["one".into()],
        force: false,
        system: true,
        migrate: false,
      })),
      Some(crate::theme::system_root())
    );
    assert_eq!(
      writable_target(&theme_command(ThemeCommand::Remove {
        id: "one".into(),
        system: true,
      })),
      Some(crate::theme::system_root())
    );
    assert_eq!(
      writable_target(&theme_command(ThemeCommand::Add {
        paths: vec!["one".into()],
        force: false,
        system: false,
        migrate: false,
      })),
      None
    );
    assert_eq!(
      writable_target(&theme_command(ThemeCommand::Remove {
        id: "one".into(),
        system: false,
      })),
      None
    );
  }

  #[test]
  fn use_targets_the_config_path() {
    assert_eq!(
      writable_target(&theme_command(ThemeCommand::Use {
        id: "one".into(),
        config: Some("/tmp/dawn.toml".into()),
      })),
      Some(PathBuf::from("/tmp/dawn.toml"))
    );
  }

  #[test]
  fn run_and_read_only_commands_never_escalate() {
    assert_eq!(writable_target(&Command::Run(Cli::default())), None);
    assert_eq!(writable_target(&theme_command(ThemeCommand::List)), None);
  }

  #[test]
  fn the_sudo_argv_is_exact_and_shell_free() {
    let args = vec![
      OsString::from("add"),
      OsString::from("--force"),
      OsString::from("a b"),
    ];
    let argv = escalation_argv(Path::new("/usr/local/bin/dawn"), args);
    assert_eq!(
      argv,
      vec![
        OsString::from("sudo"),
        OsString::from("--"),
        OsString::from("/usr/local/bin/dawn"),
        OsString::from("add"),
        OsString::from("--force"),
        OsString::from("a b"),
      ]
    );
  }

  #[test]
  fn a_writable_temp_dir_needs_no_escalation() {
    let dir = std::env::temp_dir();
    assert!(dir_writable(&dir));
    assert!(dir_writable(&dir.join("dawn-not-created-yet.toml")));
  }
}
