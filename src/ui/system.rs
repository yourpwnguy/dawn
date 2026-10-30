//! The local system monitor.
//!
//! The widget is a formatted view of [`crate::sysinfo::SystemSnapshot`]. The
//! rows style is the calm default: one uppercase label and one readable value
//! per line. The sparkline style adds a small load history above those rows
//! without needing a graph library or a second data source.

use cosmic_text::Align;

use crate::config::{SystemConfig, SystemField, SystemStyle};
use crate::layout::{Anchor, Placement};
use crate::sysinfo::{SystemSnapshot, format_memory, format_uptime};
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};

fn sparkline_height(spec: &SystemConfig, scale: f32) -> f32 {
  if spec.style == SystemStyle::Sparkline {
    spec.size * scale * 2.4
  } else {
    0.0
  }
}

fn content_size(spec: &SystemConfig, scale: f32) -> (f32, f32) {
  let size = spec.size * scale;
  let rows = spec.fields.len() as f32 * size * 1.9;
  let height = (sparkline_height(spec, scale) + rows).max(size * 1.3);
  (spec.width * scale, height)
}

fn place(
  spec: &SystemConfig,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
) -> Rect {
  let (width, height) = content_size(spec, scale);
  let (x, y) = anchor.place((viewport.w, viewport.h), (width, height), offset);
  Rect::new(viewport.x + x, viewport.y + y, width, height)
}

/// The rectangle the system widget occupies on screen.
///
/// The sparkline style reserves extra vertical room, so the renderer asks here
/// rather than assuming a fixed row count.
#[must_use]
pub fn bounds(
  spec: &SystemConfig,
  anchor: Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
) -> Option<Rect> {
  Some(place(spec, anchor, offset, viewport, scale))
}

/// Uppercase label for one reading.
fn field_label(field: SystemField) -> &'static str {
  match field {
    SystemField::Hostname => "HOSTNAME",
    SystemField::Kernel => "KERNEL",
    SystemField::Uptime => "UPTIME",
    SystemField::Load => "LOAD",
    SystemField::Memory => "MEMORY",
  }
}

/// The display value for one reading.
///
/// Missing files leave the snapshot empty or zero, and every branch prints
/// something sensible for that state. That matters on a container or a broken
/// `/proc` mount: the widget must not panic and must not invent data.
fn field_value(field: SystemField, system: &SystemSnapshot) -> String {
  match field {
    SystemField::Hostname => system.hostname.clone(),
    SystemField::Kernel => system.kernel.clone(),
    SystemField::Uptime => format_uptime(system.uptime),
    SystemField::Load => format!(
      "{:.2} {:.2} {:.2}",
      system.load[0], system.load[1], system.load[2]
    ),
    SystemField::Memory => format_memory(system.mem_used, system.mem_total),
  }
}

/// Draws the load history as a simple polyline.
///
/// The maximum is taken from the visible window rather than a fixed scale, so a
/// quiet machine still shows shape instead of a flat line at the bottom. A
/// single sample, or no samples at all, only draws the baseline.
fn draw_sparkline(
  canvas: &mut Canvas,
  spec: &SystemConfig,
  system: &SystemSnapshot,
  rect: Rect,
  scale: f32,
) {
  let baseline = rect.bottom();
  canvas.line(
    (rect.x, baseline),
    (rect.right(), baseline),
    spec.label_color.with_alpha(70),
    1.0 * scale,
  );
  let history = &system.load_history;
  if history.len() < 2 {
    return;
  }
  let peak = history.iter().copied().fold(0.0f32, f32::max).max(0.1);
  let step = rect.w / (history.len() - 1) as f32;
  let mut previous: Option<(f32, f32)> = None;
  for (index, value) in history.iter().enumerate() {
    let x = rect.x + index as f32 * step;
    let ratio = (value / peak).clamp(0.0, 1.0);
    let y = rect.bottom() - ratio * rect.h * 0.9 - rect.h * 0.05;
    if let Some(from) = previous {
      canvas.line(from, (x, y), spec.color, (1.5 * scale).max(1.0));
    }
    previous = Some((x, y));
  }
}

/// Paints the system monitor at its configured anchor.
///
/// Labels are uppercase and drawn in `label_color`; values use `color`. The
/// sparkline sits above the rows so the eye reads the trend first and the exact
/// numbers second.
pub fn draw(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  spec: &SystemConfig,
  system: &SystemSnapshot,
  placement: &Placement,
  viewport: Rect,
  scale: f32,
) {
  let rect = place(spec, placement.anchor, placement.offset, viewport, scale);
  let size = spec.size * scale;
  let mut y = rect.y;
  if spec.style == SystemStyle::Sparkline {
    draw_sparkline(
      canvas,
      spec,
      system,
      Rect::new(rect.x, y, rect.w, sparkline_height(spec, scale)),
      scale,
    );
    y += sparkline_height(spec, scale);
  }

  let label_size = (size * 0.85).max(1.0);
  let gap = size * 0.8;
  let label_width = spec
    .fields
    .iter()
    .map(|field| {
      text
        .measure(field_label(*field), label_size, rect.w, false)
        .0
    })
    .fold(0.0f32, f32::max);
  let value_x = rect.x + label_width + gap;
  let value_width = (rect.right() - value_x).max(1.0);
  let row_height = size * 1.9;

  for (index, field) in spec.fields.iter().enumerate() {
    let row_y = y + index as f32 * row_height;
    let (_, label_height) = text.measure(field_label(*field), label_size, rect.w, false);
    let (_, value_height) = text.measure("Ag", size, value_width, false);
    text.draw(
      canvas,
      field_label(*field),
      Style::new(label_size, spec.label_color),
      Area::new(
        rect.x,
        row_y + (row_height - label_height) / 2.0,
        label_width.max(1.0),
      ),
    );
    text.draw(
      canvas,
      &field_value(*field, system),
      Style::new(size, spec.color),
      Area::new(
        value_x,
        row_y + (row_height - value_height) / 2.0,
        value_width,
      )
      .align(Align::Left),
    );
  }
}

#[cfg(test)]
mod tests {
  use super::{bounds, field_label, field_value};
  use crate::config::{Config, SystemField, SystemStyle};
  use crate::layout::Anchor;
  use crate::sysinfo::SystemSnapshot;
  use crate::ui::canvas::Rect;
  use std::time::Duration;

  fn spec() -> crate::config::SystemConfig {
    Config::default().system
  }

  #[test]
  fn field_labels_are_uppercase_and_values_are_never_empty() {
    for field in [
      SystemField::Hostname,
      SystemField::Kernel,
      SystemField::Uptime,
      SystemField::Load,
      SystemField::Memory,
    ] {
      assert_eq!(field_label(field), field_label(field).to_uppercase());
    }
    let system = SystemSnapshot {
      hostname: "dawn".to_string(),
      kernel: "6.12".to_string(),
      uptime: Duration::from_secs(3_600),
      load: [0.5, 0.25, 0.1],
      mem_used: 1024 * 1024,
      mem_total: 2 * 1024 * 1024,
      ..SystemSnapshot::default()
    };
    assert_eq!(field_value(SystemField::Hostname, &system), "dawn");
    assert_eq!(field_value(SystemField::Uptime, &system), "01h 00m");
    assert!(field_value(SystemField::Load, &system).contains("0.50"));
    assert_eq!(field_value(SystemField::Memory, &system), "1.0 / 2.0 GiB");
  }

  #[test]
  fn an_empty_snapshot_still_formats_cleanly() {
    let empty = SystemSnapshot::default();
    assert_eq!(field_value(SystemField::Hostname, &empty), "");
    assert_eq!(field_value(SystemField::Uptime, &empty), "0m");
    assert_eq!(field_value(SystemField::Memory, &empty), "0.0 / 0.0 GiB");
    assert_eq!(field_value(SystemField::Load, &empty), "0.00 0.00 0.00");
  }

  #[test]
  fn the_sparkline_style_reserves_extra_height() {
    let rows = spec();
    let sparkline = crate::config::SystemConfig {
      style: SystemStyle::Sparkline,
      ..spec()
    };
    let viewport = Rect::new(0.0, 0.0, 1280.0, 720.0);
    let rows_rect = bounds(&rows, Anchor::TopLeft, (0, 0), viewport, 1.0).unwrap();
    let spark_rect = bounds(&sparkline, Anchor::TopLeft, (0, 0), viewport, 1.0).unwrap();
    assert_eq!(rows_rect.w, rows.width);
    assert!(spark_rect.h > rows_rect.h);
  }
}
