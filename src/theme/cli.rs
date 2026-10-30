//! `dawn list`, `dawn use`, `dawn add`, `dawn check`, `dawn new`, and
//! `dawn remove`.
//!
//! The CLI is intentionally small. It prints paths and next commands instead of
//! hiding them behind a "wizard", because the people who need this are admins
//! wiring a greeter into a display manager and users moving a bundle between
//! machines.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::cli::ThemeCommand;

use super::{Origin, ThemeEntry};

pub fn run(command: ThemeCommand) -> ExitCode {
  match command {
    ThemeCommand::List => list(),
    ThemeCommand::Check { path } => check(&path),
    ThemeCommand::New { name, id, parent } => new(&parent, &name, id.as_deref()),
    ThemeCommand::Add {
      paths,
      force,
      system,
      migrate,
    } => add(&paths, force, system, migrate),
    ThemeCommand::Remove { id, system } => remove(&id, system),
    ThemeCommand::Use { id, config } => super::select::run(&id, config.as_deref()),
  }
}

fn list() -> ExitCode {
  let (entries, warnings) = super::list();
  for warning in warnings {
    eprintln!("warning: {warning}");
  }
  if entries.is_empty() {
    println!("no themes installed.");
    println!("add one with: dawn add PATH");
    return ExitCode::SUCCESS;
  }
  print_table(&entries);
  print_active(&entries);
  ExitCode::SUCCESS
}

/// Prints which theme the next login starts. The config is optional, so a
/// missing file simply reports that nothing is selected yet.
fn print_active(entries: &[ThemeEntry]) {
  match crate::config::load(None).package {
    Some(id) if entries.iter().any(|entry| entry.id == id) => {
      println!();
      println!("active theme: {id}");
    }
    Some(id) => {
      println!();
      println!("active theme: {id} (not installed; choose one with `dawn use ID`)");
    }
    None => {
      println!();
      println!("active theme: none (choose one with `dawn use ID`)");
    }
  }
}

fn add(paths: &[PathBuf], force: bool, system: bool, migrate: bool) -> ExitCode {
  match super::add_with_migration(paths, force, system, migrate) {
    Ok(added) => {
      for theme in &added {
        let action = if theme.replaced {
          "replaced"
        } else {
          "installed"
        };
        println!(
          "{action} {} {} ({})",
          theme.entry.id,
          theme.entry.version.as_deref().unwrap_or("-"),
          theme.entry.path.display()
        );
      }
      if let Some(last) = added.last() {
        println!();
        println!("{}", next_step(&last.entry.id));
      }
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("dawn: {error}");
      ExitCode::FAILURE
    }
  }
}

/// The one-line hint printed after a successful install. It always names the
/// system config because that is the only file the greeter reads, no matter
/// which store the bundle came from.
fn next_step(id: &str) -> String {
  format!("next: select it with `dawn use {id}`")
}

/// Full DSB-2 validation of an uninstalled tree.
///
/// The report shape is normative: one line per problem, `rule <n>: <file>:
/// <what>`. A revision 1 tree still passes, with one warning that names the
/// migration command.
fn check(path: &std::path::Path) -> ExitCode {
  let report = super::check_tree(path);
  match report.revision {
    Some(revision) => println!("revision {revision}"),
    None => println!("revision unknown"),
  }
  for warning in &report.warnings {
    println!("warning: {warning}");
  }
  for issue in &report.issues {
    println!("{issue}");
  }
  if report.ok() {
    println!("result: ok");
    ExitCode::SUCCESS
  } else {
    println!("result: {} issue(s)", report.issues.len());
    ExitCode::FAILURE
  }
}

fn new(parent: &std::path::Path, name: &str, id: Option<&str>) -> ExitCode {
  let id = match id {
    Some(id) if super::valid_id(id) => id.to_string(),
    Some(id) => {
      eprintln!("dawn: invalid bundle id {id:?}: use [a-z0-9][a-z0-9_-]*");
      return ExitCode::FAILURE;
    }
    None => match super::scaffold::slug(name) {
      Some(id) => id,
      None => {
        eprintln!("dawn: {name:?} has no letters or digits to build an id from; pass --id");
        return ExitCode::FAILURE;
      }
    },
  };
  match super::new_bundle(parent, &id, name) {
    Ok(path) => {
      for step in super::new_next_steps(&path, &id) {
        println!("{step}");
      }
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("dawn: {error}");
      ExitCode::FAILURE
    }
  }
}

fn remove(id: &str, system: bool) -> ExitCode {
  match super::remove(id, system) {
    Ok(path) => {
      println!("removed {} ({})", id, path.display());
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("dawn: {error}");
      ExitCode::FAILURE
    }
  }
}

fn print_table(entries: &[ThemeEntry]) {
  let id_width = entries
    .iter()
    .map(|entry| entry.id.len())
    .max()
    .unwrap_or(2)
    .max(2);
  let version_width = entries
    .iter()
    .map(|entry| entry.version.as_deref().unwrap_or("-").len())
    .max()
    .unwrap_or(7)
    .max(7);
  println!(
    "{:<id_width$}  {:<version_width$}  {:<8}  NAME",
    "ID", "VERSION", "SOURCE"
  );
  for entry in entries {
    println!(
      "{:<id_width$}  {:<version_width$}  {:<8}  {}",
      entry.id,
      entry.version.as_deref().unwrap_or("-"),
      origin_label(entry.origin),
      entry.name
    );
  }
}

fn origin_label(origin: Origin) -> &'static str {
  match origin {
    Origin::User => "user",
    Origin::System => "system",
  }
}

#[cfg(test)]
mod tests {
  use super::next_step;

  #[test]
  fn install_hint_names_the_switch_command() {
    let hint = next_step("midnight");
    assert!(!hint.contains('\n'), "{hint}");
    assert!(hint.contains("dawn use midnight"), "{hint}");
  }
}
