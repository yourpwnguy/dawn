//! The dev studio's corner HUD.
//!
//! This is a tiny, self-contained overlay rather than a widget: it is only
//! constructed by `dawn dev`, and `Scene::hud` is `None` everywhere else. That
//! is what guarantees the HUD can never leak into a normal run or a
//! `--screenshot` image.
//!
//! The HUD is drawn last on the existing overlay canvas, after the form and the
//! front particles, so it is always readable. It records no hit regions: the
//! keys it advertises (F5, F1, Ctrl+Q) are handled by the window, not by a
//! clickable control.

use crate::color::Rgb;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{Area, Style, TextEngine};

/// The error line is crimson regardless of the theme. A reload failure is a
/// developer signal, not part of the login palette.
const CRIMSON: Rgb = Rgb::solid(220, 20, 60);
const INK: Rgb = Rgb::solid(235, 235, 235);
const PLATE: Rgb = Rgb::rgba(8, 8, 10, 220);
const SOURCE_INK: Rgb = Rgb::solid(180, 180, 188);
const HINT_INK: Rgb = Rgb::solid(150, 150, 158);

/// One frame's HUD values. The window rebuilds this from its dev state.
#[derive(Debug, Clone, PartialEq)]
pub struct Hud {
  pub visible: bool,
  /// A config path or a bundle id.
  pub source: String,
  /// Parse and swap cost of the last successful reload.
  pub last_ms: Option<f32>,
  /// Current frame-rate estimate.
  pub fps: Option<f32>,
  /// First line of the last failed reload, if one is still current.
  pub error: Option<String>,
}

impl Hud {
  /// The exact lines the HUD paints. Returning an empty vector when hidden is
  /// what makes `--no-hud` and F1 testable without a canvas.
  #[must_use]
  pub fn lines(&self) -> Vec<String> {
    if !self.visible {
      return Vec::new();
    }
    let reload = self.last_ms.map_or_else(
      || "reload --".to_string(),
      |millis| format!("reload {millis:.1} ms"),
    );
    let fps = self
      .fps
      .map_or_else(|| "-- fps".to_string(), |fps| format!("{fps:.0} fps"));
    let mut lines = vec![
      "DEV".to_string(),
      self.source.clone(),
      format!("{reload}  {fps}"),
      "F5 reload · F1 hud · Ctrl+Q quit".to_string(),
    ];
    if let Some(error) = &self.error {
      let first = error.lines().next().unwrap_or_default();
      lines.push(format!("reload failed: {first}"));
    }
    lines
  }
}

/// Paints the HUD in the top-left corner.
pub fn draw(canvas: &mut Canvas, text: &mut TextEngine, hud: &Hud, screen: (f32, f32), scale: f32) {
  let lines = hud.lines();
  if lines.is_empty() {
    return;
  }
  let scale = if scale.is_finite() && scale > 0.25 {
    scale
  } else {
    1.0
  };
  let size = 12.0 * scale;
  let line_height = 18.0 * scale;
  let padding = 10.0 * scale;
  let margin = 16.0 * scale;

  let mut widest: f32 = 0.0;
  for line in &lines {
    let (width, _) = text.measure(line, size, screen.0.max(1.0), false);
    widest = widest.max(width);
  }
  let max_width = (screen.0 - margin * 2.0).max(1.0);
  let plate_width = (widest + padding * 2.0).min(max_width);
  let plate_height = lines.len() as f32 * line_height + padding * 2.0;
  let plate = Rect::new(margin, margin, plate_width, plate_height);
  canvas.fill(plate, PLATE, 6.0 * scale);

  let mut y = plate.y + padding;
  let last = lines.len().saturating_sub(1);
  for (index, line) in lines.iter().enumerate() {
    let color = if hud.error.is_some() && index == last {
      CRIMSON
    } else if index == 0 {
      INK
    } else if index == 1 {
      SOURCE_INK
    } else {
      HINT_INK
    };
    text.draw_line(
      canvas,
      line,
      Style::new(size, color),
      Area::new(plate.x + padding, y, plate_width - padding * 2.0).clip(plate),
    );
    y += line_height;
  }
}

#[cfg(test)]
mod tests {
  use super::Hud;

  fn hud() -> Hud {
    Hud {
      visible: true,
      source: "kinich".to_string(),
      last_ms: Some(3.4),
      fps: Some(60.0),
      error: None,
    }
  }

  #[test]
  fn the_hud_has_exact_dev_lines() {
    assert_eq!(
      hud().lines(),
      vec![
        "DEV".to_string(),
        "kinich".to_string(),
        "reload 3.4 ms  60 fps".to_string(),
        "F5 reload · F1 hud · Ctrl+Q quit".to_string(),
      ]
    );
  }

  #[test]
  fn a_reload_failure_is_crimson_and_shows_the_first_line() {
    let mut hud = hud();
    hud.error = Some("theme.accent: invalid colour\nsecond line".to_string());
    let lines = hud.lines();
    assert_eq!(
      lines.last().map(String::as_str),
      Some("reload failed: theme.accent: invalid colour")
    );
  }

  #[test]
  fn a_hidden_hud_paints_nothing() {
    let mut hud = hud();
    hud.visible = false;
    assert!(hud.lines().is_empty());
  }
}
