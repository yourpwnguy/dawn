//! `dawn list`, `dawn add`, and `dawn remove`.
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
    ThemeCommand::Add {
      paths,
      force,
      system,
    } => add(&paths, force, system),
    ThemeCommand::Remove { id, system } => remove(&id, system),
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
  ExitCode::SUCCESS
}

fn add(paths: &[PathBuf], force: bool, system: bool) -> ExitCode {
  match super::add(paths, force, system) {
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
        println!("use it for one run:");
        println!("  dawn --theme {}", last.entry.id);
        println!();
        println!("or make it persistent in /etc/dawn/config.toml:");
        println!("  [theme]");
        println!("  package = \"{}\"", last.entry.id);
        if added.len() > 1 {
          println!();
          println!("more than one theme was added; replace the id with any of:");
          for theme in &added {
            println!("  {}", theme.entry.id);
          }
        }
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
