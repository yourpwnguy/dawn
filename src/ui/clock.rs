//! Clock formatting and drawing for the built-in and plugin clock widgets.
//!
//! The clock is a first-class widget (the built-in plugin and a third-party
//! manifest both produce the same [`ClockSpec`]). It supports digital and
//! analog faces, 12/24-hour time, optional seconds and date, and an optional
//! IANA time zone. Redraws are driven by [`ClockSpec::signature`], so a
//! minute-resolution clock does not repaint sixty times a minute.

use std::f32::consts::TAU;

use cosmic_text::Align;
use jiff::Zoned;
use jiff::fmt::strtime::{BrokenDownTime, Config as StrftimeConfig, PosixCustom};
use jiff::tz::TimeZone;

use crate::color::Rgb;
use crate::config::{ClockConfig, ClockStyle};
use crate::layout::Anchor;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};

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
      ClockStyle::Digital => format(&self.effective_format(), &local, self.posix_locale),
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
      ClockStyle::Digital => {
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
      let (width, time_height) = text.measure(&time, size, screen.0 - 40.0, false);
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
) {
  let (time, date) = spec.texts(now);
  let size = spec.size * scale;
  let date_size = spec.date_size * scale;
  match spec.style {
    ClockStyle::Digital => draw_digital(
      canvas, text, &time, &date, anchor, offset, viewport, size, date_size, color, date_color,
    ),
    ClockStyle::Analog => draw_analog(
      canvas, text, spec, &date, anchor, offset, viewport, size, date_size, color, date_color, now,
    ),
  }
}

#[allow(clippy::too_many_arguments)]
fn draw_digital(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  time: &str,
  date: &str,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  size: f32,
  date_size: f32,
  color: Rgb,
  date_color: Rgb,
) {
  if time.is_empty() {
    return;
  }
  let screen = (viewport.w, viewport.h);
  let (width, time_height) = text.measure(time, size, screen.0 - 40.0, false);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    text.measure(date, date_size, screen.0 - 40.0, false)
  };
  // Both lines share the widget's full width. Using the time width for the
  // date was a real bug: a long weekday name wrapped to two lines under a
  // short time, which looked like the clock had fallen apart.
  let total_width = width.max(date_width);
  let total = (total_width, time_height + date_height + date_size * 0.15);
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);

  text.draw(
    canvas,
    time,
    Style::new(size, color),
    Area::new(x, y, total_width.max(1.0)).align(Align::Center),
  );
  if !date.is_empty() {
    text.draw(
      canvas,
      date,
      Style::new(date_size, date_color),
      Area::new(x, y + time_height + date_size * 0.15, total_width.max(1.0)).align(Align::Center),
    );
  }
}

#[allow(clippy::too_many_arguments)]
fn draw_analog(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
  date: &str,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  diameter: f32,
  date_size: f32,
  color: Rgb,
  date_color: Rgb,
  now: &Zoned,
) {
  let local = spec.local(now);
  let screen = (viewport.w, viewport.h);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    let (width, height) = text.measure(date, date_size, screen.0, false);
    (width, height + date_size * 0.2)
  };
  // The date can be wider than the dial. Sizing the widget to the wider of the
  // two keeps the date on one line instead of wrapping it into a narrow column
  // under a small clock.
  let total_width = diameter.max(date_width);
  let total = (total_width, diameter + date_height);
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);
  let radius = diameter / 2.0;
  let center = (x + total_width / 2.0, y + radius);
  let rim = color.with_alpha(70);
  let face = color.with_alpha(30);
  let tick = color.with_alpha(120);

  canvas.fill_circle(center, radius, face);
  canvas.stroke_circle(center, radius, rim, (diameter * 0.008).max(1.0));

  for index in 0..60u32 {
    let angle = index as f32 * TAU / 60.0 - TAU / 4.0;
    let (inner, width) = if index % 5 == 0 {
      (radius * 0.82, (diameter * 0.012).max(1.5))
    } else {
      (radius * 0.90, (diameter * 0.005).max(1.0))
    };
    let outer = radius * 0.96;
    canvas.line(
      (
        center.0 + angle.cos() * inner,
        center.1 + angle.sin() * inner,
      ),
      (
        center.0 + angle.cos() * outer,
        center.1 + angle.sin() * outer,
      ),
      tick,
      width,
    );
  }

  let hour = f32::from(local.hour() % 12) + f32::from(local.minute()) / 60.0;
  let minute = f32::from(local.minute()) + f32::from(local.second()) / 60.0;
  let second = f32::from(local.second()) + local.subsec_nanosecond() as f32 / 1_000_000_000.0;
  draw_hand(
    canvas,
    center,
    hour * TAU / 12.0 - TAU / 4.0,
    radius * 0.50,
    color,
    (diameter * 0.022).max(2.0),
  );
  draw_hand(
    canvas,
    center,
    minute * TAU / 60.0 - TAU / 4.0,
    radius * 0.72,
    color,
    (diameter * 0.014).max(1.5),
  );
  if spec.seconds {
    draw_hand(
      canvas,
      center,
      second * TAU / 60.0 - TAU / 4.0,
      radius * 0.82,
      color.with_alpha(200),
      (diameter * 0.006).max(1.0),
    );
  }
  canvas.fill_circle(center, (diameter * 0.018).max(2.0), color);

  if !date.is_empty() {
    text.draw(
      canvas,
      date,
      Style::new(date_size, date_color),
      Area::new(x, y + diameter + date_size * 0.2, total_width).align(Align::Center),
    );
  }
}

fn draw_hand(
  canvas: &mut Canvas,
  center: (f32, f32),
  angle: f32,
  length: f32,
  color: Rgb,
  width: f32,
) {
  canvas.line(
    center,
    (
      center.0 + angle.cos() * length,
      center.1 + angle.sin() * length,
    ),
    color,
    width,
  );
}

/// Formats one strftime-style pattern. Config validation catches bad patterns,
/// but a timezone edge case should still not panic the greeter, so failures
/// degrade to an empty string.
#[must_use]
pub fn format(pattern: &str, now: &Zoned, posix_locale: bool) -> String {
  let tm = BrokenDownTime::from(now);
  if posix_locale {
    tm.to_string_with_config(&StrftimeConfig::new().custom(PosixCustom::new()), pattern)
      .unwrap_or_default()
  } else {
    tm.to_string(pattern).unwrap_or_default()
  }
}

#[cfg(test)]
mod tests {
  use super::{ClockSpec, format};
  use crate::config::{ClockConfig, ClockStyle, WidgetLayer};
  use crate::layout::{Anchor, Placement};

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
      draggable: true,
    };
    let spec = ClockSpec::from_config(&config);
    assert_eq!(spec.style, ClockStyle::Analog);
    assert!(spec.timezone.is_some());
  }
}
