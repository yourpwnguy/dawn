//! Clock formatting and drawing for the built-in and plugin clock widgets.
//!
//! The clock is a first-class widget (the built-in plugin and a third-party
//! manifest both produce the same [`ClockSpec`]). It supports digital and
//! analog faces, 12/24-hour time, optional seconds and date, and an optional
//! IANA time zone. Redraws are driven by [`ClockSpec::signature`], so a
//! minute-resolution clock does not repaint sixty times a minute.

use std::f32::consts::TAU;

use cosmic_text::{Align, Weight};
use jiff::Zoned;
use jiff::fmt::strtime::{BrokenDownTime, Config as StrftimeConfig, PosixCustom};
use jiff::tz::TimeZone;

use crate::color::Rgb;
use crate::config::{ClockConfig, ClockStyle, PlateStyle, SecondHand, WidgetPlate};
use crate::layout::Anchor;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};
use crate::ui::theme::Theme;

#[derive(Clone)]
pub struct ClockSpec {
  pub style: ClockStyle,
  pub format: String,
  pub date_format: String,
  pub hour12: bool,
  pub seconds: bool,
  pub timezone: Option<TimeZone>,
  pub posix_locale: bool,
  pub size: f32,
  pub date_size: f32,
  pub color: Option<Rgb>,
  pub date_color: Option<Rgb>,
  /// Small label above the time face. Empty hides it.
  pub label: String,
  pub label_color: Option<Rgb>,
  /// Zero derives a size from the clock face.
  pub label_size: f32,
  pub label_tracking: f32,
  /// Ghost the unused segments of a seven-segment face.
  pub ghost: bool,
  /// Halo colour for a seven-segment face.
  pub glow: Option<Rgb>,
  /// Halo spread in logical pixels. Zero disables the glow.
  pub glow_size: f32,
  /// Colon blink period in milliseconds. Zero keeps the colon steady.
  pub colon_blink_ms: f32,
  /// Analog second-hand motion.
  pub second_hand: SecondHand,
}

impl std::fmt::Debug for ClockSpec {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("ClockSpec")
      .field("style", &self.style)
      .field("format", &self.format)
      .field("date_format", &self.date_format)
      .field("hour12", &self.hour12)
      .field("seconds", &self.seconds)
      .field(
        "timezone",
        &self.timezone.as_ref().and_then(TimeZone::iana_name),
      )
      .field("posix_locale", &self.posix_locale)
      .field("size", &self.size)
      .field("date_size", &self.date_size)
      .field("color", &self.color)
      .field("date_color", &self.date_color)
      .field("label", &self.label)
      .field("label_color", &self.label_color)
      .field("label_size", &self.label_size)
      .field("label_tracking", &self.label_tracking)
      .field("ghost", &self.ghost)
      .field("glow", &self.glow)
      .field("glow_size", &self.glow_size)
      .field("colon_blink_ms", &self.colon_blink_ms)
      .field("second_hand", &self.second_hand)
      .finish()
  }
}

impl ClockSpec {
  #[must_use]
  pub fn from_config(config: &ClockConfig) -> Self {
    Self {
      style: config.style,
      format: config.time_format.clone(),
      date_format: config.date_format.clone(),
      hour12: config.hour12,
      seconds: config.seconds,
      timezone: config
        .timezone
        .as_deref()
        .and_then(|name| TimeZone::get(name).ok()),
      posix_locale: config.posix_locale,
      size: config.size,
      date_size: config.date_size,
      color: config.color,
      date_color: config.date_color,
      label: config.label.clone(),
      label_color: config.label_color,
      label_size: config.label_size,
      label_tracking: config.label_tracking,
      ghost: config.ghost,
      glow: config.glow,
      glow_size: config.glow_size,
      colon_blink_ms: config.colon_blink_ms,
      second_hand: config.second_hand,
    }
  }

  /// The format string with the 12/24-hour and seconds choices applied.
  ///
  /// Seconds are inserted before the AM/PM marker rather than appended, so
  /// "14:05" with `hour12` and `seconds` becomes "2:05:09 PM", not
  /// "2:05 PM:09". An explicit `%I`, `%l`, or `%r` wins over `hour12`, and an
  /// explicit `%S` wins over `seconds`.
  #[must_use]
  pub fn effective_format(&self) -> String {
    let mut format = self.format.clone();
    let wants_12h =
      self.hour12 && !format.contains("%I") && !format.contains("%l") && !format.contains("%r");
    if wants_12h {
      format = format.replace("%H", "%I");
    }
    if self.seconds && !format.contains("%S") {
      match format.find("%p").or_else(|| format.find("%P")) {
        Some(index) => format.insert_str(index, ":%S"),
        None => format.push_str(":%S"),
      }
    }
    if wants_12h && !format.contains("%p") && !format.contains("%P") {
      format.push_str(" %p");
    }
    format
  }

  fn local(&self, now: &Zoned) -> Zoned {
    match &self.timezone {
      Some(zone) => now.with_time_zone(zone.clone()),
      None => now.clone(),
    }
  }

  /// The visible strings, used for drawing and for change detection.
  #[must_use]
  pub fn texts(&self, now: &Zoned) -> (String, String) {
    let local = self.local(now);
    let time = match self.style {
      // Flip and segment faces use the same formatted string as digital.
      // Only the analog dial has no text face.
      ClockStyle::Digital | ClockStyle::Flip | ClockStyle::Segment => {
        format(&self.effective_format(), &local, self.posix_locale)
      }
      ClockStyle::Analog => String::new(),
    };
    let date = if self.date_format.is_empty() {
      String::new()
    } else {
      format(&self.date_format, &local, self.posix_locale)
    };
    (time, date)
  }

  /// A string that changes exactly when the visible clock changes.
  #[must_use]
  pub fn signature(&self, now: &Zoned) -> String {
    let local = self.local(now);
    match self.style {
      ClockStyle::Digital | ClockStyle::Flip | ClockStyle::Segment => {
        let (time, date) = self.texts(now);
        format!("{time}\u{1f}{date}")
      }
      ClockStyle::Analog => {
        let date = if self.date_format.is_empty() {
          String::new()
        } else {
          format(&self.date_format, &local, self.posix_locale)
        };
        if self.seconds {
          format!(
            "{:02}:{:02}:{:02}\u{1f}{date}",
            local.hour(),
            local.minute(),
            local.second()
          )
        } else {
          format!("{:02}:{:02}\u{1f}{date}", local.hour(), local.minute())
        }
      }
    }
  }
}

/// Width the digital face is measured at. Cosmic-text needs a finite wrapping
/// limit even when wrapping is off, and a clock string is never this wide.
const FACE_MEASURE_WIDTH: f32 = 16_384.0;
/// Nominal advance of one seven-segment digit, as a fraction of the font size.
const DIGIT_WIDTH: f32 = 0.62;
/// Gap between characters on the flip and segment faces.
const FACE_GAP: f32 = 0.12;
/// Extra padding on each side of a flip card, as a fraction of the font size.
const FLIP_CARD_PADDING: f32 = 0.55;
/// A flip card is a little taller than the glyph so the seam has room.
const FLIP_CARD_HEIGHT: f32 = 1.28;

/// The time face's natural size, before the date line is added.
///
/// `bounds` and the drawing code both call this, which is what keeps the plate
/// and the hit region aligned: if they each measured a face differently, a
/// dragged flip clock would jump when the pointer grabbed it. Digital keeps the
/// shaped-text measurement, analog is a square dial, and the two decorative
/// faces derive their width from the character count so no font metrics leak
/// into their geometry.
#[must_use]
pub fn face_size(text: &mut TextEngine, spec: &ClockSpec, time: &str, size: f32) -> (f32, f32) {
  match spec.style {
    ClockStyle::Digital => text.measure(time, size, FACE_MEASURE_WIDTH, false),
    ClockStyle::Analog => (size, size),
    ClockStyle::Flip => {
      if time.is_empty() {
        return (0.0, size * FLIP_CARD_HEIGHT);
      }
      let card = size * DIGIT_WIDTH + size * FLIP_CARD_PADDING;
      let mut width = 0.0;
      for (index, character) in time.chars().enumerate() {
        if index > 0 {
          width += size * FACE_GAP;
        }
        width += if character == ':' { card * 0.5 } else { card };
      }
      (width, size * FLIP_CARD_HEIGHT)
    }
    ClockStyle::Segment => {
      if time.is_empty() {
        return (0.0, size);
      }
      let mut width = 0.0;
      for (index, character) in time.chars().enumerate() {
        if index > 0 {
          width += size * FACE_GAP;
        }
        width += if character == ':' {
          size * DIGIT_WIDTH * 0.5
        } else {
          size * DIGIT_WIDTH
        };
      }
      (width, size)
    }
  }
}

/// Measured label box. A zero `label_size` derives 0.22 of the face size,
/// which keeps a micro caps label proportional on a small clock. The returned
/// height is zero when the label is empty, so the old geometry is untouched.
fn label_measure(text: &mut TextEngine, spec: &ClockSpec, size: f32, max_width: f32) -> (f32, f32) {
  if spec.label.is_empty() {
    return (0.0, 0.0);
  }
  let label_size = if spec.label_size > 0.0 {
    spec.label_size
  } else {
    size * 0.22
  };
  text.measure_with(
    &spec.label,
    label_size,
    max_width,
    false,
    Weight::NORMAL,
    spec.label_tracking,
  )
}

/// The bounding box the clock will occupy, including its date line. The
/// renderer uses this to paint a contrast scrim before the text lands: a white
/// clock over a bright photo is unreadable otherwise, and the adaptive contrast
/// guard only knows about the glass panel, not an arbitrary wallpaper.
#[must_use]
pub fn bounds(
  text: &mut TextEngine,
  spec: &ClockSpec,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  now: &Zoned,
) -> Option<Rect> {
  let (time, date) = spec.texts(now);
  let size = spec.size * scale;
  let date_size = spec.date_size * scale;
  let screen = (viewport.w, viewport.h);
  match spec.style {
    ClockStyle::Digital => {
      if time.is_empty() {
        return None;
      }
      let (width, time_height) = face_size(text, spec, &time, size);
      let (date_width, date_height) = if date.is_empty() {
        (0.0, 0.0)
      } else {
        text.measure(&date, date_size, screen.0 - 40.0, false)
      };
      let (label_width, label_height) = label_measure(text, spec, size, screen.0 - 40.0);
      let total_width = width.max(date_width).max(label_width);
      let total_height = label_height + time_height + date_height + date_size * 0.15;
      let (x, y) = anchor.place(screen, (total_width, total_height), offset);
      Some(Rect::new(
        viewport.x + x,
        viewport.y + y,
        total_width,
        total_height,
      ))
    }
    ClockStyle::Flip | ClockStyle::Segment => {
      if time.is_empty() {
        return None;
      }
      let (width, time_height) = face_size(text, spec, &time, size);
      let (date_width, date_height) = if date.is_empty() {
        (0.0, 0.0)
      } else {
        text.measure(&date, date_size, screen.0 - 40.0, false)
      };
      let total_width = width.max(date_width);
      let total_height = time_height + date_height + date_size * 0.15;
      let (x, y) = anchor.place(screen, (total_width, total_height), offset);
      Some(Rect::new(
        viewport.x + x,
        viewport.y + y,
        total_width,
        total_height,
      ))
    }
    ClockStyle::Analog => {
      let (date_width, date_height) = if date.is_empty() {
        (0.0, 0.0)
      } else {
        let (width, height) = text.measure(&date, date_size, screen.0, false);
        (width, height + date_size * 0.2)
      };
      let total_width = size.max(date_width);
      let (x, y) = anchor.place(screen, (total_width, size + date_height), offset);
      Some(Rect::new(
        viewport.x + x,
        viewport.y + y,
        total_width,
        size + date_height,
      ))
    }
  }
}

/// Draws a clock widget at its configured anchor. `theme_color` is the resolved
/// primary colour and `date_color` the secondary one.
///
/// `elapsed` is seconds since the widget's motion started, used only for the
/// colon blink. A screenshot passes zero, which leaves the colon in its lit
/// phase. `plate` is the resolved widget plate: the flip face uses it as the
/// card surface, so a themed clock keeps the same material as its backdrop.
#[allow(clippy::too_many_arguments)]
pub fn draw(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  color: Rgb,
  date_color: Rgb,
  now: &Zoned,
  elapsed: f32,
  plate: Option<&WidgetPlate>,
  theme: &Theme,
) {
  let (time, date) = spec.texts(now);
  let size = spec.size * scale;
  let date_size = spec.date_size * scale;
  match spec.style {
    ClockStyle::Digital => draw_digital(
      canvas, text, spec, &time, &date, anchor, offset, viewport, size, date_size, color,
      date_color,
    ),
    ClockStyle::Flip => draw_flip(
      canvas, text, spec, &time, &date, anchor, offset, viewport, size, date_size, color,
      date_color, elapsed, plate, theme, scale,
    ),
    ClockStyle::Segment => draw_segment(
      canvas, text, spec, &time, &date, anchor, offset, viewport, size, date_size, color,
      date_color, elapsed, scale,
    ),
    ClockStyle::Analog => draw_analog(
      canvas, text, spec, &date, anchor, offset, viewport, size, date_size, color, date_color, now,
    ),
  }
}

#[allow(clippy::too_many_arguments)]
pub fn format(pattern: &str, now: &Zoned, posix_locale: bool) -> String {
  let tm = BrokenDownTime::from(now);
  if posix_locale {
    tm.to_string_with_config(&StrftimeConfig::new().custom(PosixCustom::new()), pattern)
      .unwrap_or_default()
  } else {
    tm.to_string(pattern).unwrap_or_default()
  }
}

mod analog;
mod digital;
mod flip;
mod segment;

use analog::*;
use digital::*;
use flip::*;
use segment::*;

#[cfg(test)]
mod tests;
