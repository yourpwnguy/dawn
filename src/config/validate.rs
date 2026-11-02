//! Turns the permissive [`RawConfig`] into a validated [`Config`], recording a
//! precise issue for every value that had to fall back.
//!
//! The rule here is "degrade one field at a time". A typo in the accent colour
//! must not take the clock down with it, and a bad clock format must not throw
//! away the session directories. Only a file that cannot be parsed at all
//! falls back wholesale (handled in `mod.rs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use toml::Value;

use crate::audio::{AmbientSpec, AudioConfig, Event, EventSound, Retrigger};
use crate::color::Rgb;
use crate::config::raw::{
  Field, RawAmbient, RawAudio, RawAudioEvent, RawBackgroundOverride, RawConfig, RawField, RawGpu,
  RawParticles, RawRituals, RawSplash, RawTicker, RawTransition, RawWidgetPlate,
};
use crate::config::{
  AuthConfig, BackgroundConfig, BackgroundKind, BackgroundOverride, BatteryConfig, BatteryStyle,
  BeatConfig, BlurConfig, ButtonEffect, CalendarConfig, CameraConfig, CaretStyle, ClockConfig,
  ClockStyle, Config, DecorMotion, EnterFrom, EnterStyle, FieldConfig, GlitchOn, GpuBackend,
  GpuConfig, GpuDevice, GpuFallback, GpuPower, GpuPresent, HintConfig, HitstopConfig, HoverReveal,
  Issue, LayoutConfig, Location, LoginConfig, MediaArt, MediaConfig, MediaLayout, MediaLook,
  MediaProgress, MotionConfig, ParticleDrift, ParticleLayer, ParticleSprite, ParticlesConfig,
  PlateStyle, PluginsConfig, PostConfig, PowerConfig, SecondHand, SessionRowConfig, SessionsConfig,
  SpeedLinesOn, SplashConfig, SplashDismiss, SurfaceStyle, SystemConfig, SystemField, SystemStyle,
  ThemeConfig, ThemePreset, TickerAnchor, TickerConfig, TransitionConfig, TransitionTrigger,
  UiConfig, UsernamePrefill, WeekStart, WidgetLayer, WidgetLook, WidgetPlate, WidgetState,
};
use crate::layout::{Anchor, Fit, Placement};
use crate::rituals::{DropSeed, DropWeights, RitualsConfig};

const MAX_PATH_CHARS: usize = 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

pub struct Validator<'a> {
  path: &'a Path,
  source: &'a str,
  issues: Vec<Issue>,
}

impl<'a> Validator<'a> {
  pub fn new(path: &'a Path, source: &'a str) -> Self {
    Self {
      path,
      source,
      issues: Vec::new(),
    }
  }

  pub fn into_issues(self) -> Vec<Issue> {
    self.issues
  }
}

mod build;
mod build_background;
mod build_login;
mod build_motion;
mod build_widgets;
mod button;
mod decor;
mod fields;
mod paths;
mod resources;
mod sections;
mod values;
mod widgets;

use paths::*;

#[cfg(test)]
mod tests;
