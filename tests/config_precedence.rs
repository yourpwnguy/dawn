//! End-to-end config discovery checks.
//!
//! These spawn the real binary because the bug they guard against is
//! environment-driven: `XDG_CONFIG_HOME` used to be a discovery source. Unit
//! tests cannot set process-wide environment variables safely while other tests
//! run in parallel, so a child process is the honest way to prove it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
  let path = std::env::temp_dir().join(format!(
    "dawn-cli-precedence-{name}-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|duration| duration.as_nanos())
      .unwrap_or_default()
  ));
  std::fs::create_dir_all(&path).expect("create temp dir");
  path
}

fn write_config(path: &Path) {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).expect("create config parent");
  }
  std::fs::write(
    path,
    "[theme]\npreset = \"glass-dark\"\n\n[layout]\nsave = false\n",
  )
  .expect("write config");
}

fn check_config(dir: &Path, env_config: Option<&Path>, xdg: Option<&Path>) -> String {
  let mut command = Command::new(env!("CARGO_BIN_EXE_dawn"));
  command.arg("--check-config").env_remove("DAWN_CONFIG");
  if let Some(env_config) = env_config {
    command.env("DAWN_CONFIG", env_config);
  }
  if let Some(xdg) = xdg {
    command.env("XDG_CONFIG_HOME", xdg);
  }
  command.env("HOME", dir.join("home"));
  let output = command.output().expect("run dawn");
  String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn flag_wins_over_env_and_xdg() {
  let dir = temp_dir("flag");
  let flag = dir.join("flag.toml");
  let env = dir.join("env.toml");
  let xdg = dir.join("xdg");
  write_config(&flag);
  write_config(&env);
  write_config(&xdg.join("dawn/config.toml"));

  let mut command = Command::new(env!("CARGO_BIN_EXE_dawn"));
  let output = command
    .arg("--check-config")
    .arg("--config")
    .arg(&flag)
    .env("DAWN_CONFIG", &env)
    .env("XDG_CONFIG_HOME", &xdg)
    .output()
    .expect("run dawn");
  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(
    stdout.contains(&format!("config: {}", flag.display())),
    "{stdout}"
  );
  assert!(!stdout.contains(&env.display().to_string()), "{stdout}");
  std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn env_wins_over_xdg() {
  let dir = temp_dir("env");
  let env = dir.join("env.toml");
  let xdg = dir.join("xdg");
  write_config(&env);
  write_config(&xdg.join("dawn/config.toml"));

  let stdout = check_config(&dir, Some(&env), Some(&xdg));
  assert!(
    stdout.contains(&format!("config: {}", env.display())),
    "{stdout}"
  );
  assert!(!stdout.contains(&xdg.display().to_string()), "{stdout}");
  std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn xdg_is_never_consulted_without_an_explicit_file() {
  let dir = temp_dir("xdg-only");
  let xdg = dir.join("xdg");
  write_config(&xdg.join("dawn/config.toml"));

  let stdout = check_config(&dir, None, Some(&xdg));
  // The system config exists on some hosts and may select a theme, so the
  // exact path is host-dependent. The invariant is that the XDG file, which is
  // the only config this test controls, is never the one that wins.
  assert!(stdout.contains("config:"), "{stdout}");
  assert!(!stdout.contains(&xdg.display().to_string()), "{stdout}");
  std::fs::remove_dir_all(dir).expect("cleanup");
}
