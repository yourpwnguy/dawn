//! Command line parsing with clap.
//!
//! `dawn` has one run mode and a small theme manager. Clap gives us the usual
//! `--help`, `--version`, value validation, and conflict rules for free, which
//! is less code to keep correct than a hand-written parser.

use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand as ClapSubcommand};
use clap_complete::Shell;

/// A parsed invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
  Run(Cli),
  Dev(DevCommand),
  Theme(ThemeCommand),
  Init(InitCommand),
  Completions(Shell),
}

/// Options for the windowed hot-reload theme studio.
///
/// This is deliberately separate from [`Cli`]. Dev mode is always a dry run,
/// so it has no `--dry-run` or `--fullscreen` flag to get wrong, and it cannot
/// combine with a normal run's screenshot or TTY options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevCommand {
  /// A config file or a DSB bundle directory.
  pub path: Option<PathBuf>,
  /// An installed theme id, resolved exactly like `--theme`.
  pub theme: Option<String>,
  /// Window size, default 1280x800.
  pub size: Option<(u32, u32)>,
  /// Start with the HUD hidden.
  pub no_hud: bool,
}

impl DevCommand {
  /// Dev mode is a dry run by construction. The window layer takes this as a
  /// type-level fact: `run_dev` never builds a greetd auth mode, so there is no
  /// boolean a caller could flip.
  #[must_use]
  pub const fn dry_run(&self) -> bool {
    true
  }
}

/// Where `dawn init` should write the config.
///
/// The default is the system path because a real greeter runs as the `greeter`
/// user with no useful `HOME`. `--config` is the explicit opt-out for a test or
/// a custom install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitTarget {
  /// `/etc/dawn/config.toml`.
  System,
  /// The exact path given with `--config`.
  Path(PathBuf),
}

/// A first-run config write, detached from clap like [`ThemeCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitCommand {
  pub target: InitTarget,
  pub force: bool,
}

/// A theme-store operation, detached from clap so the rest of the crate does
/// not depend on the parser type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeCommand {
  List,
  Check {
    path: PathBuf,
  },
  New {
    parent: PathBuf,
    id: String,
  },
  Add {
    paths: Vec<PathBuf>,
    force: bool,
    system: bool,
    /// Upgrade revision 1 trees before installing them.
    migrate: bool,
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
  #[arg(short = 'c', long, value_name = "PATH", conflicts_with = "theme")]
  pub config: Option<PathBuf>,

  /// Load an installed theme bundle by id
  #[arg(short = 't', long, value_name = "ID")]
  pub theme: Option<String>,

  /// Fake authentication; any non-empty password works
  #[arg(short = 'd', long)]
  pub dry_run: bool,

  /// Use the terminal fallback instead of a window
  #[arg(short = 'y', long)]
  pub tty: bool,

  /// Validate config, sessions, and plugins, then exit
  #[arg(short = 'k', long)]
  pub check_config: bool,

  /// List GPU adapters and the one this config selects
  #[arg(short = 'g', long)]
  pub list_gpus: bool,

  /// Render one frame to a PNG and exit
  #[arg(
    short = 's',
    long,
    value_name = "PATH",
    conflicts_with_all = ["tty", "check_config"]
  )]
  pub screenshot: Option<PathBuf>,

  /// Screenshot size, for example 1920x1080
  #[arg(
    short = 'z',
    long,
    value_name = "WxH",
    value_parser = parse_size,
    requires = "screenshot"
  )]
  pub size: Option<(u32, u32)>,

  /// Force a fullscreen window (overrides config)
  #[arg(short = 'f', long)]
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
    A DSB-2 bundle is a directory with theme.toml, config.toml, preview.webp,
    and an assets/ root. `dawn check PATH` validates a tree before install and
    `dawn new PARENT --id ID` scaffolds one. `dawn add` accepts one bundle or a
    directory of bundles and installs into the canonical system store
    /usr/share/dawn/themes. `--migrate` upgrades a revision 1 tree. --user
    installs into the deprecated $XDG_DATA_HOME/dawn/themes store instead.
    Lookup checks the system stores first; DAWN_THEME_DIRS adds colon-separated
    read-only system stores (useful on NixOS). A theme found only in the user
    store still loads but warns that it should be reinstalled with
    `sudo dawn add PATH --force`.

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
  #[command(alias = "ls")]
  List,

  /// Install one bundle or a directory of bundles
  #[command(alias = "install", alias = "i")]
  Add(AddArgs),

  /// Validate a DSB-2 bundle tree without installing it
  Check(CheckArgs),

  /// Create a minimal DSB-2 bundle
  New(NewArgs),

  /// Run a windowed dry-run greeter that reloads a theme when it changes
  Dev(DevArgs),

  /// Remove an installed theme bundle
  #[command(alias = "uninstall", alias = "rm")]
  Remove(RemoveArgs),

  /// Write a commented first-run config
  Init(InitArgs),

  /// Print a shell completion script to stdout
  Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
struct AddArgs {
  /// Theme bundle directories
  #[arg(value_name = "PATH", required = true)]
  paths: Vec<PathBuf>,

  /// Replace an installed theme with the same id
  #[arg(short = 'F', long)]
  force: bool,

  /// Install into the user store instead of the system store
  #[arg(short = 'u', long, conflicts_with = "system")]
  user: bool,

  /// Install into /usr/share/dawn/themes (the default)
  #[arg(long)]
  system: bool,

  /// Upgrade a revision 1 bundle to DSB-2 before installing it
  #[arg(long)]
  migrate: bool,
}

#[derive(Debug, Args)]
struct CheckArgs {
  /// Bundle directory to validate
  #[arg(value_name = "PATH")]
  path: PathBuf,
}

#[derive(Debug, Args)]
struct NewArgs {
  /// Parent directory that will contain the new bundle
  #[arg(value_name = "PARENT")]
  parent: PathBuf,

  /// Bundle id, also the new directory name
  #[arg(long, value_name = "ID", required = true)]
  id: String,
}

#[derive(Debug, Args)]
struct DevArgs {
  /// Config file or DSB bundle directory to watch
  #[arg(value_name = "PATH")]
  path: Option<PathBuf>,

  /// Load an installed theme bundle by id
  #[arg(long, value_name = "ID", conflicts_with = "path")]
  theme: Option<String>,

  /// Window size, for example 1280x800
  #[arg(long, value_name = "WxH", value_parser = parse_size)]
  size: Option<(u32, u32)>,

  /// Start with the HUD hidden
  #[arg(long)]
  no_hud: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
  /// Theme id, as shown by `dawn list`
  #[arg(value_name = "ID")]
  id: String,

  /// Remove from the user store instead of the system store
  #[arg(short = 'u', long, conflicts_with = "system")]
  user: bool,

  /// Remove from /usr/share/dawn/themes (the default)
  #[arg(long)]
  system: bool,
}

#[derive(Debug, Args)]
struct InitArgs {
  /// Write this path instead of /etc/dawn/config.toml
  #[arg(short = 'c', long, value_name = "PATH")]
  config: Option<PathBuf>,

  /// Overwrite an existing config file
  #[arg(short = 'F', long)]
  force: bool,
}

#[derive(Debug, Args)]
struct CompletionsArgs {
  /// Shell to generate completions for
  #[arg(value_name = "SHELL", required_unless_present = "shell_flag")]
  shell: Option<Shell>,

  /// Shell to generate completions for (flag form)
  #[arg(
    short = 's',
    long = "shell",
    value_name = "SHELL",
    conflicts_with = "shell"
  )]
  shell_flag: Option<Shell>,
}

/// Parses the process arguments.
pub fn parse() -> Result<Command, clap::Error> {
  let root = Root::try_parse()?;
  from_root(root)
}

/// Maps a parsed root to the crate's command type. Kept separate from
/// [`parse`] so the tests exercise the exact same mapping instead of keeping a
/// copy that can drift from the real dispatch.
fn from_root(root: Root) -> Result<Command, clap::Error> {
  Ok(match root.command {
    Some(Subcommand::List) => Command::Theme(ThemeCommand::List),
    Some(Subcommand::Check(args)) => Command::Theme(ThemeCommand::Check { path: args.path }),
    Some(Subcommand::New(args)) => Command::Theme(ThemeCommand::New {
      parent: args.parent,
      id: args.id,
    }),
    Some(Subcommand::Dev(args)) => Command::Dev(DevCommand {
      path: args.path,
      theme: args.theme,
      size: args.size,
      no_hud: args.no_hud,
    }),
    Some(Subcommand::Add(args)) => Command::Theme(ThemeCommand::Add {
      paths: args.paths,
      force: args.force,
      // The system store is the default, so `--system` is accepted but
      // redundant; only `--user` turns it off.
      system: args.system || !args.user,
      migrate: args.migrate,
    }),
    Some(Subcommand::Remove(args)) => Command::Theme(ThemeCommand::Remove {
      id: args.id,
      system: args.system || !args.user,
    }),
    Some(Subcommand::Init(args)) => Command::Init(InitCommand {
      target: match args.config {
        Some(path) => InitTarget::Path(path),
        None => InitTarget::System,
      },
      force: args.force,
    }),
    Some(Subcommand::Completions(args)) => {
      // clap already guarantees exactly one source and that they never
      // coexist, but the match keeps the failure explicit instead of
      // reaching for an unwrap.
      let Some(shell) = args.shell.or(args.shell_flag) else {
        return Err(clap::Error::new(
          clap::error::ErrorKind::MissingRequiredArgument,
        ));
      };
      Command::Completions(shell)
    }
    None => Command::Run(root.run),
  })
}

/// Writes one shell completion script to stdout.
///
/// We render into a buffer first. clap_complete's direct stdout path panics on
/// a write error, which would turn the normal `dawn completions zsh | head`
/// pipeline into a panic; buffering lets the caller treat a closed pipe as a
/// clean exit.
pub fn write_completions(shell: Shell) -> std::io::Result<()> {
  use std::io::Write;

  let mut command = Root::command();
  let mut buffer = Vec::new();
  clap_complete::generate(shell, &mut command, "dawn", &mut buffer);
  std::io::stdout().lock().write_all(&buffer)
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
  use clap_complete::Shell;

  use super::{Cli, Command, DevCommand, InitCommand, InitTarget, Root, ThemeCommand, parse_size};

  fn parse(args: &[&str]) -> Result<Command, clap::Error> {
    let mut argv = vec!["dawn"];
    argv.extend_from_slice(args);
    let root = Root::try_parse_from(argv)?;
    super::from_root(root)
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
  fn dev_parses_path_theme_size_and_no_hud() {
    assert_eq!(
      parse(&["dev", "examples/themes/kinich"]).ok(),
      Some(Command::Dev(DevCommand {
        path: Some("examples/themes/kinich".into()),
        theme: None,
        size: None,
        no_hud: false,
      }))
    );
    assert_eq!(
      parse(&["dev", "--theme", "kinich", "--size", "1600x900", "--no-hud"]).ok(),
      Some(Command::Dev(DevCommand {
        path: None,
        theme: Some("kinich".to_string()),
        size: Some((1600, 900)),
        no_hud: true,
      }))
    );
    assert!(parse(&["dev", "one", "--theme", "two"]).is_err());
    assert!(parse(&["dev", "--size", "10x10"]).is_err());
  }

  #[test]
  fn dev_is_non_auth_by_construction() {
    let command = match parse(&["dev"]) {
      Ok(Command::Dev(command)) => command,
      other => panic!("expected a dev command, got {other:?}"),
    };
    assert!(command.dry_run());
    // A normal run is only non-auth when --dry-run is passed.
    assert!(!run(&[]).dry_run);
    assert!(run(&["--dry-run"]).dry_run);
  }

  #[test]
  fn parses_theme_commands_with_system_as_the_default() {
    assert_eq!(
      parse(&["list"]).ok(),
      Some(Command::Theme(ThemeCommand::List))
    );
    assert_eq!(
      parse(&["add", "one", "two"]).ok(),
      Some(Command::Theme(ThemeCommand::Add {
        paths: vec!["one".into(), "two".into()],
        force: false,
        system: true,
        migrate: false
      }))
    );
    assert_eq!(
      parse(&["add", "one", "--user"]).ok(),
      Some(Command::Theme(ThemeCommand::Add {
        paths: vec!["one".into()],
        force: false,
        system: false,
        migrate: false
      }))
    );
    assert_eq!(
      parse(&["add", "one", "--system"]).ok(),
      Some(Command::Theme(ThemeCommand::Add {
        paths: vec!["one".into()],
        force: false,
        system: true,
        migrate: false
      }))
    );
    assert_eq!(
      parse(&["add", "one", "--migrate"]).ok(),
      Some(Command::Theme(ThemeCommand::Add {
        paths: vec!["one".into()],
        force: false,
        system: true,
        migrate: true
      }))
    );
    assert_eq!(
      parse(&["check", "one"]).ok(),
      Some(Command::Theme(ThemeCommand::Check { path: "one".into() }))
    );
    assert_eq!(
      parse(&["new", "out", "--id", "probe"]).ok(),
      Some(Command::Theme(ThemeCommand::New {
        parent: "out".into(),
        id: "probe".to_string()
      }))
    );
    assert_eq!(
      parse(&["remove", "midnight"]).ok(),
      Some(Command::Theme(ThemeCommand::Remove {
        id: "midnight".to_string(),
        system: true
      }))
    );
    assert_eq!(
      parse(&["remove", "midnight", "--user"]).ok(),
      Some(Command::Theme(ThemeCommand::Remove {
        id: "midnight".to_string(),
        system: false
      }))
    );
  }

  #[test]
  fn init_targets_and_short_flags_parse() {
    assert_eq!(
      parse(&["init"]).ok(),
      Some(Command::Init(InitCommand {
        target: InitTarget::System,
        force: false
      }))
    );
    assert_eq!(
      parse(&["init", "-c", "/tmp/dawn.toml"]).ok(),
      Some(Command::Init(InitCommand {
        target: InitTarget::Path("/tmp/dawn.toml".into()),
        force: false
      }))
    );
    assert_eq!(
      parse(&["init", "--config", "/tmp/dawn.toml", "-F"]).ok(),
      Some(Command::Init(InitCommand {
        target: InitTarget::Path("/tmp/dawn.toml".into()),
        force: true
      }))
    );
    // --user is gone. The system path is the only default, and --config is the
    // explicit escape hatch for a test or a custom install.
    assert!(parse(&["init", "--user"]).is_err());
    assert!(parse(&["init", "-u"]).is_err());
    assert!(parse(&["init", "extra"]).is_err());
    assert!(parse(&["new", "out"]).is_err());
  }

  #[test]
  fn subcommand_aliases_are_equivalent() {
    let add = Command::Theme(ThemeCommand::Add {
      paths: vec!["one".into()],
      force: false,
      system: true,
      migrate: false,
    });
    for alias in ["add", "install", "i"] {
      assert_eq!(parse(&[alias, "one"]).ok(), Some(add.clone()), "{alias}");
    }

    let remove = Command::Theme(ThemeCommand::Remove {
      id: "midnight".to_string(),
      system: true,
    });
    for alias in ["remove", "uninstall", "rm"] {
      assert_eq!(
        parse(&[alias, "midnight"]).ok(),
        Some(remove.clone()),
        "{alias}"
      );
    }

    assert_eq!(
      parse(&["ls"]).ok(),
      Some(Command::Theme(ThemeCommand::List))
    );
  }

  #[test]
  fn short_theme_flags_round_trip_exactly() {
    assert_eq!(
      parse(&["add", "-F", "-u", "one"]).ok(),
      Some(Command::Theme(ThemeCommand::Add {
        paths: vec!["one".into()],
        force: true,
        system: false,
        migrate: false
      }))
    );
    assert_eq!(
      parse(&["remove", "-u", "midnight"]).ok(),
      Some(Command::Theme(ThemeCommand::Remove {
        id: "midnight".to_string(),
        system: false
      }))
    );
  }

  #[test]
  fn short_run_flags_round_trip_exactly() {
    assert_eq!(
      parse(&["-c", "/etc/dawn/config.toml", "-d", "-f", "-v"]).ok(),
      Some(Command::Run(Cli {
        config: Some("/etc/dawn/config.toml".into()),
        dry_run: true,
        fullscreen: true,
        verbose: true,
        ..Cli::default()
      }))
    );
    assert_eq!(
      parse(&["-t", "midnight", "-y"]).ok(),
      Some(Command::Run(Cli {
        theme: Some("midnight".to_string()),
        tty: true,
        ..Cli::default()
      }))
    );
    assert_eq!(
      parse(&["-k"]).ok(),
      Some(Command::Run(Cli {
        check_config: true,
        ..Cli::default()
      }))
    );
    assert_eq!(
      parse(&["-g"]).ok(),
      Some(Command::Run(Cli {
        list_gpus: true,
        ..Cli::default()
      }))
    );
    assert_eq!(
      parse(&["-s", "/tmp/dawn.png", "-z", "800x600"]).ok(),
      Some(Command::Run(Cli {
        screenshot: Some("/tmp/dawn.png".into()),
        size: Some((800, 600)),
        ..Cli::default()
      }))
    );
  }

  #[test]
  fn completions_parse_for_each_shell() {
    for (name, shell) in [
      ("bash", Shell::Bash),
      ("elvish", Shell::Elvish),
      ("fish", Shell::Fish),
      ("powershell", Shell::PowerShell),
      ("zsh", Shell::Zsh),
    ] {
      assert_eq!(
        parse(&["completions", name]).ok(),
        Some(Command::Completions(shell)),
        "{name}"
      );
    }
    assert_eq!(
      parse(&["completions", "-s", "bash"]).ok(),
      Some(Command::Completions(Shell::Bash))
    );
    assert_eq!(
      parse(&["completions", "--shell", "zsh"]).ok(),
      Some(Command::Completions(Shell::Zsh))
    );
    assert!(parse(&["completions"]).is_err());
    assert!(parse(&["completions", "nushell"]).is_err());
  }

  #[test]
  fn user_and_system_flags_conflict() {
    assert!(parse(&["add", "one", "--user", "--system"]).is_err());
    assert!(parse(&["remove", "midnight", "--user", "--system"]).is_err());
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
