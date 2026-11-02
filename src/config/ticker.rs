//! Bottom marquee configuration.

use super::*;

/// The edge the ticker strip hugs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickerAnchor {
  Top,
  #[default]
  Bottom,
}

impl TickerAnchor {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "top" => Some(Self::Top),
      "bottom" => Some(Self::Bottom),
      _ => None,
    }
  }
}

/// A kinetic marquee that scrolls one joined text run.
///
/// The run repeats itself at its measured width, so the loop is seamless at
/// any font and tracking value. It is disabled by default and uses the same
/// plate vocabulary as the clock, which is what lets a glass ticket stub sit
/// behind the copy without a second styling system.
#[derive(Debug, Clone, PartialEq)]
pub struct TickerConfig {
  pub enabled: bool,
  /// Up to eight short lines, joined with `separator`.
  pub items: Vec<String>,
  pub separator: String,
  pub anchor: TickerAnchor,
  /// Logical pixel margin from the chosen edge.
  pub offset: (i32, i32),
  pub size: f32,
  /// Scroll speed in logical pixels per second. Zero keeps it still.
  pub speed: f32,
  pub color: Rgb,
  pub bold: bool,
  /// Letter spacing in em.
  pub tracking: f32,
  pub layer: WidgetLayer,
  pub plate: WidgetPlate,
  /// Put the selected session name in front of the configured items and give
  /// the run a one-shot flash when it changes. Off by default, so the marquee
  /// keeps its exact configured order.
  pub session_first: bool,
  /// Speed up the marquee on each beat of `[motion.beat]`. Off by default.
  pub pulse_on_beat: bool,
}

impl Default for TickerConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      items: Vec::new(),
      separator: "  \u{2022}  ".to_string(),
      anchor: TickerAnchor::Bottom,
      offset: (0, 24),
      size: 20.0,
      speed: 60.0,
      color: Rgb::solid(0xF7, 0xF3, 0xE9),
      bold: true,
      tracking: 0.2,
      layer: WidgetLayer::Behind,
      plate: WidgetPlate::default(),
      session_first: false,
      pulse_on_beat: false,
    }
  }
}

impl TickerConfig {
  /// The one run that scrolls, with the separator repeated at the seam so the
  /// wrap never glues the last line to the first one.
  #[must_use]
  pub fn run(&self) -> String {
    self.run_with(None)
  }

  /// The run with an optional selected session placed first. Session names
  /// come from desktop files and are displayed in the poster's uppercase
  /// voice; the launch command is untouched.
  ///
  /// The session name is live input from the session finder, so it is not part
  /// of `items`. Keeping it separate means the configured list stays a pure
  /// document value and the reorder is a deterministic function that tests can
  /// call without a running greeter.
  #[must_use]
  pub fn run_with(&self, session: Option<&str>) -> String {
    let session = session
      .filter(|name| self.session_first && !name.is_empty())
      .map(|name| name.to_uppercase());
    if self.items.is_empty() && session.is_none() {
      return String::new();
    }
    let mut lines = Vec::with_capacity(self.items.len() + 1);
    if let Some(session) = session {
      lines.push(session);
    }
    for item in &self.items {
      // A configured line that happens to repeat the live session would read
      // as a stutter in the marquee, so drop it while it is first.
      if lines.first() == Some(item) && self.session_first {
        continue;
      }
      lines.push(item.clone());
    }
    if lines.is_empty() {
      return String::new();
    }
    let mut run = lines.join(&self.separator);
    run.push_str(&self.separator);
    run
  }
}
