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
  Rituals(RitualsCommand),
  Audio(crate::audio::AudioCommand),
  Init(InitCommand),
  Completions(Shell),
}

/// Ritual state maintenance. These commands never touch the login path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RitualsCommand {
  Status,
  Reset,
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
    name: String,
    id: Option<String>,
    parent: PathBuf,
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
  /// Select the theme the next login uses.
  Use {
    id: String,
    config: Option<PathBuf>,
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
    and an assets/ root. `dawn new NAME` scaffolds one, `dawn check PATH`
    validates a tree, and `dawn add PATH` installs it into the system store
    /usr/share/dawn/themes. `dawn use ID` switches the theme the next login
    starts, so nobody has to edit /etc/dawn/config.toml by hand. `dawn dev PATH`
    opens a bundle with hot reload.

    Commands that write /etc or /usr/share ask for sudo themselves when they
    have a terminal. Set DAWN_NO_SUDO=1 to disable that and get the plain
    command instead.

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
  /// Switch the theme the next login uses
  #[command(alias = "switch")]
  Use(UseArgs),

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

  /// Show or reset the local ritual state
  Rituals(RitualsArgs),

  /// List or test ALSA output devices
  Audio(AudioArgs),

  /// Print a shell completion script to stdout
  Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
struct RitualsArgs {
  #[command(subcommand)]
  action: RitualsAction,
}

#[derive(Debug, ClapSubcommand)]
enum RitualsAction {
  /// Print the streak, best streak, and unlocked achievements
  Status,
  /// Remove the local ritual state file
  Reset,
}

#[derive(Debug, Args)]
struct AudioArgs {
  #[command(subcommand)]
  action: AudioAction,
}

#[derive(Debug, ClapSubcommand)]
enum AudioAction {
  /// List ALSA playback devices the greeter can try
  Devices,
  /// Open one device and play a short test tone
  Test {
    /// ALSA PCM name, for example hw:0,0 or default
    #[arg(long, default_value = "auto")]
    device: String,
  },
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
  /// Display name for the theme, for example "Crimson Stage"
  #[arg(value_name = "NAME")]
  name: String,

  /// Bundle id and folder name (default: the name in lowercase with dashes)
  #[arg(long, value_name = "ID")]
  id: Option<String>,

  /// Folder that will contain the new bundle
  #[arg(short = 'p', long, value_name = "DIR", default_value = ".")]
  parent: PathBuf,
}

#[derive(Debug, Args)]
struct UseArgs {
  /// Theme id, as shown by `dawn list`
  #[arg(value_name = "ID")]
  id: String,

  /// Edit this config instead of the discovered one
  #[arg(short = 'c', long, value_name = "PATH")]
  config: Option<PathBuf>,
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
      name: args.name,
      id: args.id,
      parent: args.parent,
    }),
    Some(Subcommand::Use(args)) => Command::Theme(ThemeCommand::Use {
      id: args.id,
      config: args.config,
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
    Some(Subcommand::Rituals(args)) => Command::Rituals(match args.action {
      RitualsAction::Status => RitualsCommand::Status,
      RitualsAction::Reset => RitualsCommand::Reset,
    }),
    Some(Subcommand::Audio(args)) => Command::Audio(match args.action {
      AudioAction::Devices => crate::audio::AudioCommand::Devices,
      AudioAction::Test { device } => crate::audio::AudioCommand::Test { device },
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
mod tests;
