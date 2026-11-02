use super::{ClockSpec, face_size, format, segment_mask};
use crate::config::{ClockConfig, ClockStyle, SecondHand, WidgetLayer, WidgetLook};
use crate::layout::{Anchor, Placement};
use crate::ui::text::TextEngine;

fn now() -> jiff::Zoned {
  jiff::Timestamp::from_second(0)
    .unwrap()
    .in_tz("UTC")
    .unwrap()
}

fn spec() -> ClockSpec {
  ClockSpec {
    style: ClockStyle::Digital,
    format: "%H:%M".to_string(),
    date_format: "%Y-%m-%d".to_string(),
    hour12: false,
    seconds: false,
    timezone: None,
    posix_locale: false,
    size: 54.0,
    date_size: 18.0,
    color: None,
    date_color: None,
    label: String::new(),
    label_color: None,
    label_size: 0.0,
    label_tracking: 0.0,
    ghost: false,
    glow: None,
    glow_size: 0.0,
    colon_blink_ms: 0.0,
    second_hand: SecondHand::Tick,
  }
}

#[test]
fn formats_with_jiff_strftime() {
  assert_eq!(format("%Y-%m-%d %H:%M", &now(), false), "1970-01-01 00:00");
}

#[test]
fn a_bad_pattern_degrades_to_empty() {
  assert_eq!(format("%E", &now(), false), "");
}

#[test]
fn seconds_and_hour12_are_applied() {
  let mut spec = spec();
  spec.seconds = true;
  spec.hour12 = true;
  assert_eq!(spec.effective_format(), "%I:%M:%S %p");
}

#[test]
fn signature_only_changes_when_visible_value_changes() {
  let spec = spec();
  let first = spec.signature(&now());
  let second = spec.signature(&now());
  assert_eq!(first, second);
  let mut seconds = spec.clone();
  seconds.style = ClockStyle::Analog;
  seconds.seconds = true;
  assert!(seconds.signature(&now()).contains("00:00:00"));
}

#[test]
fn config_conversion_keeps_placement_shape() {
  let config = ClockConfig {
    enabled: true,
    style: ClockStyle::Analog,
    layer: WidgetLayer::Behind,
    time_format: "%H:%M".to_string(),
    date_format: String::new(),
    hour12: false,
    seconds: true,
    timezone: Some("UTC".to_string()),
    posix_locale: true,
    placement: Placement {
      anchor: Anchor::TopLeft,
      offset: (1, 2),
      monitor: None,
      rotation: 0.0,
    },
    size: 80.0,
    date_size: 16.0,
    color: None,
    date_color: None,
    label: String::new(),
    label_color: None,
    label_size: 0.0,
    label_tracking: 0.0,
    ghost: true,
    glow: Some(crate::color::Rgb::solid(1, 2, 3)),
    glow_size: 6.0,
    colon_blink_ms: 500.0,
    second_hand: SecondHand::Sweep,
    draggable: true,
    look: WidgetLook::default(),
  };
  let spec = ClockSpec::from_config(&config);
  assert_eq!(spec.style, ClockStyle::Analog);
  assert!(spec.timezone.is_some());
  assert!(spec.ghost);
  assert!(spec.glow.is_some());
  assert_eq!(spec.second_hand, SecondHand::Sweep);
}

#[test]
fn flip_and_segment_parse_and_bad_styles_return_none() {
  assert_eq!(ClockStyle::parse("flip"), Some(ClockStyle::Flip));
  assert_eq!(ClockStyle::parse("segment"), Some(ClockStyle::Segment));
  assert_eq!(ClockStyle::parse("digital"), Some(ClockStyle::Digital));
  assert_eq!(ClockStyle::parse("analog"), Some(ClockStyle::Analog));
  assert_eq!(ClockStyle::parse("sundial"), None);
  assert_eq!(SecondHand::parse("sweep"), Some(SecondHand::Sweep));
  assert_eq!(SecondHand::parse("tick"), Some(SecondHand::Tick));
  assert_eq!(SecondHand::parse("wiggle"), None);
}

#[test]
fn decorative_faces_measure_positive_and_include_the_colon() {
  let mut text = TextEngine::new("Inter");
  let mut spec = spec();
  spec.style = ClockStyle::Flip;
  let (flip_width, flip_height) = face_size(&mut text, &spec, "12:34", 54.0);
  let (flip_no_colon, _) = face_size(&mut text, &spec, "1234", 54.0);
  assert!(flip_width > 0.0 && flip_height > 0.0);
  assert!(
    flip_height > 54.0,
    "a flip card is taller than the glyph size"
  );
  assert!(
    flip_width > flip_no_colon,
    "the colon must occupy a half-width card"
  );

  spec.style = ClockStyle::Segment;
  let (segment_width, segment_height) = face_size(&mut text, &spec, "12:34", 54.0);
  let (segment_no_colon, _) = face_size(&mut text, &spec, "1234", 54.0);
  assert!(segment_width > 0.0 && segment_height > 0.0);
  assert!(
    segment_width > segment_no_colon,
    "the colon must occupy a half-width slot"
  );
}

#[test]
fn the_segment_map_covers_every_digit_and_has_the_expected_masks() {
  for digit in '0'..='9' {
    assert!(
      segment_mask(digit).is_some(),
      "missing segment geometry for {digit}"
    );
  }
  assert_eq!(segment_mask('0').unwrap().count_ones(), 6);
  assert_eq!(segment_mask('1').unwrap().count_ones(), 2);
  assert_eq!(segment_mask('8').unwrap().count_ones(), 7);
  assert_eq!(segment_mask(':'), None);
}
