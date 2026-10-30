//! Process entry point and the only place that calls `std::process::exit`.
//!
//! Startup order matters for recovery: config and sessions are loaded first
//! (they always succeed), then the frontend is chosen. If the graphical
//! frontend cannot start, the terminal fallback is tried when a terminal is
//! available. A broken config or plugin never prevents a login; it only
//! produces warnings.

use std::process::ExitCode;

use anyhow::{Context, Result};
use dawn::app::Finish;
use dawn::cli::{self, Cli, Command};
use dawn::config::{self, LoadedConfig};
use dawn::plugin::{self, Report};
use dawn::session::{self, Session};
use dawn::ui::screenshot;
use dawn::ui::window;
use tracing::{Level, info, warn};

fn main() -> ExitCode {
  let command = match cli::parse() {
    Ok(command) => command,
    Err(error) => {
      let code = error.exit_code();
      let _ = error.print();
      return ExitCode::from(u8::try_from(code).unwrap_or(2));
    }
  };

  // Commands that write /etc or /usr/share ask for sudo themselves when they
  // have a terminal. A child that ran under sudo returns its status here.
  if let Some(code) = dawn::elevate::maybe_reexec(&command) {
    return code;
  }

  let cli = match command {
    Command::Theme(command) => return dawn::theme::cli::run(command),
    Command::Init(command) => return dawn::init::run(command),
    Command::Dev(command) => {
      init_logging(false);
      return match dawn::dev::run(command) {
        Ok(code) => code,
        Err(error) => {
          eprintln!("dawn: {error:#}");
          ExitCode::FAILURE
        }
      };
    }
    Command::Completions(shell) => {
      return match cli::write_completions(shell) {
        Ok(()) => ExitCode::SUCCESS,
        // `dawn completions zsh | head` closes the pipe early; that is a
        // normal way to read the first lines, not a failure.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
          eprintln!("dawn: cannot write completions: {error}");
          ExitCode::FAILURE
        }
      };
    }
    Command::Run(cli) => cli,
  };

  init_logging(cli.verbose);
  match run(cli) {
    Ok(code) => code,
    Err(error) => {
      eprintln!("dawn: {error:#}");
      ExitCode::FAILURE
    }
  }
}

fn run(cli: Cli) -> Result<ExitCode> {
  let mut loaded = config::load(cli.config.as_deref());
  // A theme bundle replaces the whole config, but the GPU belongs to the
  // machine, not to the theme. Save the host table before selection and put it
  // back afterwards so `--theme` can never change which adapter boots.
  let host_gpu = loaded.config.gpu.clone();
  if let Some(id) = cli.theme.as_deref() {
    loaded = load_theme(id).unwrap_or_else(|| {
      let mut fallback = config::load(None);
      fallback.issues.insert(0, missing_theme_issue(id));
      fallback
    });
  } else if let Some(id) = loaded.package.take() {
    loaded = load_theme(&id).unwrap_or_else(|| {
      loaded.issues.insert(0, missing_theme_issue(&id));
      loaded
    });
  }
  loaded.config.gpu = host_gpu;
  let mut config = loaded.config.clone();
  config.gpu.apply_env();
  // Keep the diagnostics view in sync with the env override. The fields below
  // read `loaded.config`, and an admin running `--check-config` should see the
  // adapter policy that will actually be used.
  loaded.config.gpu = config.gpu.clone();
  if cli.fullscreen {
    config.ui.fullscreen = true;
  }

  for issue in &loaded.issues {
    warn!("{issue}");
  }
  match &loaded.path {
    Some(path) => info!(path = %path.display(), "loaded config"),
    None => info!("no config file found, using built-in defaults"),
  }
  // Exactly one plain line, not a tracing record: this is the instruction an
  // admin sees on a fresh install, and it has to survive a log filter.
  if let Some(hint) = config::missing_config_hint(loaded.path.as_deref()) {
    eprintln!("{hint}");
  }

  if cli.list_gpus {
    return match dawn::ui::gpu::list_adapters(&config.gpu) {
      Ok(text) => {
        println!("{text}");
        Ok(ExitCode::SUCCESS)
      }
      Err(error) => {
        eprintln!("dawn: {error}");
        Ok(ExitCode::FAILURE)
      }
    };
  }

  let sessions = session::discover(&config.sessions.dirs);
  let report = plugin::load_all(&config.plugins, &config.theme);
  for error in &report.errors {
    warn!("plugin: {error}");
  }

  if cli.dry_run {
    warn!("dry run: authentication is simulated and no session will start");
  }
  let warning = loaded.summary();

  if cli.check_config {
    return Ok(check_config(&loaded, &sessions, &report));
  }

  if let Some(path) = cli.screenshot.clone() {
    let size = cli.size.unwrap_or((1366, 768));
    screenshot::render(
      &config,
      &sessions,
      &report.plugins,
      size,
      cli.dry_run,
      warning,
      &path,
    )
    .with_context(|| format!("cannot write screenshot {}", path.display()))?;
    println!("wrote {} ({}x{})", path.display(), size.0, size.1);
    return Ok(ExitCode::SUCCESS);
  }

  let finish = if cli.tty {
    dawn::tty::run(&config, &sessions, cli.dry_run).context("terminal login failed")?
  } else {
    let window_config = config.clone();
    match window::run(
      window_config,
      sessions.clone(),
      report.plugins,
      warning,
      cli.dry_run,
      config.ui.fullscreen,
    ) {
      Ok(finish) => finish,
      Err(error) => {
        warn!(%error, "graphical frontend failed");
        if dawn::tty::available() {
          eprintln!("dawn: graphical frontend failed ({error}); using the terminal fallback");
          dawn::tty::run(&config, &sessions, cli.dry_run).context("terminal fallback failed")?
        } else {
          return Err(error).context("graphical frontend failed and no terminal is available");
        }
      }
    }
  };

  match finish {
    Finish::Started => info!("session start accepted, exiting"),
    Finish::Quit => info!("greeter closed, exiting"),
  }
  Ok(ExitCode::SUCCESS)
}

fn load_theme(id: &str) -> Option<LoadedConfig> {
  let dir = dawn::theme::find(id)?;
  info!(theme = id, path = %dir.display(), "loading theme bundle");
  Some(config::load_theme(&dir.join("config.toml")))
}

fn missing_theme_issue(id: &str) -> config::Issue {
  config::Issue {
    key: "theme.package".to_string(),
    location: None,
    message: format!("theme {id:?} is not installed; run `dawn list` or `dawn add PATH`"),
  }
}

/// Prints an admin-facing report. The exit code is nonzero when something is
/// actually wrong, so a package post-install check can use it.
fn check_config(loaded: &LoadedConfig, sessions: &[Session], report: &Report) -> ExitCode {
  println!("dawn {} config check", env!("CARGO_PKG_VERSION"));
  match &loaded.path {
    Some(path) => println!("config: {}", path.display()),
    None => println!("config: built-in defaults (no file found)"),
  }
  println!("gpu: {}", loaded.config.gpu.summary());

  if loaded.issues.is_empty() {
    println!("issues: none");
  } else {
    println!("issues: {}", loaded.issues.len());
    for issue in &loaded.issues {
      println!("  {issue}");
    }
  }

  println!("sessions: {}", sessions.len());
  for session in sessions {
    println!("  {} ({})", session.name, session.id);
  }

  // Built-in widgets are real plugins, but they are not in `report.plugins`
  // until `App::new` appends them. Showing them here keeps the admin report
  // honest about what will actually be painted.
  let builtins = plugin::builtins(&loaded.config);
  let builtin_kinds: Vec<&str> = builtins
    .plugins
    .iter()
    .flat_map(|plugin| &plugin.widgets)
    .map(|widget| widget.kind())
    .collect();
  println!(
    "built-in widgets: {}",
    if builtin_kinds.is_empty() {
      "none".to_string()
    } else {
      builtin_kinds.join(", ")
    }
  );

  println!("plugins: {}", report.plugins.len());
  for plugin in &report.plugins {
    println!(
      "  {} {} ({}, {} widgets)",
      plugin.id,
      plugin.version,
      plugin.name,
      plugin.widgets.len()
    );
  }
  if !report.errors.is_empty() {
    println!("plugin errors: {}", report.errors.len());
    for error in &report.errors {
      println!("  {error}");
    }
  }

  let clean = loaded.issues.is_empty() && report.errors.is_empty() && !sessions.is_empty();
  if !sessions.is_empty() {
    println!("result: {}", if clean { "ok" } else { "problems found" });
  } else {
    println!("result: no sessions, a login would have nothing to start");
  }
  if clean {
    ExitCode::SUCCESS
  } else {
    ExitCode::FAILURE
  }
}

fn init_logging(verbose: bool) {
  let level = if verbose { Level::DEBUG } else { Level::INFO };
  let _ = tracing_subscriber::fmt()
    .with_max_level(level)
    .with_target(false)
    .try_init();
}
