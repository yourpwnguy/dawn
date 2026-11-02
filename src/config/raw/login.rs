//! Login section of the raw config schema.

use super::*;
use serde::Deserialize;

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
  pub prefill: Option<Field>,
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
  #[serde(default)]
  pub splash: RawSplash,
  #[serde(default)]
  pub transition: RawTransition,
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
  pub glass: Option<Field>,
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
  pub pulse_on_beat: Option<Field>,
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
  #[serde(default)]
  pub parallax_depth: Option<Field>,
  #[serde(default)]
  pub enter: Option<Field>,
  #[serde(default)]
  pub enter_ms: Option<Field>,
  #[serde(default)]
  pub enter_delay_ms: Option<Field>,
  #[serde(default)]
  pub enter_from: Option<Field>,
  #[serde(default)]
  pub wave: Option<Field>,
  #[serde(default)]
  pub wave_ms: Option<Field>,
  #[serde(default)]
  pub wave_step: Option<Field>,
  #[serde(default)]
  pub scramble_ms: Option<Field>,
  #[serde(default)]
  pub glitch: Option<Field>,
  #[serde(default)]
  pub glitch_ms: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawField {
  #[serde(default)]
  pub label: Option<Field>,
  #[serde(default)]
  pub label_color: Option<Field>,
  #[serde(default)]
  pub label_size: Option<Field>,
  #[serde(default)]
  pub label_tracking: Option<Field>,
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
  #[serde(default)]
  pub key_pop_ms: Option<Field>,
  #[serde(default)]
  pub key_pop_scale: Option<Field>,
  #[serde(default)]
  pub key_pop_color: Option<Field>,
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
