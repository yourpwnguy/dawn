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
fn draw_digital(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
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
  let (label_width, label_height) = label_measure(text, spec, size, screen.0 - 40.0);
  // Both lines share the widget's full width. Using the time width for the
  // date was a real bug: a long weekday name wrapped to two lines under a
  // short time, which looked like the clock had fallen apart.
  let total_width = width.max(date_width).max(label_width);
  let total = (
    total_width,
    label_height + time_height + date_height + date_size * 0.15,
  );
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);

  if !spec.label.is_empty() {
    let label_size = if spec.label_size > 0.0 {
      spec.label_size
    } else {
      size * 0.22
    };
    let label_color = spec.label_color.unwrap_or(color);
    text.draw(
      canvas,
      &spec.label,
      Style::new(label_size, label_color).tracking(spec.label_tracking),
      Area::new(x, y, total_width.max(1.0)).align(Align::Center),
    );
  }
  text.draw(
    canvas,
    time,
    Style::new(size, color),
    Area::new(x, y + label_height, total_width.max(1.0)).align(Align::Center),
  );
  if !date.is_empty() {
    text.draw(
      canvas,
      date,
      Style::new(date_size, date_color),
      Area::new(
        x,
        y + label_height + time_height + date_size * 0.15,
        total_width.max(1.0),
      )
      .align(Align::Center),
    );
  }
}

/// Whether the colon should be lit at `elapsed` seconds.
///
/// `colon_blink_ms` is a full on/off period: the first half is lit and the
/// second half is dark. Zero (the default) means "steady", so an unconfigured
/// clock never blinks.
fn colon_on(spec: &ClockSpec, elapsed: f32) -> bool {
  if spec.colon_blink_ms <= 0.0 {
    return true;
  }
  let phase = (elapsed * 1000.0 / spec.colon_blink_ms).rem_euclid(2.0);
  phase < 1.0
}

/// Draws the date line under a text face. Flip and segment share it so their
/// date placement cannot drift from each other.
#[allow(clippy::too_many_arguments)]
fn draw_date(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  date: &str,
  x: f32,
  y: f32,
  width: f32,
  date_size: f32,
  date_color: Rgb,
) {
  if date.is_empty() {
    return;
  }
  text.draw(
    canvas,
    date,
    Style::new(date_size, date_color),
    Area::new(x, y, width.max(1.0)).align(Align::Center),
  );
}

/// Split-flap face: one rounded card per character, with a seam across the
/// middle and the glyph sitting on top. The colon is two dots on a half-width
/// slot, and it honours the configured blink.
#[allow(clippy::too_many_arguments)]
fn draw_flip(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
  time: &str,
  date: &str,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  size: f32,
  date_size: f32,
  color: Rgb,
  date_color: Rgb,
  elapsed: f32,
  plate: Option<&WidgetPlate>,
  theme: &Theme,
  scale: f32,
) {
  if time.is_empty() {
    return;
  }
  let screen = (viewport.w, viewport.h);
  let (face_width, face_height) = face_size(text, spec, time, size);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    text.measure(date, date_size, screen.0 - 40.0, false)
  };
  let total_width = face_width.max(date_width);
  let total = (total_width, face_height + date_height + date_size * 0.15);
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);

  let card_width = size * DIGIT_WIDTH + size * FLIP_CARD_PADDING;
  let gap = size * FACE_GAP;
  let radius = plate
    .map_or(1.0, |plate| (plate.radius * scale).max(1.0))
    .min(card_width * 0.25)
    .min(face_height * 0.25);
  let blink = colon_on(spec, elapsed);
  let mut cursor = x + (total_width - face_width) / 2.0;
  for character in time.chars() {
    let width = if character == ':' {
      card_width * 0.5
    } else {
      card_width
    };
    if character == ':' {
      if blink {
        let dot = (size * 0.075).max(1.0);
        let center_x = cursor + width / 2.0;
        canvas.fill_circle((center_x, y + face_height * 0.34), dot, color);
        canvas.fill_circle((center_x, y + face_height * 0.66), dot, color);
      }
    } else {
      draw_flip_card(
        canvas,
        text,
        Rect::new(cursor, y, width, face_height),
        character,
        color,
        plate,
        theme,
        radius,
      );
    }
    cursor += width + gap;
  }

  draw_date(
    canvas,
    text,
    date,
    x,
    y + face_height + date_size * 0.15,
    total_width,
    date_size,
    date_color,
  );
}

/// Paints one flip card. The plate supplies the material, but the card is
/// smaller than the widget plate, so a `Tint` plate becomes a denser glass card
/// instead of the usual scrim. A `None` plate draws no surface at all, which
/// lets a theme put the cards straight on the wallpaper.
#[allow(clippy::too_many_arguments)]
fn draw_flip_card(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  rect: Rect,
  glyph: char,
  color: Rgb,
  plate: Option<&WidgetPlate>,
  theme: &Theme,
  radius: f32,
) {
  let style = plate.map_or(PlateStyle::Tint, |plate| plate.style);
  if style != PlateStyle::None {
    match style {
      PlateStyle::Tint => canvas.fill(rect, theme.tint.with_alpha(230), radius),
      PlateStyle::Solid => canvas.fill(
        rect,
        plate.and_then(|plate| plate.color).unwrap_or(theme.tint),
        radius,
      ),
      PlateStyle::Gradient => {
        let start = plate.and_then(|plate| plate.color).unwrap_or(theme.tint);
        let end = plate
          .and_then(|plate| plate.color_end)
          .unwrap_or(Rgb::rgba(0, 0, 0, 0));
        let angle = plate.map_or(0.0, |plate| plate.gradient_angle);
        canvas.linear_gradient_stops(rect, radius, angle, &[(0.0, start), (1.0, end)]);
      }
      // The widget plate already painted the shared backdrop underneath the
      // flip cards. Each card still needs its own denser surface to look like a
      // hinged flap, so Glass behaves like Tint here and honours a configured
      // colour when the author set one.
      PlateStyle::Glass => canvas.fill(
        rect,
        plate
          .and_then(|plate| plate.color)
          .unwrap_or(theme.tint.with_alpha(230)),
        radius,
      ),
      PlateStyle::None => {}
    }
    let border = plate
      .and_then(|plate| plate.border)
      .unwrap_or(theme.border.with_alpha(70));
    canvas.stroke(rect, border, radius, 1.0);
    // A faint highlight across the top half sells the hinged-flap look without
    // needing a second image or a real 3D transform.
    if rect.h > 4.0 {
      let highlight = Rect::new(
        rect.x + 1.0,
        rect.y + 1.0,
        (rect.w - 2.0).max(0.0),
        rect.h * 0.42,
      );
      canvas.fill(highlight, Rgb::rgba(255, 255, 255, 12), radius * 0.8);
    }
    canvas.line(
      (rect.x + 2.0, rect.center().1),
      (rect.right() - 2.0, rect.center().1),
      theme.border.with_alpha(90),
      1.0,
    );
  }

  let glyph_size = rect.h * 0.78;
  let label = glyph.to_string();
  let (_, glyph_height) = text.measure(&label, glyph_size, rect.w, false);
  text.draw(
    canvas,
    &label,
    Style::new(glyph_size, color),
    Area::new(rect.x, rect.y + (rect.h - glyph_height) / 2.0, rect.w).align(Align::Center),
  );
}

/// Seven-segment face. Ghost segments are drawn first at low alpha, then the
/// glow pass for the lit segments, then the solid pass on top. The order is
/// what keeps the halo behind the crisp stroke instead of washing it out.
#[allow(clippy::too_many_arguments)]
fn draw_segment(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &ClockSpec,
  time: &str,
  date: &str,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  size: f32,
  date_size: f32,
  color: Rgb,
  date_color: Rgb,
  elapsed: f32,
  scale: f32,
) {
  if time.is_empty() {
    return;
  }
  let screen = (viewport.w, viewport.h);
  let (face_width, face_height) = face_size(text, spec, time, size);
  let (date_width, date_height) = if date.is_empty() {
    (0.0, 0.0)
  } else {
    text.measure(date, date_size, screen.0 - 40.0, false)
  };
  let total_width = face_width.max(date_width);
  let total = (total_width, face_height + date_height + date_size * 0.15);
  let (x, y) = anchor.place(screen, total, offset);
  let (x, y) = (x + viewport.x, y + viewport.y);

  let digit_width = size * DIGIT_WIDTH;
  let gap = size * FACE_GAP;
  let blink = colon_on(spec, elapsed);
  let mut cursor = x + (total_width - face_width) / 2.0;
  for character in time.chars() {
    if character == ':' {
      if blink {
        let dot = (size * 0.07).max(1.0);
        let center_x = cursor + digit_width * 0.25;
        canvas.fill_circle((center_x, y + face_height * 0.34), dot, color);
        canvas.fill_circle((center_x, y + face_height * 0.66), dot, color);
      }
      cursor += digit_width * 0.5 + gap;
      continue;
    }
    let rect = Rect::new(cursor, y, digit_width, face_height);
    match segment_mask(character) {
      Some(mask) => draw_segment_digit(canvas, rect, mask, spec, color, scale),
      // A format can include AM/PM or a literal letter. Those have no segment
      // geometry, so print them as ordinary text instead of dropping them.
      None => draw_segment_letter(canvas, text, rect, character, color),
    }
    cursor += digit_width + gap;
  }

  draw_date(
    canvas,
    text,
    date,
    x,
    y + face_height + date_size * 0.15,
    total_width,
    date_size,
    date_color,
  );
}

fn draw_segment_letter(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  rect: Rect,
  character: char,
  color: Rgb,
) {
  let glyph_size = rect.h * 0.8;
  let label = character.to_string();
  let (_, glyph_height) = text.measure(&label, glyph_size, rect.w, false);
  text.draw(
    canvas,
    &label,
    Style::new(glyph_size, color),
    Area::new(rect.x, rect.y + (rect.h - glyph_height) / 2.0, rect.w).align(Align::Center),
  );
}

fn draw_segment_digit(
  canvas: &mut Canvas,
  rect: Rect,
  mask: u8,
  spec: &ClockSpec,
  color: Rgb,
  scale: f32,
) {
  let thickness = (rect.h * 0.12).max(1.5);
  if spec.ghost {
    for segment in 0..7 {
      if mask & (1 << segment) == 0 {
        draw_segment_line(canvas, rect, segment, color.with_alpha(30), thickness);
      }
    }
  }
  if let Some(glow) = spec.glow
    && spec.glow_size > 0.0
  {
    let glow_width = thickness + spec.glow_size * scale;
    for segment in 0..7 {
      if mask & (1 << segment) != 0 {
        draw_segment_line(canvas, rect, segment, glow.with_alpha(60), glow_width);
      }
    }
  }
  for segment in 0..7 {
    if mask & (1 << segment) != 0 {
      draw_segment_line(canvas, rect, segment, color, thickness);
    }
  }
}

/// One of the seven strokes. The small gaps at the joints keep adjacent
/// segments from merging into a blob at larger sizes, which is exactly how a
/// real seven-segment display looks.
fn draw_segment_line(canvas: &mut Canvas, rect: Rect, segment: usize, color: Rgb, width: f32) {
  let thickness = (rect.h * 0.12).max(1.0);
  let inset = thickness * 0.6;
  let x0 = rect.x + inset;
  let x1 = rect.right() - inset;
  let y0 = rect.y + thickness * 0.5;
  let middle = rect.y + rect.h * 0.5;
  let y1 = rect.bottom() - thickness * 0.5;
  let gap = thickness * 0.35;
  let (from, to) = match segment {
    0 => ((x0, y0), (x1, y0)),
    1 => ((x1, y0 + gap), (x1, middle - gap)),
    2 => ((x1, middle + gap), (x1, y1 - gap)),
    3 => ((x0, y1), (x1, y1)),
    4 => ((x0, middle + gap), (x0, y1 - gap)),
    5 => ((x0, y0 + gap), (x0, middle - gap)),
    _ => ((x0, middle), (x1, middle)),
  };
  canvas.line(from, to, color, width);
}

/// Active-segment bitmask for the digits 0 to 9. Bit 0 is A (top), then B
/// (upper right), C (lower right), D (bottom), E (lower left), F (upper left),
/// and bit 6 is G (middle). Anything else has no seven-segment form.
fn segment_mask(character: char) -> Option<u8> {
  const A: u8 = 1 << 0;
  const B: u8 = 1 << 1;
  const C: u8 = 1 << 2;
  const D: u8 = 1 << 3;
  const E: u8 = 1 << 4;
  const F: u8 = 1 << 5;
  const G: u8 = 1 << 6;
  match character {
    '0' => Some(A | B | C | D | E | F),
    '1' => Some(B | C),
    '2' => Some(A | B | G | E | D),
    '3' => Some(A | B | G | C | D),
    '4' => Some(F | G | B | C),
    '5' => Some(A | F | G | C | D),
    '6' => Some(A | F | G | E | C | D),
    '7' => Some(A | B | C),
    '8' => Some(A | B | C | D | E | F | G),
    '9' => Some(A | B | C | D | F | G),
    _ => None,
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
  // A ticking hand jumps to whole seconds; a sweeping hand follows the
  // subsecond clock so it glides around the dial. The minute and hour hands are
  // always smooth, which is what makes a sweep second hand look correct.
  let second = match spec.second_hand {
    SecondHand::Tick => f32::from(local.second()),
    SecondHand::Sweep => {
      f32::from(local.second()) + local.subsec_nanosecond() as f32 / 1_000_000_000.0
    }
  };
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
}
