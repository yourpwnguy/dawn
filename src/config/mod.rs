//! Config loading, validation, and defaults.
//!
//! Config errors must never prevent a login. So `load` always returns a usable
//! [`Config`], plus a list of [`Issue`]s describing every field that fell back
//! to its default. The UI shows a short summary and the log has the details;
//! `dawn --check-config` turns the same data into an admin-friendly report.
//!
//! Precedence: `--config PATH`, then `DAWN_CONFIG`, then
//! `/etc/dawn/config.toml`, then built-in defaults. Relative paths inside the
//! file are resolved against the config file's directory, and `~/` is expanded.

mod raw;
mod validate;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::audio::AudioConfig;
use crate::color::Rgb;
use crate::layout::{Anchor, Fit, Placement};
use crate::rituals::RitualsConfig;
use raw::RawConfig;
use validate::Validator;

/// The machine config a real greeter reads. There is exactly one default path:
/// a greeter runs as the `greeter` user with no useful `HOME`, so a per-user
/// XDG file would be invisible at boot anyway.
mod app;
mod background;
mod decor;
mod field;
mod gpu;
mod issue;
mod load;
mod login;
mod motion;
mod particles;
mod power;
mod splash;
mod theme;
mod ticker;
mod widget_config;
mod widget_style;

pub use app::{AuthConfig, Config, PluginsConfig, SessionsConfig, UiConfig};
pub use background::{BackgroundConfig, BackgroundKind, BackgroundOverride};
pub use decor::{DecorMotion, DecorSample, DecorSpec, EnterFrom, EnterStyle};
pub use field::{CaretStyle, FieldConfig, HintConfig, LayoutConfig, SessionRowConfig};
pub use gpu::{GpuBackend, GpuConfig, GpuDevice, GpuFallback, GpuPower, GpuPresent};
pub use issue::{Issue, LoadedConfig, Location};
#[cfg(test)]
pub(crate) use load::discover_from;
pub(crate) use load::location_of;
pub use load::{load, load_theme};
pub use login::{
  ButtonConfig, ButtonEffect, HoverReveal, LoginConfig, LoginLayout, SurfaceStyle, UsernamePrefill,
  valid_username,
};
pub use motion::{
  BeatConfig, CameraConfig, GlitchOn, HitstopConfig, MotionConfig, MotionMoment, PostConfig,
  SpeedLinesOn,
};
pub use particles::{
  BURST_LIFETIME, BurstTrigger, MAX_BURST_PARTICLES, MAX_PARTICLES, ParticleDrift, ParticleLayer,
  ParticleSprite, ParticlesConfig, SESSION_FLASH_MS,
};
pub use power::PowerConfig;
pub use splash::{SplashConfig, SplashDismiss, TransitionConfig, TransitionTrigger};
pub use theme::{BlurConfig, ThemeConfig, ThemePreset};
pub use ticker::{TickerAnchor, TickerConfig};
pub use widget_config::{
  BatteryConfig, CalendarConfig, ClockConfig, MediaArt, MediaConfig, MediaLayout, MediaLook,
  MediaProgress, SystemConfig,
};
pub use widget_style::{
  BatteryStyle, ClockStyle, PlateStyle, SecondHand, SystemField, SystemStyle, WeekStart,
  WidgetLayer, WidgetLook, WidgetPlate, WidgetState,
};

pub const SYSTEM_CONFIG: &str = "/etc/dawn/config.toml";

/// Environment variable that names a config file explicitly.
pub const ENV_CONFIG: &str = "DAWN_CONFIG";

/// Printed once when no config file was found. The wording is stable because
/// it is the only instruction an admin sees before the login prompt.
pub const MISSING_CONFIG_HINT: &str = "no machine config; create it: sudo dawn init";

/// Returns the one-line hint for a run that found no config, and `None` when a
/// file was loaded. Kept pure so the "exactly once" behaviour is testable.
#[must_use]
pub fn missing_config_hint(path: Option<&Path>) -> Option<&'static str> {
  path.is_none().then_some(MISSING_CONFIG_HINT)
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod precedence_tests {
  use std::ffi::OsString;
  use std::path::{Path, PathBuf};

  use super::{MISSING_CONFIG_HINT, discover_from, missing_config_hint};

  fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-precedence-{name}-{}-{}",
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
  fn env_config_beats_the_system_path() {
    let dir = temp_dir("env");
    let env = dir.join("env.toml");
    let system = dir.join("system.toml");
    std::fs::write(&env, "[ui]\nfont_size = 17\n").expect("env config");
    std::fs::write(&system, "[ui]\nfont_size = 18\n").expect("system config");
    assert_eq!(
      discover_from(Some(OsString::from(env.to_str().unwrap())), &system),
      Some(env)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
  }

  #[test]
  fn system_path_is_used_when_env_is_absent_or_missing() {
    let dir = temp_dir("system");
    let system = dir.join("system.toml");
    std::fs::write(&system, "[ui]\nfont_size = 18\n").expect("system config");
    assert_eq!(discover_from(None, &system), Some(system.clone()));
    assert_eq!(
      discover_from(Some(OsString::from("/nonexistent/dawn.toml")), &system),
      Some(system)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
  }

  #[test]
  fn absent_everything_means_defaults_and_one_hint() {
    let dir = temp_dir("absent");
    let system = dir.join("missing.toml");
    assert_eq!(discover_from(None, &system), None);
    assert_eq!(missing_config_hint(None), Some(MISSING_CONFIG_HINT));
    assert_eq!(
      MISSING_CONFIG_HINT,
      "no machine config; create it: sudo dawn init"
    );
    // A loaded path suppresses the hint, which is what makes main print it at
    // most once per run.
    assert_eq!(
      missing_config_hint(Some(Path::new("/etc/dawn/config.toml"))),
      None
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
  }

  #[test]
  fn xdg_config_home_is_not_a_discovery_input() {
    // The test creates a valid config under a fake XDG tree and proves the
    // resolver has no way to reach it: `discover_from` takes only the env
    // override and the system path. This is stronger than setting the process
    // variable, which would leak into parallel tests.
    let dir = temp_dir("xdg");
    let xdg_config = dir.join("xdg/dawn/config.toml");
    std::fs::create_dir_all(xdg_config.parent().expect("parent")).expect("xdg dir");
    std::fs::write(&xdg_config, "[ui]\nfont_size = 30\n").expect("xdg config");
    let system = dir.join("system.toml");
    std::fs::write(&system, "[ui]\nfont_size = 17\n").expect("system config");

    assert_ne!(
      discover_from(None, &system),
      Some(xdg_config),
      "the XDG path must never win"
    );
    assert_eq!(discover_from(None, &system), Some(system));
    std::fs::remove_dir_all(dir).expect("cleanup");
  }
}
