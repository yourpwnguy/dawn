//! The permissive, span-preserving shape of a config file.
//!
//! Every value is kept as a [`toml::Value`] inside a [`Spanned`] instead of
//! deserialising straight into typed fields. That buys two things:
//!
//! 1. A typo or wrong type in one field does not throw away the whole file.
//!    The validator can fall back per field and point at the exact line.
//! 2. Validation messages are ours, not serde's. "ui.font_size must be between
//!    10 and 72" with a line number is a lot more useful at boot than an
//!    "invalid type: string" dump.

use serde::Deserialize;
use toml::{Spanned, Value};

/// One value plus its byte range in the original document.
pub type Field = Spanned<Value>;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
  #[serde(default)]
  pub ui: RawUi,
  #[serde(default)]
  pub login: RawLogin,
  #[serde(default)]
  pub clock: RawClock,
  #[serde(default)]
  pub media: RawMedia,
  #[serde(default)]
  pub calendar: RawCalendar,
  #[serde(default)]
  pub battery: RawBattery,
  #[serde(default)]
  pub system: RawSystem,
  #[serde(default)]
  pub particles: RawParticles,
  #[serde(default)]
  pub ticker: RawTicker,
  #[serde(default)]
  pub background: RawBackground,
  #[serde(default)]
  pub theme: RawTheme,
  #[serde(default)]
  pub motion: RawMotion,
  #[serde(default)]
  pub power: RawPower,
  #[serde(default)]
  pub gpu: RawGpu,
  #[serde(default)]
  pub audio: RawAudio,
  #[serde(default)]
  pub rituals: RawRituals,
  #[serde(default)]
  pub layout: RawLayout,
  #[serde(default)]
  pub sessions: RawSessions,
  #[serde(default)]
  pub plugins: RawPlugins,
  #[serde(default)]
  pub auth: RawAuth,
}

mod background;
mod login;
mod motion;
mod resources;
mod widgets;

pub use background::*;
pub use login::*;
pub use motion::*;
pub use resources::*;
pub use widgets::*;
