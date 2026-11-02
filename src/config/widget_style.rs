//! Shared widget style vocabulary: plates, layers, states, and faces.

use super::*;

/// Visual style of the built-in clock.
///
/// `Digital` and `Analog` are the historical faces. `Flip` and `Segment` are
/// decorative faces for a HUD theme: both are drawn from the same formatted
/// time string, so the usual `time_format`, `hour12`, and `seconds` keys keep
/// working exactly as they do for the digital face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockStyle {
  #[default]
  Digital,
  Analog,
  /// Split-flap cards, one per character.
  Flip,
  /// Seven-segment digits, with optional ghost segments and a glow pass.
  Segment,
}

impl ClockStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "digital" => Some(Self::Digital),
      "analog" => Some(Self::Analog),
      "flip" => Some(Self::Flip),
      "segment" => Some(Self::Segment),
      _ => None,
    }
  }
}

/// How the analog second hand advances.
///
/// `Tick` is the traditional one-step-per-second motion and `Sweep` follows the
/// subsecond clock, which looks smoother but asks the renderer for more frames.
/// This only matters when the analog face is showing seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecondHand {
  /// Jump to each whole second.
  #[default]
  Tick,
  /// Move continuously using the subsecond part of the time.
  Sweep,
}

impl SecondHand {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "tick" => Some(Self::Tick),
      "sweep" => Some(Self::Sweep),
      _ => None,
    }
  }
}

/// Where a built-in widget paints relative to the login decor.
///
/// The default is the historical order: widgets first, then the card and its
/// decor, then the form. A theme with a HUD bar made of decor can move the
/// widget in front of that decor so the digits are not covered, while the
/// form still paints afterwards and can never be obscured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WidgetLayer {
  /// Paint before the decor pass.
  #[default]
  Behind,
  /// Paint after the decor pass but before the form.
  Front,
}

impl WidgetLayer {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "behind" => Some(Self::Behind),
      "front" => Some(Self::Front),
      _ => None,
    }
  }
}

/// How the built-in widget's backplate is filled.
///
/// This is the same vocabulary the decor panel uses, so a theme can make a
/// clock plate and a HUD bar out of one palette. `Tint` is the default because
/// it reproduces the plain scrim every existing config already gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlateStyle {
  /// No plate, border, glow, or shadow. The widget floats on the wallpaper.
  None,
  /// The theme glass tint. This is the historical scrim.
  #[default]
  Tint,
  /// A single flat fill from `plate.color`, falling back to the theme tint.
  Solid,
  /// A linear gradient from `plate.color` to `plate.color_end`.
  Gradient,
  /// The shared blurred backdrop, then `plate.color` on top when it is set.
  /// With no colour this is pure blur; a translucent colour is what makes it
  /// look like a panel rather than a window cut into the wallpaper.
  Glass,
}

impl PlateStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "none" | "transparent" => Some(Self::None),
      "tint" => Some(Self::Tint),
      "solid" => Some(Self::Solid),
      "gradient" => Some(Self::Gradient),
      "glass" | "blur" => Some(Self::Glass),
      _ => None,
    }
  }
}

/// The plate painted behind a built-in clock or media widget.
///
/// The defaults deliberately match the old fixed scrim: a tinted pill with a
/// 14 px radius, 12 px padding, and full opacity. A theme that only wants to
/// recolour the pill can set `color` on a `Tint` plate and leave the rest
/// alone. Radius and padding are logical pixels and scale with the output, just
/// like the widget itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetPlate {
  pub style: PlateStyle,
  /// Fill colour for `Solid`, `Gradient`, and an optional tint over `Glass`.
  /// `None` follows the theme tint on the first two and leaves `Glass` as pure
  /// blur.
  pub color: Option<Rgb>,
  /// Second gradient stop. `None` ends in a fully transparent colour.
  pub color_end: Option<Rgb>,
  /// Gradient direction in degrees, -360 to 360. Zero goes left to right.
  pub gradient_angle: f32,
  /// Corner radius in logical pixels, 0 to 64.
  pub radius: f32,
  /// Optional one-pixel stroke. `Tint` already has a faint theme border, so
  /// this overrides it only when set.
  pub border: Option<Rgb>,
  /// Space between the widget content and the plate edge, 0 to 64.
  pub padding: f32,
  /// Optional outer halo colour.
  pub glow: Option<Rgb>,
  /// Halo spread in logical pixels, 0 to 48. Zero disables it even when a
  /// colour is set.
  pub glow_size: f32,
  /// Draw a soft drop shadow under the plate.
  pub shadow: bool,
  /// Overall opacity for the widget, plate included, 0 to 1.
  pub opacity: f32,
}

impl Default for WidgetPlate {
  fn default() -> Self {
    Self {
      style: PlateStyle::Tint,
      color: None,
      color_end: None,
      gradient_angle: 0.0,
      radius: 14.0,
      border: None,
      padding: 12.0,
      glow: None,
      glow_size: 0.0,
      shadow: false,
      opacity: 1.0,
    }
  }
}

/// A login state a widget can react to.
///
/// The renderer derives exactly one state per frame, in priority order: busy,
/// error, authenticated, typing, idle. A theme can therefore show the clock
/// while the user types and hide it during authentication, without any extra
/// event plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WidgetState {
  /// Nothing typed, no request in flight, no error.
  #[default]
  Idle,
  /// The username or password field has content.
  Typing,
  /// An authentication request is in flight.
  Busy,
  /// The status line is showing an error.
  Error,
  /// greetd accepted the credentials.
  Authenticated,
}

impl WidgetState {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "idle" => Some(Self::Idle),
      "typing" => Some(Self::Typing),
      "busy" => Some(Self::Busy),
      "error" => Some(Self::Error),
      "authenticated" => Some(Self::Authenticated),
      _ => None,
    }
  }
}

/// Presentation for one built-in widget: its plate, motion, and state rules.
///
/// `show_on` empty means "always show". `hide_on` always wins, which is what
/// lets a theme say "show while typing, except when busy" without ordering the
/// lists. Both lists are capped at eight states and deduplicated at load time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetLook {
  pub plate: WidgetPlate,
  /// Movement applied to the whole widget, including its plate and progress
  /// bar. The default is static, so an unconfigured widget does not move.
  pub motion: DecorMotion,
  /// States that may show the widget. Empty means every state.
  pub show_on: Vec<WidgetState>,
  /// States that hide the widget. Takes precedence over `show_on`.
  pub hide_on: Vec<WidgetState>,
}

/// Which day starts a week in the calendar grid.
///
/// The default is Monday because that is what most desktop calendars use. A
/// theme can switch to Sunday without changing anything else in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeekStart {
  #[default]
  Monday,
  Sunday,
}

impl WeekStart {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "monday" => Some(Self::Monday),
      "sunday" => Some(Self::Sunday),
      _ => None,
    }
  }
}

/// How the battery widget draws its remaining charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryStyle {
  /// A ring of sixty radial segments with the percentage in the middle.
  #[default]
  Ring,
  /// A horizontal rounded bar with the percentage to the right.
  Bar,
  /// A battery body with a terminal nub and a filled portion.
  Pill,
}

impl BatteryStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "ring" => Some(Self::Ring),
      "bar" => Some(Self::Bar),
      "pill" => Some(Self::Pill),
      _ => None,
    }
  }
}

/// How the system monitor lays out its readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemStyle {
  /// One label and value per row.
  #[default]
  Rows,
  /// A compact load sparkline above the text rows.
  Sparkline,
}

impl SystemStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "rows" => Some(Self::Rows),
      "sparkline" => Some(Self::Sparkline),
      _ => None,
    }
  }
}

/// One reading the system monitor may show.
///
/// The order in the config is preserved, so a theme can put memory first. The
/// validator drops unknown names and duplicates instead of rejecting the whole
/// widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemField {
  #[default]
  Hostname,
  Kernel,
  Uptime,
  Load,
  Memory,
}

impl SystemField {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "hostname" => Some(Self::Hostname),
      "kernel" => Some(Self::Kernel),
      "uptime" => Some(Self::Uptime),
      "load" => Some(Self::Load),
      "memory" => Some(Self::Memory),
      _ => None,
    }
  }
}
