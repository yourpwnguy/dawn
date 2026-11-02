//! Per-widget configuration (clock, media, calendar, battery, system).

use super::*;

/// The month grid widget.
///
/// It draws the current month from the system clock, with the current day in a
/// filled cell. There are no event files and no network access: the calendar is
/// a read-only view of the local date.
#[derive(Debug, Clone)]
pub struct CalendarConfig {
  pub enabled: bool,
  pub placement: Placement,
  /// Day number font size. The cell height is twice this value.
  pub size: f32,
  /// Total content width. The seven columns divide it evenly.
  pub width: f32,
  pub week_start: WeekStart,
  /// Draw the month and year above the weekday row.
  pub show_header: bool,
  /// Colour for ordinary day numbers.
  pub color: Rgb,
  /// Colour for weekday labels, empty cells, and secondary text.
  pub muted_color: Rgb,
  /// Fill behind the current day.
  pub today_color: Rgb,
  pub header_color: Rgb,
  pub layer: WidgetLayer,
  /// Plate, motion, and state visibility for this widget.
  pub look: WidgetLook,
  pub draggable: bool,
}

/// The local battery indicator.
///
/// It reads `/sys/class/power_supply` once every two seconds on the UI tick.
/// The widget never writes to the battery and never asks the network for
/// anything.
#[derive(Debug, Clone)]
pub struct BatteryConfig {
  pub enabled: bool,
  pub placement: Placement,
  pub size: f32,
  pub width: f32,
  pub style: BatteryStyle,
  pub color: Rgb,
  /// Colour used at or below `low_at`.
  pub low_color: Rgb,
  /// Colour used while the battery reports `Charging`.
  pub charging_color: Rgb,
  /// Threshold for `low_color`, 0 to 100.
  pub low_at: u8,
  pub show_percent: bool,
  pub layer: WidgetLayer,
  pub look: WidgetLook,
  pub draggable: bool,
}

/// The local system monitor.
///
/// Hostname, kernel, uptime, load average, and memory all come from `/proc` on
/// the local machine. Nothing is sent anywhere and no file is written.
#[derive(Debug, Clone)]
pub struct SystemConfig {
  pub enabled: bool,
  pub placement: Placement,
  pub size: f32,
  pub width: f32,
  pub style: SystemStyle,
  /// Readings to show, in order. At most six, duplicates removed at load time.
  pub fields: Vec<SystemField>,
  pub color: Rgb,
  pub label_color: Rgb,
  pub layer: WidgetLayer,
  pub look: WidgetLook,
  pub draggable: bool,
}

#[derive(Debug, Clone)]
pub struct ClockConfig {
  pub enabled: bool,
  pub style: ClockStyle,
  /// Draw order relative to the login decor. Built-in widgets default to
  /// `Behind`; `Front` lets a clock sit inside a themed HUD bar. Plugin
  /// widgets have their own `layer` in the manifest.
  pub layer: WidgetLayer,
  /// Plate, motion, and state visibility for this widget.
  pub look: WidgetLook,
  /// jiff/strftime format. Validated at load time so a bad format cannot
  /// produce an empty or panicking clock at draw time.
  pub time_format: String,
  /// Empty hides the date line.
  pub date_format: String,
  /// Display 12-hour time even when the format uses `%H`.
  pub hour12: bool,
  /// Append seconds when the format does not already contain `%S`.
  pub seconds: bool,
  /// Optional IANA time zone. `None` uses the system zone.
  pub timezone: Option<String>,
  /// `true` formats `%c`, `%x`, and `%X` with the POSIX/C locale.
  pub posix_locale: bool,
  pub placement: Placement,
  pub size: f32,
  pub date_size: f32,
  /// Optional colour override. `None` follows the theme.
  pub color: Option<Rgb>,
  pub date_color: Option<Rgb>,
  /// Small label above the time. Empty hides it, which is the default.
  pub label: String,
  /// Label colour. `None` uses the configured `color`.
  pub label_color: Option<Rgb>,
  /// Label size in logical pixels. Zero derives 22 percent of `size`.
  pub label_size: f32,
  /// Label letter spacing in em. The small caps look wants a wide value.
  pub label_tracking: f32,
  /// Draw the unused segments on a seven-segment face at low alpha. The
  /// digital, analog, and flip faces ignore it.
  pub ghost: bool,
  /// Halo colour for the active segments of a seven-segment face.
  pub glow: Option<Rgb>,
  /// Halo spread in logical pixels, 0 to 32. Zero disables the glow even when
  /// `glow` is set.
  pub glow_size: f32,
  /// The colon turns off for half of this period, in milliseconds. Zero keeps
  /// it steady. Applies to the flip and segment faces.
  pub colon_blink_ms: f32,
  /// Motion of the analog second hand.
  pub second_hand: SecondHand,
  /// Lets the pointer move this widget. The position can be saved by `[layout]`.
  pub draggable: bool,
}

/// How the media widget arranges its art and text.
///
/// `Card` is the original composition: square art on the left and the title,
/// artist, and progress on the right. `Compact` is a single tight row for a
/// corner widget, and `Vinyl` puts a circular record above centred text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaLayout {
  #[default]
  Card,
  Compact,
  Vinyl,
}

impl MediaLayout {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "card" => Some(Self::Card),
      "compact" => Some(Self::Compact),
      "vinyl" => Some(Self::Vinyl),
      _ => None,
    }
  }
}

/// The shape used for album art.
///
/// `Cover` keeps the original square crop. `Disc` uses a circle masked at load
/// time, so the expensive per-pixel work happens once per track rather than on
/// every frame. `None` hides the art and lets the text use the full width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaArt {
  #[default]
  Cover,
  Disc,
  None,
}

impl MediaArt {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "cover" => Some(Self::Cover),
      "disc" => Some(Self::Disc),
      "none" => Some(Self::None),
      _ => None,
    }
  }
}

/// The shape of the media progress indicator.
///
/// `Line` is the original thin bar. `Segments` draws twelve blocks and `Wave`
/// draws a decorative sine polyline. Neither needs audio data: the wave is
/// deterministic and only encodes the playback fraction, so it never pretends
/// to be a real spectrum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaProgress {
  #[default]
  Line,
  Segments,
  Wave,
}

impl MediaProgress {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "line" => Some(Self::Line),
      "segments" => Some(Self::Segments),
      "wave" => Some(Self::Wave),
      _ => None,
    }
  }
}

/// Presentation choices for the media widget.
///
/// The art, progress, and tint flags are purely visual. `controls` draws a
/// transport row and, once the widget has real metadata, makes those glyphs
/// clickable: the renderer records hit regions and the window sends the
/// matching MPRIS command. A rotated widget keeps the picture but does not
/// accept clicks, because the hit math stays honest only for the unrotated
/// layout. That keeps a greeter from ever exposing a control it cannot drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MediaLook {
  pub layout: MediaLayout,
  pub art: MediaArt,
  pub progress: MediaProgress,
  /// Draw clickable previous, play/pause, and next buttons under the progress.
  /// They are only active while the widget is showing real metadata and is not
  /// rotated.
  pub controls: bool,
  /// Derive the progress and plate accent from the dominant album-art colour.
  pub tint_from_art: bool,
}

#[derive(Debug, Clone)]
pub struct MediaConfig {
  pub enabled: bool,
  pub placement: Placement,
  pub size: f32,
  pub show_art: bool,
  /// Layout, art shape, progress style, and art tinting.
  pub look: MediaLook,
  /// Draw order relative to the login decor. See [`ClockConfig::layer`].
  pub layer: WidgetLayer,
  /// Plate, motion, and state visibility for this widget. This is the same
  /// shape the clock uses under `clock.look`; media keeps it under
  /// `presentation` because `look` now selects the media composition.
  pub presentation: WidgetLook,
  /// Optional colour override. `None` follows the theme.
  pub color: Option<Rgb>,
  /// Show MPRIS metadata before authentication. This is what makes the widget
  /// useful in a normal desktop session and in `--dry-run`; a real greeter on
  /// a separate bus usually has no player anyway. Set it to false on shared
  /// machines where even the last track name is private.
  pub show_before_login: bool,
  /// Keep the last track when the player pauses, stops, or disappears.
  pub keep_last: bool,
  pub draggable: bool,
}
