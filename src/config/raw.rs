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

use std::collections::BTreeMap;

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
  pub layout: RawLayout,
  #[serde(default)]
  pub sessions: RawSessions,
  #[serde(default)]
  pub plugins: RawPlugins,
  #[serde(default)]
  pub auth: RawAuth,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLogin {
  #[serde(default)]
  pub layout: Option<Field>,
  #[serde(default)]
  pub anchor: Option<Field>,
  #[serde(default)]
  pub offset: Option<Field>,
  #[serde(default)]
  pub monitor: Option<Field>,
  #[serde(default)]
  pub width: Option<Field>,
  #[serde(default)]
  pub height: Option<Field>,
  #[serde(default)]
  pub padding: Option<Field>,
  #[serde(default)]
  pub gap: Option<Field>,
  #[serde(default)]
  pub radius: Option<Field>,
  #[serde(default)]
  pub panel: Option<Field>,
  #[serde(default)]
  pub panel_fade: Option<Field>,
  #[serde(default)]
  pub border: Option<Field>,
  #[serde(default)]
  pub surface: Option<Field>,
  #[serde(default)]
  pub show_shadow: Option<Field>,
  #[serde(default)]
  pub show_border: Option<Field>,
  #[serde(default)]
  pub rotation: Option<Field>,
  #[serde(default)]
  pub draggable: Option<Field>,
  #[serde(default)]
  pub greeting: Option<Field>,
  #[serde(default)]
  pub show_session: Option<Field>,
  #[serde(default)]
  pub show_hints: Option<Field>,
  #[serde(default)]
  pub show_picker: Option<Field>,
  #[serde(default)]
  pub picker_title: Option<Field>,
  #[serde(default)]
  pub busy_text: Option<Field>,
  #[serde(default)]
  pub dry_run_badge: Option<Field>,
  #[serde(default)]
  pub show_dry_run_badge: Option<Field>,
  #[serde(default)]
  pub no_sessions: Option<Field>,
  #[serde(default)]
  pub field: RawField,
  #[serde(default)]
  pub username: RawField,
  #[serde(default)]
  pub password: RawField,
  #[serde(default)]
  pub session: RawSession,
  #[serde(default)]
  pub hints: RawHints,
  #[serde(default)]
  pub tab: Option<Field>,
  #[serde(default)]
  pub form_offset: Option<Field>,
  #[serde(default)]
  pub form_width: Option<Field>,
  #[serde(default)]
  pub button: RawButton,
  #[serde(default, rename = "decor")]
  pub decor: Vec<RawDecor>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawButton {
  #[serde(default)]
  pub show: Option<Field>,
  #[serde(default)]
  pub text: Option<Field>,
  #[serde(default)]
  pub gap: Option<Field>,
  #[serde(default)]
  pub height: Option<Field>,
  #[serde(default)]
  pub radius: Option<Field>,
  #[serde(default)]
  pub font_size: Option<Field>,
  #[serde(default)]
  pub background: Option<Field>,
  #[serde(default)]
  pub background_end: Option<Field>,
  #[serde(default)]
  pub gradient_angle: Option<Field>,
  #[serde(default)]
  pub text_color: Option<Field>,
  #[serde(default)]
  pub bracket: Option<Field>,
  #[serde(default)]
  pub bracket_gap: Option<Field>,
  #[serde(default)]
  pub bold: Option<Field>,
  #[serde(default)]
  pub tracking: Option<Field>,
  #[serde(default)]
  pub hover_background: Option<Field>,
  #[serde(default)]
  pub hover_background_end: Option<Field>,
  #[serde(default)]
  pub hover_text_color: Option<Field>,
  #[serde(default)]
  pub hover_direction: Option<Field>,
  #[serde(default)]
  pub hover_ms: Option<Field>,
  #[serde(default)]
  pub hover_scale: Option<Field>,
  #[serde(default)]
  pub hover_lift: Option<Field>,
  #[serde(default)]
  pub glow: Option<Field>,
  #[serde(default)]
  pub glow_size: Option<Field>,
  #[serde(default)]
  pub effect: Option<Field>,
  #[serde(default)]
  pub effect_color: Option<Field>,
  #[serde(default)]
  pub effect_ms: Option<Field>,
  #[serde(default)]
  pub effect_angle: Option<Field>,
  #[serde(default)]
  pub effect_strength: Option<Field>,
  #[serde(default)]
  pub rotation: Option<Field>,
}

/// A decorative image or text run inside the login card.
///
/// This is a flat struct rather than a tagged enum because `toml`'s spanned
/// values do not survive enum deserialisation here. Keeping every field
/// optional also means a typo inside one item only skips that item instead of
/// rejecting the whole array.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDecor {
  #[serde(default)]
  pub kind: Option<Field>,
  #[serde(default)]
  pub text: Option<Field>,
  #[serde(default)]
  pub image: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub color_end: Option<Field>,
  #[serde(default)]
  pub gradient_angle: Option<Field>,
  #[serde(default)]
  pub radius: Option<Field>,
  #[serde(default)]
  pub border: Option<Field>,
  #[serde(default)]
  pub bold: Option<Field>,
  #[serde(default)]
  pub tracking: Option<Field>,
  #[serde(default)]
  pub stroke: Option<Field>,
  #[serde(default)]
  pub stroke_width: Option<Field>,
  #[serde(default)]
  pub width: Option<Field>,
  #[serde(default)]
  pub height: Option<Field>,
  #[serde(default)]
  pub anchor: Option<Field>,
  #[serde(default)]
  pub offset: Option<Field>,
  #[serde(default)]
  pub opacity: Option<Field>,
  #[serde(default)]
  pub rotation: Option<Field>,
  #[serde(default)]
  pub motion: Option<RawDecorMotion>,
}

/// Motion subtable for one decor item. TOML attaches this to the last
/// `[[login.decor]]` entry, for example:
///
/// ```toml
/// [[login.decor]]
/// kind = "image"
/// image = "spark.png"
/// [login.decor.motion]
/// float = [8, -12]
/// fade = 0.4
/// ```
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDecorMotion {
  #[serde(default)]
  pub spin: Option<Field>,
  #[serde(default)]
  pub sway: Option<Field>,
  #[serde(default)]
  pub sway_ms: Option<Field>,
  #[serde(default)]
  pub float: Option<Field>,
  #[serde(default)]
  pub float_ms: Option<Field>,
  #[serde(default)]
  pub pulse: Option<Field>,
  #[serde(default)]
  pub pulse_ms: Option<Field>,
  #[serde(default)]
  pub fade: Option<Field>,
  #[serde(default)]
  pub fade_ms: Option<Field>,
  #[serde(default)]
  pub phase: Option<Field>,
  #[serde(default)]
  pub typer_ms: Option<Field>,
  #[serde(default)]
  pub blink_ms: Option<Field>,
  #[serde(default)]
  pub ticker_ms: Option<Field>,
  #[serde(default)]
  pub ticker_width: Option<Field>,
  #[serde(default)]
  pub ticker_gap: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawField {
  #[serde(default)]
  pub label: Option<Field>,
  #[serde(default)]
  pub label_color: Option<Field>,
  #[serde(default)]
  pub placeholder: Option<Field>,
  #[serde(default)]
  pub font_size: Option<Field>,
  #[serde(default)]
  pub height: Option<Field>,
  #[serde(default)]
  pub radius: Option<Field>,
  #[serde(default)]
  pub padding: Option<Field>,
  #[serde(default)]
  pub background: Option<Field>,
  #[serde(default)]
  pub background_end: Option<Field>,
  #[serde(default)]
  pub border: Option<Field>,
  #[serde(default)]
  pub underline: Option<Field>,
  #[serde(default)]
  pub underline_size: Option<Field>,
  #[serde(default)]
  pub bracket: Option<Field>,
  #[serde(default)]
  pub bracket_gap: Option<Field>,
  #[serde(default)]
  pub text: Option<Field>,
  #[serde(default)]
  pub placeholder_color: Option<Field>,
  #[serde(default)]
  pub focus: Option<Field>,
  #[serde(default)]
  pub caret: Option<Field>,
  #[serde(default)]
  pub caret_width: Option<Field>,
  #[serde(default)]
  pub caret_style: Option<Field>,
  #[serde(default)]
  pub icon: Option<Field>,
  #[serde(default)]
  pub icon_size: Option<Field>,
  #[serde(default)]
  pub icon_gap: Option<Field>,
  #[serde(default)]
  pub icon_color: Option<Field>,
  #[serde(default)]
  pub rotation: Option<Field>,
  #[serde(default)]
  pub mask: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSession {
  #[serde(default)]
  pub label: Option<Field>,
  #[serde(default)]
  pub change_hint: Option<Field>,
  #[serde(default)]
  pub empty: Option<Field>,
  #[serde(default)]
  pub show: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawHints {
  #[serde(default)]
  pub username: Option<Field>,
  #[serde(default)]
  pub password: Option<Field>,
  #[serde(default)]
  pub picker: Option<Field>,
  #[serde(default)]
  pub busy: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLayout {
  #[serde(default)]
  pub save: Option<Field>,
  #[serde(default)]
  pub file: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawUi {
  #[serde(default)]
  pub font: Option<Field>,
  #[serde(default)]
  pub font_size: Option<Field>,
  #[serde(default)]
  pub card_width: Option<Field>,
  #[serde(default)]
  pub fullscreen: Option<Field>,
  #[serde(default)]
  pub greeting: Option<Field>,
}

/// Plate subtable for a built-in clock or media widget. Every key is optional,
/// and the resolved defaults reproduce the historical scrim.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawWidgetPlate {
  #[serde(default)]
  pub style: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub color_end: Option<Field>,
  #[serde(default)]
  pub gradient_angle: Option<Field>,
  #[serde(default)]
  pub radius: Option<Field>,
  #[serde(default)]
  pub border: Option<Field>,
  #[serde(default)]
  pub padding: Option<Field>,
  #[serde(default)]
  pub glow: Option<Field>,
  #[serde(default)]
  pub glow_size: Option<Field>,
  #[serde(default)]
  pub shadow: Option<Field>,
  #[serde(default)]
  pub opacity: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawClock {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub style: Option<Field>,
  #[serde(default)]
  pub layer: Option<Field>,
  #[serde(default)]
  pub plate: RawWidgetPlate,
  #[serde(default)]
  pub motion: Option<RawDecorMotion>,
  #[serde(default)]
  pub show_on: Option<Field>,
  #[serde(default)]
  pub hide_on: Option<Field>,
  #[serde(default)]
  pub time_format: Option<Field>,
  #[serde(default)]
  pub date_format: Option<Field>,
  #[serde(default)]
  pub hour12: Option<Field>,
  #[serde(default)]
  pub seconds: Option<Field>,
  #[serde(default)]
  pub timezone: Option<Field>,
  #[serde(default)]
  pub posix_locale: Option<Field>,
  #[serde(default)]
  pub anchor: Option<Field>,
  #[serde(default)]
  pub offset: Option<Field>,
  #[serde(default)]
  pub monitor: Option<Field>,
  #[serde(default)]
  pub rotation: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub date_size: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub date_color: Option<Field>,
  #[serde(default)]
  pub ghost: Option<Field>,
  #[serde(default)]
  pub glow: Option<Field>,
  #[serde(default)]
  pub glow_size: Option<Field>,
  #[serde(default)]
  pub colon_blink_ms: Option<Field>,
  #[serde(default)]
  pub second_hand: Option<Field>,
  #[serde(default)]
  pub draggable: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMedia {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub layer: Option<Field>,
  #[serde(default)]
  pub plate: RawWidgetPlate,
  #[serde(default)]
  pub motion: Option<RawDecorMotion>,
  #[serde(default)]
  pub show_on: Option<Field>,
  #[serde(default)]
  pub hide_on: Option<Field>,
  #[serde(default)]
  pub anchor: Option<Field>,
  #[serde(default)]
  pub offset: Option<Field>,
  #[serde(default)]
  pub monitor: Option<Field>,
  #[serde(default)]
  pub rotation: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub show_art: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub layout: Option<Field>,
  #[serde(default)]
  pub art: Option<Field>,
  #[serde(default)]
  pub progress: Option<Field>,
  #[serde(default)]
  pub controls: Option<Field>,
  #[serde(default)]
  pub tint_from_art: Option<Field>,
  #[serde(default)]
  pub show_before_login: Option<Field>,
  #[serde(default)]
  pub keep_last: Option<Field>,
  #[serde(default)]
  pub draggable: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBackground {
  #[serde(default)]
  pub kind: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub gradient: Option<Field>,
  #[serde(default)]
  pub image: Option<Field>,
  #[serde(default)]
  pub fit: Option<Field>,
  #[serde(default)]
  pub fps: Option<Field>,
  #[serde(default)]
  pub memory_limit_mb: Option<Field>,
  #[serde(default)]
  pub idle_pause_secs: Option<Field>,
  #[serde(default)]
  pub monitors: BTreeMap<String, RawBackgroundOverride>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBackgroundOverride {
  #[serde(default)]
  pub kind: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub image: Option<Field>,
  #[serde(default)]
  pub fit: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTheme {
  /// Select a complete theme bundle from the theme store. The bundle's own
  /// config replaces this file; it is a pointer, not an overlay.
  #[serde(default)]
  pub package: Option<Field>,
  #[serde(default)]
  pub preset: Option<Field>,
  #[serde(default)]
  pub text: Option<Field>,
  #[serde(default)]
  pub muted: Option<Field>,
  #[serde(default)]
  pub accent: Option<Field>,
  #[serde(default)]
  pub error: Option<Field>,
  /// Legacy name for the glass tint.
  #[serde(default)]
  pub card: Option<Field>,
  #[serde(default)]
  pub tint: Option<Field>,
  #[serde(default)]
  pub border: Option<Field>,
  #[serde(default)]
  pub shadow: Option<Field>,
  #[serde(default)]
  pub focus_ring: Option<Field>,
  /// Legacy alias for `[theme.blur].size`.
  #[serde(default)]
  pub blur_radius: Option<Field>,
  /// Legacy alias for `[theme.blur].refresh_ms`.
  #[serde(default)]
  pub blur_refresh_ms: Option<Field>,
  /// Accepted for old configs; the dual Kawase pyramid does not use a single
  /// downsample factor.
  #[serde(default)]
  pub blur_downsample: Option<Field>,
  #[serde(default)]
  pub blur: RawBlur,
  #[serde(default)]
  pub corner_radius: Option<Field>,
  #[serde(default)]
  pub spacing: Option<Field>,
  #[serde(default)]
  pub noise: Option<Field>,
  #[serde(default)]
  pub min_contrast: Option<Field>,
}

/// Hyprland's `decoration:blur` block, minus the compositor-only switches.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBlur {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub size: Option<Field>,
  #[serde(default)]
  pub passes: Option<Field>,
  #[serde(default)]
  pub noise: Option<Field>,
  #[serde(default)]
  pub contrast: Option<Field>,
  #[serde(default)]
  pub brightness: Option<Field>,
  #[serde(default)]
  pub vibrancy: Option<Field>,
  #[serde(default)]
  pub vibrancy_darkness: Option<Field>,
  #[serde(default)]
  pub refresh_ms: Option<Field>,
  // Compositor-only switches accepted so a Hyprland blur block can be pasted
  // without a wall of warnings. A single full-screen greeter is always opaque,
  // so none of these change its pixels.
  #[serde(default)]
  pub ignore_opacity: Option<Field>,
  #[serde(default)]
  pub new_optimizations: Option<Field>,
  #[serde(default)]
  pub xray: Option<Field>,
  #[serde(default)]
  pub special: Option<Field>,
  #[serde(default)]
  pub popups: Option<Field>,
  #[serde(default)]
  pub popup_ignore_alpha: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMotion {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub reduced: Option<Field>,
  #[serde(default)]
  pub boot_ms: Option<Field>,
  #[serde(default)]
  pub focus_ms: Option<Field>,
  #[serde(default)]
  pub shake_ms: Option<Field>,
  #[serde(default)]
  pub drag_ms: Option<Field>,
  #[serde(default)]
  pub decor_fps: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPower {
  #[serde(default)]
  pub low_power: Option<Field>,
}

/// The `[gpu]` table. Each key stays a raw spanned value so a bad choice string
/// can report its own line and fall back without disturbing the rest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawGpu {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub backend: Option<Field>,
  #[serde(default)]
  pub adapter: Option<Field>,
  #[serde(default)]
  pub device: Option<Field>,
  #[serde(default)]
  pub power: Option<Field>,
  #[serde(default)]
  pub present: Option<Field>,
  #[serde(default)]
  pub fallback: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSessions {
  #[serde(default)]
  pub dirs: Option<Field>,
  #[serde(default)]
  pub default: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPlugins {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub dirs: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAuth {
  #[serde(default)]
  pub socket: Option<Field>,
  #[serde(default)]
  pub timeout_secs: Option<Field>,
}
