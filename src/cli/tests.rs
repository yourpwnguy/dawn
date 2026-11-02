use std::path::PathBuf;

use clap::Parser;
use clap_complete::Shell;

use crate::audio::AudioCommand;

use super::{
  Cli, Command, DevCommand, InitCommand, InitTarget, RitualsCommand, Root, ThemeCommand, parse_size,
};

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
      name: "out".to_string(),
      id: Some("probe".to_string()),
      parent: ".".into()
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
  assert!(parse(&["new"]).is_err());
}

#[test]
fn rituals_status_and_reset_parse() {
  assert_eq!(
    parse(&["rituals", "status"]).ok(),
    Some(Command::Rituals(RitualsCommand::Status))
  );
  assert_eq!(
    parse(&["rituals", "reset"]).ok(),
    Some(Command::Rituals(RitualsCommand::Reset))
  );
  assert!(parse(&["rituals"]).is_err());
  assert!(parse(&["rituals", "nope"]).is_err());
}

#[test]
fn audio_devices_and_test_parse() {
  assert_eq!(
    parse(&["audio", "devices"]).ok(),
    Some(Command::Audio(AudioCommand::Devices))
  );
  assert_eq!(
    parse(&["audio", "test"]).ok(),
    Some(Command::Audio(AudioCommand::Test {
      device: "auto".to_string()
    }))
  );
  assert_eq!(
    parse(&["audio", "test", "--device", "hw:0,0"]).ok(),
    Some(Command::Audio(AudioCommand::Test {
      device: "hw:0,0".to_string()
    }))
  );
  assert!(parse(&["audio"]).is_err());
  assert!(parse(&["audio", "nope"]).is_err());
}

#[test]
fn new_takes_a_name_and_derives_a_default_id() {
  assert_eq!(
    parse(&["new", "midnight"]).ok(),
    Some(Command::Theme(ThemeCommand::New {
      name: "midnight".to_string(),
      id: None,
      parent: PathBuf::from("."),
    }))
  );
  assert_eq!(
    parse(&[
      "new",
      "Crimson Stage",
      "--id",
      "crimson",
      "-p",
      "/tmp/themes"
    ])
    .ok(),
    Some(Command::Theme(ThemeCommand::New {
      name: "Crimson Stage".to_string(),
      id: Some("crimson".to_string()),
      parent: PathBuf::from("/tmp/themes"),
    }))
  );
}

#[test]
fn use_takes_an_id_and_an_optional_config() {
  assert_eq!(
    parse(&["use", "kinich"]).ok(),
    Some(Command::Theme(ThemeCommand::Use {
      id: "kinich".to_string(),
      config: None,
    }))
  );
  assert_eq!(
    parse(&["switch", "kinich", "-c", "/tmp/dawn.toml"]).ok(),
    Some(Command::Theme(ThemeCommand::Use {
      id: "kinich".to_string(),
      config: Some(PathBuf::from("/tmp/dawn.toml")),
    }))
  );
  assert_eq!(
    parse(&["use", "-c", "/tmp/dawn.toml", "kinich"]).ok(),
    Some(Command::Theme(ThemeCommand::Use {
      id: "kinich".to_string(),
      config: Some(PathBuf::from("/tmp/dawn.toml")),
    }))
  );
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

  let use_theme = Command::Theme(ThemeCommand::Use {
    id: "midnight".to_string(),
    config: None,
  });
  for alias in ["use", "switch"] {
    assert_eq!(
      parse(&[alias, "midnight"]).ok(),
      Some(use_theme.clone()),
      "{alias}"
    );
  }
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
  assert!(parse(&["use"]).is_err());
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
