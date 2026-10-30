//! Command line parsing with clap.
//!
//! `dawn` has one run mode and a small theme manager. Clap gives us the usual
//! `--help`, `--version`, value validation, and conflict rules for free, which
//! is less code to keep correct than a hand-written parser.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand as ClapSubcommand};

/// A parsed invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
  Run(Cli),
  Theme(ThemeCommand),
}

/// A theme-store operation, detached from clap so the rest of the crate does
/// not depend on the parser type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeCommand {
  List,
  Add {
    paths: Vec<PathBuf>,
    force: bool,
    system: bool,
  },
  Remove {
    id: String,
    system: bool,
  },
}

/// Options for a normal greeter run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Args)]
#[command(next_help_heading = "Run options")]
pub struct Cli {
  /// Read this config file instead of the discovered one
  #[arg(long, value_name = "PATH", conflicts_with = "theme")]
  pub config: Option<PathBuf>,

  /// Load an installed theme bundle by id
  #[arg(long, value_name = "ID")]
  pub theme: Option<String>,

  /// Fake authentication; any non-empty password works
  #[arg(long)]
  pub dry_run: bool,

  /// Use the terminal fallback instead of a window
  #[arg(long)]
  pub tty: bool,

  /// Validate config, sessions, and plugins, then exit
  #[arg(long)]
  pub check_config: bool,

  /// List GPU adapters and the one this config selects
  #[arg(long)]
  pub list_gpus: bool,

  /// Render one frame to a PNG and exit
  #[arg(long, value_name = "PATH", conflicts_with_all = ["tty", "check_config"])]
  pub screenshot: Option<PathBuf>,

  /// Screenshot size, for example 1920x1080
  #[arg(long, value_name = "WxH", value_parser = parse_size, requires = "screenshot")]
  pub size: Option<(u32, u32)>,

  /// Force a fullscreen window (overrides config)
  #[arg(long)]
  pub fullscreen: bool,

  /// Debug logging
  #[arg(short, long)]
  pub verbose: bool,
}

#[derive(Debug, Parser)]
#[command(
  name = "dawn",
  version,
  about = "A GPU-composited greetd login greeter",
  long_about = "A GPU-composited greetd login greeter with animated wallpapers, \
                cardless or card layouts, widgets, and a theme store.",
  args_conflicts_with_subcommands = true,
  after_help = "THEMES:
    A theme bundle is a directory with theme.toml, config.toml, and its assets.
    `dawn add` accepts one bundle or a directory of bundles. The user store is
    $XDG_DATA_HOME/dawn/themes; --system uses /usr/share/dawn/themes.
    DAWN_THEME_DIRS adds colon-separated read-only stores (useful on NixOS).

KEYBOARD:
    Tab / Shift+Tab      Move between username, password, and sessions
    Enter                Continue, sign in, or choose a session
    Up / Down            Choose a session while the picker is open
    Escape               Cancel a login in flight, otherwise clear the field"
)]
struct Root {
  #[command(subcommand)]
  command: Option<Subcommand>,

  #[command(flatten)]
  run: Cli,
}

#[derive(Debug, ClapSubcommand)]
enum Subcommand {
  /// List installed theme bundles
  List,

  /// Install one bundle or a directory of bundles
  Add(AddArgs),

  /// Remove an installed theme bundle
  Remove(RemoveArgs),
}

#[derive(Debug, Args)]
struct AddArgs {
  /// Theme bundle directories
  #[arg(value_name = "PATH", required = true)]
  paths: Vec<PathBuf>,

  /// Replace an installed theme with the same id
  #[arg(long)]
  force: bool,

  /// Install into /usr/share/dawn/themes
  #[arg(long)]
  system: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
  /// Theme id, as shown by `dawn list`
  #[arg(value_name = "ID")]
  id: String,

  /// Remove from /usr/share/dawn/themes
  #[arg(long)]
  system: bool,
}

/// Parses the process arguments.
pub fn parse() -> Result<Command, clap::Error> {
  let root = Root::try_parse()?;
  Ok(match root.command {
    Some(Subcommand::List) => Command::Theme(ThemeCommand::List),
    Some(Subcommand::Add(args)) => Command::Theme(ThemeCommand::Add {
      paths: args.paths,
      force: args.force,
      system: args.system,
    }),
    Some(Subcommand::Remove(args)) => Command::Theme(ThemeCommand::Remove {
      id: args.id,
      system: args.system,
    }),
    None => Command::Run(root.run),
  })
}

fn parse_size(value: &str) -> Result<(u32, u32), String> {
  let (width, height) = value
    .split_once(['x', 'X'])
    .ok_or_else(|| "expected WxH, for example 1366x768".to_string())?;
  let parse = |part: &str| -> Result<u32, String> {
    part
      .trim()
      .parse::<u32>()
      .map_err(|_| "expected two numbers".to_string())
  };
  let width = parse(width)?;
  let height = parse(height)?;
  if !(320..=8192).contains(&width) || !(240..=8192).contains(&height) {
    return Err("width must be 320..=8192 and height 240..=8192".to_string());
  }
  Ok((width, height))
}

#[cfg(test)]
mod tests {
  use clap::Parser;

  use super::{Cli, Command, Root, ThemeCommand, parse_size};

  fn parse(args: &[&str]) -> Result<Command, clap::Error> {
    let mut argv = vec!["dawn"];
    argv.extend_from_slice(args);
    let root = Root::try_parse_from(argv)?;
    Ok(match root.command {
      Some(super::Subcommand::List) => Command::Theme(ThemeCommand::List),
      Some(super::Subcommand::Add(args)) => Command::Theme(ThemeCommand::Add {
        paths: args.paths,
        force: args.force,
        system: args.system,
      }),
      Some(super::Subcommand::Remove(args)) => Command::Theme(ThemeCommand::Remove {
        id: args.id,
        system: args.system,
      }),
      None => Command::Run(root.run),
    })
  }

  fn run(args: &[&str]) -> Cli {
    match parse(args) {
      Ok(Command::Run(cli)) => cli,
      other => panic!("expected a run command, got {other:?}"),
    }
  }

  #[test]
  fn defaults_are_a_normal_graphical_run() {
    let cli = run(&[]);
    assert!(!cli.dry_run);
    assert!(!cli.tty);
    assert_eq!(cli.screenshot, None);
    assert_eq!(cli.theme, None);
  }

  #[test]
  fn parses_values_in_both_spellings() {
    let cli = run(&[
      "--config",
      "/etc/dawn/config.toml",
      "--screenshot=/tmp/dawn.png",
      "--size",
      "800x600",
    ]);
    assert_eq!(cli.config.unwrap().to_str(), Some("/etc/dawn/config.toml"));
    assert_eq!(cli.screenshot.unwrap().to_str(), Some("/tmp/dawn.png"));
    assert_eq!(cli.size, Some((800, 600)));
  }

  #[test]
  fn parses_theme_selection() {
    assert_eq!(
      run(&["--theme", "midnight"]).theme.as_deref(),
      Some("midnight")
    );
    assert_eq!(
      run(&["--theme=midnight"]).theme.as_deref(),
      Some("midnight")
    );
    assert!(parse(&["--theme", "midnight", "--config", "x"]).is_err());
  }

  #[test]
  fn parses_theme_commands() {
    assert_eq!(
      parse(&["list"]).ok(),
      Some(Command::Theme(ThemeCommand::List))
    );
    assert_eq!(
      parse(&["add", "one", "two"]).ok(),
      Some(Command::Theme(ThemeCommand::Add {
        paths: vec!["one".into(), "two".into()],
        force: false,
        system: false
      }))
    );
    assert_eq!(
      parse(&["remove", "midnight", "--system"]).ok(),
      Some(Command::Theme(ThemeCommand::Remove {
        id: "midnight".to_string(),
        system: true
      }))
    );
  }

  #[test]
  fn theme_commands_reject_missing_values() {
    assert!(parse(&["add"]).is_err());
    assert!(parse(&["remove"]).is_err());
    assert!(parse(&["list", "extra"]).is_err());
  }

  #[test]
  fn run_options_conflict_with_subcommands() {
    assert!(parse(&["--theme", "midnight", "list"]).is_err());
  }

  #[test]
  fn help_and_version_are_clap_errors() {
    assert!(parse(&["--help"]).is_err());
    assert!(parse(&["-V"]).is_err());
    assert!(parse(&["add", "--help"]).is_err());
  }

  #[test]
  fn sizes_are_bounded() {
    assert!(parse_size("10x10").is_err());
    assert!(parse_size("800by600").is_err());
    assert_eq!(parse_size("800x600"), Ok((800, 600)));
  }

  #[test]
  fn size_requires_screenshot() {
    assert!(parse(&["--size", "800x600"]).is_err());
  }

  #[test]
  fn parses_list_gpus() {
    assert!(run(&["--list-gpus"]).list_gpus);
    assert!(!run(&[]).list_gpus);
  }
}
