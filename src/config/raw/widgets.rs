//! Widgets section of the raw config schema.

use super::*;
use serde::Deserialize;

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
  pub label: Option<Field>,
  #[serde(default)]
  pub label_color: Option<Field>,
  #[serde(default)]
  pub label_size: Option<Field>,
  #[serde(default)]
  pub label_tracking: Option<Field>,
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
pub struct RawCalendar {
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
  pub width: Option<Field>,
  #[serde(default)]
  pub week_start: Option<Field>,
  #[serde(default)]
  pub show_header: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub muted_color: Option<Field>,
  #[serde(default)]
  pub today_color: Option<Field>,
  #[serde(default)]
  pub header_color: Option<Field>,
  #[serde(default)]
  pub draggable: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBattery {
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
  pub width: Option<Field>,
  #[serde(default)]
  pub style: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub low_color: Option<Field>,
  #[serde(default)]
  pub charging_color: Option<Field>,
  #[serde(default)]
  pub low_at: Option<Field>,
  #[serde(default)]
  pub show_percent: Option<Field>,
  #[serde(default)]
  pub draggable: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSystem {
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
  pub width: Option<Field>,
  #[serde(default)]
  pub style: Option<Field>,
  #[serde(default)]
  pub fields: Option<Field>,
  #[serde(default)]
  pub color: Option<Field>,
  #[serde(default)]
  pub label_color: Option<Field>,
  #[serde(default)]
  pub draggable: Option<Field>,
}
