//! Shared layout vocabulary for config, plugins, and the renderer.
//!
//! Placement is deliberately plain data: an anchor, a margin, and an optional
//! output name. Config uses it for the built-in clock and media widgets, plugin
//! manifests use it for their widgets, and the renderer turns it into pixels.
//! Keeping it in one module avoids a dependency from config to plugin (or the
//! other way around) just to name a corner of the screen.

/// Where a widget sits on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
  TopLeft,
  TopCenter,
  TopRight,
  BottomLeft,
  BottomCenter,
  #[default]
  BottomRight,
  /// Dead centre. Used by the login card, where a corner anchor would be a
  /// strange default for every screen size.
  Center,
}

impl Anchor {
  /// Converts an anchor plus a logical-pixel offset into a top-left position.
  #[must_use]
  pub fn place(self, screen: (f32, f32), size: (f32, f32), offset: (i32, i32)) -> (f32, f32) {
    let (width, height) = screen;
    let (widget_width, widget_height) = size;
    let (offset_x, offset_y) = (offset.0 as f32, offset.1 as f32);
    let (x, y) = match self {
      Self::TopLeft => (offset_x, offset_y),
      Self::TopCenter => ((width - widget_width) / 2.0 + offset_x, offset_y),
      Self::TopRight => (width - widget_width - offset_x, offset_y),
      Self::BottomLeft => (offset_x, height - widget_height - offset_y),
      Self::BottomCenter => (
        (width - widget_width) / 2.0 + offset_x,
        height - widget_height - offset_y,
      ),
      Self::BottomRight => (
        width - widget_width - offset_x,
        height - widget_height - offset_y,
      ),
      Self::Center => (
        (width - widget_width) / 2.0 + offset_x,
        (height - widget_height) / 2.0 + offset_y,
      ),
    };
    (x.max(0.0), y.max(0.0))
  }

  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "top-left" => Some(Self::TopLeft),
      "top-center" => Some(Self::TopCenter),
      "top-right" => Some(Self::TopRight),
      "bottom-left" => Some(Self::BottomLeft),
      "bottom-center" => Some(Self::BottomCenter),
      "bottom-right" => Some(Self::BottomRight),
      "center" => Some(Self::Center),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::TopLeft => "top-left",
      Self::TopCenter => "top-center",
      Self::TopRight => "top-right",
      Self::BottomLeft => "bottom-left",
      Self::BottomCenter => "bottom-center",
      Self::BottomRight => "bottom-right",
      Self::Center => "center",
    }
  }
}

/// How a decoded image is fitted into the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
  /// Fill the screen, cropping the overflow. Keeps the aspect ratio.
  #[default]
  Cover,
  /// Show the whole image, letterboxing with the background colour.
  Contain,
  /// Stretch to the screen. Distorts the aspect ratio, but some wallpapers
  /// are authored for exactly one resolution and prefer this.
  Fill,
}

impl Fit {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "cover" => Some(Self::Cover),
      "contain" => Some(Self::Contain),
      "fill" => Some(Self::Fill),
      _ => None,
    }
  }
}

/// A widget's anchor, margin, and optional output restriction.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
  pub anchor: Anchor,
  pub offset: (i32, i32),
  /// Output name, for example `DP-1`. `None` means every output.
  pub monitor: Option<String>,
  /// Clockwise rotation around the widget's own centre, in degrees.
  pub rotation: f32,
}

impl Default for Placement {
  fn default() -> Self {
    Self {
      anchor: Anchor::BottomRight,
      offset: (24, 24),
      monitor: None,
      rotation: 0.0,
    }
  }
}

impl Placement {
  /// True when this placement should be drawn on `current`. A missing current
  /// name (screenshot mode, or a compositor that hides output names) shows
  /// every widget, which is friendlier than a preview that silently loses one.
  #[must_use]
  pub fn visible_on(&self, current: Option<&str>) -> bool {
    match (&self.monitor, current) {
      (None, _) => true,
      (Some(_), None) => true,
      (Some(expected), Some(actual)) => expected == actual,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{Anchor, Placement};

  #[test]
  fn anchor_positions_respect_offsets() {
    let (x, y) = Anchor::TopLeft.place((100.0, 50.0), (10.0, 5.0), (2, 3));
    assert_eq!((x, y), (2.0, 3.0));
    let (x, y) = Anchor::BottomRight.place((100.0, 50.0), (10.0, 5.0), (2, 3));
    assert_eq!((x, y), (88.0, 42.0));
  }

  #[test]
  fn monitor_placements_are_permissive_in_preview() {
    let placement = Placement {
      monitor: Some("DP-1".to_string()),
      ..Placement::default()
    };
    assert!(placement.visible_on(None));
    assert!(placement.visible_on(Some("DP-1")));
    assert!(!placement.visible_on(Some("HDMI-1")));
  }
}
