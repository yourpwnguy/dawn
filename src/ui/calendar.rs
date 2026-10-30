//! The month grid widget.
//!
//! This is a pure view of the local date. It uses the same plate, motion, and
//! state gate as the clock, so a theme can put it in a HUD corner without
//! inventing a second drawing model. `bounds` and `draw` share one geometry
//! helper: if they ever measured different grids, a dragged calendar would jump
//! the moment the pointer grabbed it.

use cosmic_text::Align;
use jiff::Zoned;

use crate::config::{CalendarConfig, WeekStart};
use crate::layout::{Anchor, Placement};
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};

/// The weekday labels in the order the grid draws them.
const MONDAY_FIRST: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const SUNDAY_FIRST: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

/// Resolved geometry for one frame.
///
/// All values are already in physical pixels. Keeping them in one struct means
/// the bounding box and the paint pass cannot disagree about a row height or a
/// column width.
#[derive(Clone, Copy)]
struct Geometry {
  width: f32,
  height: f32,
  pad: f32,
  header_h: f32,
  weekday_h: f32,
  cell_w: f32,
  cell_h: f32,
  rows: usize,
  leading: usize,
  days: usize,
  today: i8,
}

fn geometry(spec: &CalendarConfig, now: &Zoned, scale: f32) -> Geometry {
  let size = spec.size * scale;
  let width = spec.width * scale;
  let pad = size * 0.35;
  let header_h = if spec.show_header { size * 1.8 } else { 0.0 };
  let weekday_h = size * 1.5;
  let cell_h = size * 2.0;
  // The configured width is the widget content width, not a hint. Seven equal
  // columns keep the days aligned even when a theme uses a narrow sidebar.
  let cell_w = ((width - pad * 2.0) / 7.0).max(1.0);
  let date = now.date();
  let first = date.first_of_month();
  let leading = match spec.week_start {
    WeekStart::Monday => first.weekday().to_monday_zero_offset(),
    WeekStart::Sunday => first.weekday().to_sunday_zero_offset(),
  } as usize;
  let days = date.days_in_month() as usize;
  // Four to six rows covers every month without leaving a permanently empty
  // sixth row under a short February.
  let rows = ((leading + days).div_ceil(7)).clamp(4, 6);
  Geometry {
    width,
    height: header_h + weekday_h + cell_h * rows as f32,
    pad,
    header_h,
    weekday_h,
    cell_w,
    cell_h,
    rows,
    leading,
    days,
    today: date.day(),
  }
}

fn place(
  spec: &CalendarConfig,
  now: &Zoned,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
) -> (Geometry, Rect) {
  let geometry = geometry(spec, now, scale);
  let (x, y) = anchor.place(
    (viewport.w, viewport.h),
    (geometry.width, geometry.height),
    offset,
  );
  (
    geometry,
    Rect::new(
      viewport.x + x,
      viewport.y + y,
      geometry.width,
      geometry.height,
    ),
  )
}

/// The rectangle the calendar occupies on screen.
///
/// The renderer uses this for the plate, the hit region, and the drag
/// highlight. It always returns a rectangle: a calendar with an empty month
/// still has a plate and a header.
#[must_use]
pub fn bounds(
  spec: &CalendarConfig,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  now: &Zoned,
) -> Option<Rect> {
  let (_, rect) = place(spec, now, anchor, offset, viewport, scale);
  Some(rect)
}

/// Paints the month grid at its configured anchor.
///
/// The header is the full month name and year in `header_color`. Weekday
/// labels use `muted_color`, ordinary days use `color`, and the current day
/// gets a filled rounded cell behind it. Leading and trailing blanks are simply
/// skipped, which is what keeps the first row aligned to the correct weekday.
pub fn draw(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &CalendarConfig,
  placement: &Placement,
  viewport: Rect,
  scale: f32,
  now: &Zoned,
) {
  let (geometry, rect) = place(
    spec,
    now,
    placement.anchor,
    placement.offset,
    viewport,
    scale,
  );
  let size = spec.size * scale;
  let mut y = rect.y;

  if spec.show_header {
    let header = crate::ui::clock::format("%B %Y", now, false).to_uppercase();
    let header_size = size * 0.95;
    let (_, height) = text.measure(&header, header_size, rect.w, false);
    text.draw(
      canvas,
      &header,
      Style::new(header_size, spec.header_color),
      Area::new(rect.x, y + (geometry.header_h - height) / 2.0, rect.w).align(Align::Center),
    );
    y += geometry.header_h;
  }

  let labels = match spec.week_start {
    WeekStart::Monday => MONDAY_FIRST,
    WeekStart::Sunday => SUNDAY_FIRST,
  };
  let label_size = (size * 0.55).max(1.0);
  let label_height = text.measure("MO", label_size, geometry.cell_w, false).1;
  for (column, label) in labels.iter().enumerate() {
    text.draw(
      canvas,
      label,
      Style::new(label_size, spec.muted_color),
      Area::new(
        rect.x + geometry.pad + column as f32 * geometry.cell_w,
        y + (geometry.weekday_h - label_height) / 2.0,
        geometry.cell_w,
      )
      .align(Align::Center),
    );
  }
  y += geometry.weekday_h;

  for row in 0..geometry.rows {
    for column in 0..7 {
      let index = row * 7 + column;
      let Some(day) = index
        .checked_sub(geometry.leading)
        .and_then(|day| (day < geometry.days).then_some(day + 1))
      else {
        continue;
      };
      let cell = Rect::new(
        rect.x + geometry.pad + column as f32 * geometry.cell_w,
        y + row as f32 * geometry.cell_h,
        geometry.cell_w,
        geometry.cell_h,
      );
      let today = day as i8 == geometry.today;
      let ink = if today {
        let inset = (geometry.cell_h * 0.12).min(cell.w * 0.12);
        let fill = cell.inset(inset);
        canvas.fill(
          fill,
          spec.today_color,
          (geometry.cell_h * 0.22).min(fill.w * 0.3),
        );
        // Pick black or white against the fill instead of assuming the theme
        // ink will stay readable on an arbitrary accent.
        spec.today_color.ensure_contrast(spec.today_color, 4.5)
      } else {
        spec.color
      };
      let label = day.to_string();
      let (_, height) = text.measure(&label, size, cell.w, false);
      text.draw(
        canvas,
        &label,
        Style::new(size, ink),
        Area::new(cell.x, cell.y + (cell.h - height) / 2.0, cell.w).align(Align::Center),
      );
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{bounds, geometry};
  use crate::config::{CalendarConfig, WeekStart};
  use crate::layout::Anchor;
  use crate::ui::canvas::Rect;

  fn spec() -> CalendarConfig {
    crate::config::Config::default().calendar
  }

  fn month(month: i8) -> jiff::Zoned {
    jiff::civil::date(2026, month, 15)
      .at(12, 0, 0, 0)
      .in_tz("UTC")
      .unwrap()
  }

  #[test]
  fn grid_starts_on_the_configured_weekday() {
    // 2026-10-01 is a Thursday, so a Monday-first grid needs three blanks.
    let config = spec();
    let grid = geometry(&config, &month(10), 1.0);
    assert_eq!(grid.leading, 3);
    assert_eq!(grid.days, 31);
    assert_eq!(grid.rows, 5);

    let sunday = CalendarConfig {
      week_start: WeekStart::Sunday,
      ..spec()
    };
    assert_eq!(geometry(&sunday, &month(10), 1.0).leading, 4);
  }

  #[test]
  fn bounds_match_the_configured_width_and_scale() {
    let config = spec();
    let rect = bounds(
      &config,
      Anchor::TopLeft,
      (0, 0),
      Rect::new(0.0, 0.0, 1280.0, 720.0),
      2.0,
      &month(10),
    )
    .unwrap();
    assert_eq!(rect.w, 520.0);
    assert!(rect.h > 0.0);
  }

  #[test]
  fn a_short_month_does_not_grow_to_six_rows() {
    // February 2026 starts on Sunday and has 28 days. Sunday-first is the
    // worst case at exactly four rows.
    let sunday = CalendarConfig {
      week_start: WeekStart::Sunday,
      ..spec()
    };
    assert_eq!(geometry(&sunday, &month(2), 1.0).rows, 4);
  }
}
