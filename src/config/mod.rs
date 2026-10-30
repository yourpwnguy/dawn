//! Config loading, validation, and defaults.
//!
//! Config errors must never prevent a login. So `load` always returns a usable
//! [`Config`], plus a list of [`Issue`]s describing every field that fell back
//! to its default. The UI shows a short summary and the log has the details;
//! `dawn --check-config` turns the same data into an admin-friendly report.
//!
//! Precedence: `--config PATH`, then `$XDG_CONFIG_HOME/dawn/config.toml`, then
//! `/etc/dawn/config.toml`, then built-in defaults. Relative paths inside the
//! file are resolved against the config file's directory, and `~/` is expanded.

mod raw;
mod validate;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::color::Rgb;
use crate::layout::{Anchor, Fit, Placement};
use raw::RawConfig;
use validate::Validator;

/// A problem with the config file. Field-level problems are recoverable: the
/// field uses its default and every other setting still applies.
#[derive(Debug, Clone)]
pub struct Issue {
  /// Dotted key path, for example `ui.font_size`. Per-output overrides append
  /// the output name, as in `background.monitors.DP-1.image`, so the message
  /// points at the exact table that has the problem.
  pub key: String,
  pub location: Option<Location>,
  pub message: String,
}

#[derive(Debug, Clone)]
pub struct Location {
  pub file: PathBuf,
  pub line: usize,
  pub column: usize,
}

impl fmt::Display for Issue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match &self.location {
      Some(location) => write!(
        f,
        "{} ({}:{}:{}): {}",
        self.key,
        location.file.display(),
        location.line,
        location.column,
        self.message
      ),
      None => write!(f, "{}: {}", self.key, self.message),
    }
  }
}

/// The result of loading config: always a usable value, plus diagnostics.
#[derive(Debug)]
pub struct LoadedConfig {
  pub config: Config,
  pub path: Option<PathBuf>,
  pub issues: Vec<Issue>,
  /// A `[theme].package` id from the parsed file. The caller resolves it so the
  /// config layer does not depend on the theme store.
  pub package: Option<String>,
}

impl LoadedConfig {
  /// A one-line summary suitable for the login screen. We intentionally do
  /// not paint the entire error there: it would push the password field
  /// around, and an admin reading this at boot can check the log.
  #[must_use]
  pub fn summary(&self) -> Option<String> {
    let first = self.issues.first()?;
    let more = self.issues.len().saturating_sub(1);
    let mut text = format!("Config warning: {first}");
    if more > 0 {
      text.push_str(&format!(" (+{more} more, see the log)"));
    }
    Some(text)
  }
}

#[derive(Debug, Clone)]
pub struct Config {
  pub ui: UiConfig,
  /// The login card, its fields, and every visible string in the form.
  pub login: LoginConfig,
  pub clock: ClockConfig,
  pub media: MediaConfig,
  pub background: BackgroundConfig,
  pub theme: ThemeConfig,
  pub motion: MotionConfig,
  pub power: PowerConfig,
  /// Adapter, backend, and fallback policy for the live GPU compositor.
  pub gpu: GpuConfig,
  /// Dragging and where the dragged positions are remembered.
  pub layout: LayoutConfig,
  pub sessions: SessionsConfig,
  pub plugins: PluginsConfig,
  pub auth: AuthConfig,
}

#[derive(Debug, Clone)]
pub struct UiConfig {
  /// Font family name. Empty or unavailable names fall back to the embedded
  /// Inter family, so text is always readable.
  pub font: String,
  pub font_size: f32,
  /// Kept for older configs. New configs should use `login.width`.
  pub card_width: f32,
  pub fullscreen: bool,
  /// Kept for older configs. New configs should use `login.greeting`.
  pub greeting: String,
}

/// The login card and everything inside it.
///
/// This is deliberately a data structure, not a pile of hardcoded strings in
/// the renderer. The form still has behaviour (Tab, Enter, PAM prompts), but
/// its position, size, labels, colours, and hints are all configuration.
/// How much freedom the login card has over the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoginLayout {
  /// The compact card: frosted glass over the wallpaper, height from content.
  #[default]
  Card,
  /// A fixed-size, edge-to-edge panel for a themed login screen. The panel
  /// can fade into the wallpaper, and the form sits at `form_offset` instead
  /// of the card's padding. This is what a website-style clone needs.
  Hero,
}

/// How the login card meets the wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceStyle {
  /// The frosted glass panel.
  #[default]
  Glass,
  /// A flat fill, with no blur and no glass highlights.
  Flat,
  /// No panel, border, or shadow at all. The fields and text float directly
  /// on the wallpaper, which is what someone means by "hide the card".
  None,
}

impl SurfaceStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "glass" => Some(Self::Glass),
      "flat" | "solid" => Some(Self::Flat),
      "none" | "transparent" => Some(Self::None),
      _ => None,
    }
  }
}

/// Which edge a hover colour grows from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HoverReveal {
  /// Cross-fade the whole fill.
  #[default]
  Fade,
  Left,
  Right,
  Up,
  Down,
  /// Grow outward from the centre line.
  Center,
}

impl HoverReveal {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "fade" => Some(Self::Fade),
      "left" => Some(Self::Left),
      "right" => Some(Self::Right),
      "up" => Some(Self::Up),
      "down" => Some(Self::Down),
      "center" => Some(Self::Center),
      _ => None,
    }
  }
}

/// A game-style effect drawn over (or around) the submit button while hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonEffect {
  #[default]
  None,
  /// A bright band travels across the face.
  Sweep,
  /// A narrow specular highlight, like a collectible card.
  Sheen,
  /// The fill breathes brighter and darker.
  Pulse,
  /// A pulsing outer glow. `glow` and `glow_size` still choose the colour and
  /// spread; the effect only controls the breathing.
  Glow,
  /// Four corner brackets breathe outward like a targeting cue.
  Corners,
}

impl ButtonEffect {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "none" => Some(Self::None),
      "sweep" => Some(Self::Sweep),
      "sheen" | "shine" => Some(Self::Sheen),
      "pulse" => Some(Self::Pulse),
      "glow" => Some(Self::Glow),
      "corners" => Some(Self::Corners),
      _ => None,
    }
  }
}

#[derive(Debug, Clone)]
pub struct LoginConfig {
  pub layout: LoginLayout,
  pub placement: Placement,
  pub width: f32,
  /// Fixed height in logical pixels. Zero keeps the content-derived height,
  /// which is what `layout = "card"` uses.
  pub height: f32,
  pub padding: f32,
  pub gap: f32,
  pub radius: f32,
  /// Hero panel colour. `None` follows the theme tint.
  pub panel: Option<Rgb>,
  /// Hero panel fade as `[start, end]` fractions of the card width. Before
  /// `start` the panel is transparent and the wallpaper shows through; after
  /// `end` it is fully opaque. Only used by `layout = "hero"`.
  pub panel_fade: (f32, f32),
  /// Card border for the hero layout. `None` follows the theme border.
  pub border: Option<Rgb>,
  /// Glass, flat, or no surface at all.
  pub surface: SurfaceStyle,
  /// Draw the card's shadow. Ignored by `surface = "none"`.
  pub show_shadow: bool,
  /// Draw the card's outline. Ignored by `surface = "none"`.
  pub show_border: bool,
  /// Rotation of the whole card in degrees. The form, decor, and tab rotate
  /// with it, which is what makes an off-axis layout possible.
  pub rotation: f32,
  /// Lets the user move the whole card with the pointer. Off by default because
  /// a stray click while typing should never move the form.
  pub draggable: bool,
  /// Empty hides the greeting line.
  pub greeting: String,
  pub show_hints: bool,
  pub show_picker: bool,
  pub picker_title: String,
  pub busy_text: String,
  pub dry_run_badge: String,
  pub show_dry_run_badge: bool,
  pub no_sessions: String,
  /// Shared field defaults. Username and password override them individually.
  pub field: FieldConfig,
  pub username: FieldConfig,
  pub password: FieldConfig,
  pub session: SessionRowConfig,
  pub hints: HintConfig,
  /// Top-left tab label, used by the hero layout. Empty hides it.
  pub tab: String,
  /// Where the username field starts inside the card, in logical pixels. The
  /// hero layout uses this instead of the card padding.
  pub form_offset: (i32, i32),
  /// Width of the credentials/button column. Zero uses the card padding.
  pub form_width: f32,
  pub button: ButtonConfig,
  /// Cosmetic images and text placed inside the card. The host draws these
  /// after the panel and before the form, so they never cover an input.
  pub decor: Vec<DecorSpec>,
}

/// The submit button. Off by default so the classic card stays keyboard-only.
#[derive(Debug, Clone)]
pub struct ButtonConfig {
  pub show: bool,
  pub text: String,
  /// Vertical gap above the button.
  pub gap: f32,
  pub height: f32,
  pub radius: f32,
  /// Zero inherits the global `ui.font_size`.
  pub font_size: f32,
  pub background: Option<Rgb>,
  /// Second fill stop. When set, the button uses a gradient.
  pub background_end: Option<Rgb>,
  /// Gradient direction in degrees. 0 is left to right, 90 top to bottom.
  pub gradient_angle: f32,
  pub text_color: Option<Rgb>,
  /// Draw the broken targeting bracket around the button.
  pub bracket: Option<Rgb>,
  pub bracket_gap: f32,
  pub bold: bool,
  /// Letter spacing in em. Wide tracking suits short, uppercase button copy.
  pub tracking: f32,
  /// Fill after the pointer enters. `None` derives a lighter version of the
  /// base fill, so even a config with no animation keys gets a hover response.
  pub hover_background: Option<Rgb>,
  pub hover_background_end: Option<Rgb>,
  pub hover_text_color: Option<Rgb>,
  pub hover_direction: HoverReveal,
  /// Duration of the hover colour transition, in milliseconds.
  pub hover_ms: f32,
  /// Uniform scale while hovered. 1.0 keeps the geometry still.
  pub hover_scale: f32,
  /// Upward movement while hovered, in logical pixels.
  pub hover_lift: f32,
  /// Outer glow colour while hovered.
  pub glow: Option<Rgb>,
  pub glow_size: f32,
  pub effect: ButtonEffect,
  pub effect_color: Option<Rgb>,
  /// One full effect cycle, in milliseconds.
  pub effect_ms: f32,
  /// Direction of sweep/sheen bands, in degrees.
  pub effect_angle: f32,
  /// Alpha of the effect, 0 to 1.
  pub effect_strength: f32,
  pub rotation: f32,
}

impl Default for ButtonConfig {
  fn default() -> Self {
    Self {
      show: false,
      text: "Sign in".to_string(),
      gap: 16.0,
      height: 44.0,
      radius: 8.0,
      font_size: 0.0,
      background: None,
      background_end: None,
      gradient_angle: 0.0,
      text_color: None,
      bracket: None,
      bracket_gap: 0.0,
      bold: false,
      tracking: 0.0,
      hover_background: None,
      hover_background_end: None,
      hover_text_color: None,
      hover_direction: HoverReveal::Fade,
      hover_ms: 180.0,
      hover_scale: 1.0,
      hover_lift: 0.0,
      glow: None,
      glow_size: 0.0,
      effect: ButtonEffect::None,
      effect_color: None,
      effect_ms: 1400.0,
      effect_angle: 120.0,
      effect_strength: 0.25,
      rotation: 0.0,
    }
  }
}

impl ButtonConfig {
  /// True when something about the button changes over time. The window uses
  /// this to keep requesting frames only while the pointer is on the button.
  #[must_use]
  pub fn has_motion(&self) -> bool {
    self.hover_ms > 0.0
      || (self.hover_scale - 1.0).abs() > f32::EPSILON
      || self.hover_lift > 0.0
      || (self.glow_size > 0.0 && self.glow.is_some())
      || self.effect != ButtonEffect::None
  }
}

/// One cosmetic item inside the login card.
#[derive(Debug, Clone)]
pub enum DecorSpec {
  Text {
    text: String,
    size: f32,
    color: Option<Rgb>,
    bold: bool,
    tracking: f32,
    /// Optional outline colour. When set, the run is drawn around a small
    /// radius before the fill pass, which keeps a thin HUD label readable over
    /// busy artwork.
    stroke: Option<Rgb>,
    /// Outline radius in logical pixels, 0 to 12. A set `stroke` with a zero
    /// width is resolved to 2 so the option never silently does nothing.
    stroke_width: f32,
    anchor: Anchor,
    offset: (i32, i32),
    rotation: f32,
    motion: DecorMotion,
  },
  Image {
    path: PathBuf,
    width: f32,
    anchor: Anchor,
    offset: (i32, i32),
    opacity: f32,
    rotation: f32,
    motion: DecorMotion,
  },
  /// An anchored plate behind or over other decor. Unlike an image it needs no
  /// asset; the colour falls back to the theme tint. `color_end` turns the fill
  /// into a linear gradient at `gradient_angle`, and `border` adds a one-pixel
  /// stroke. Typewriter and ticker motion do not apply, but blink still does.
  Panel {
    width: f32,
    height: f32,
    color: Option<Rgb>,
    color_end: Option<Rgb>,
    gradient_angle: f32,
    radius: f32,
    border: Option<Rgb>,
    opacity: f32,
    anchor: Anchor,
    offset: (i32, i32),
    rotation: f32,
    motion: DecorMotion,
  },
}

/// How a decor item moves. All values are optional in TOML; an item with no
/// motion table stays completely static.
///
/// The transform motions are deliberately additive so one ornament can spin,
/// drift, and breathe at the same time. `phase` offsets the whole pattern in
/// turns, which is how a row of particles avoids moving in lockstep. The
/// clock-like fields have two scopes: `blink_ms` gates both text and image
/// decor, while `typer_ms` and the ticker pair only apply to text; images
/// ignore those three.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorMotion {
  /// Continuous rotation in degrees per second.
  pub spin: f32,
  /// Sine-wave rotation amplitude in degrees.
  pub sway: f32,
  /// Period of `sway` in milliseconds.
  pub sway_ms: f32,
  /// Elliptical drift amplitude in logical pixels. X uses sine, Y uses cosine.
  pub float: (i32, i32),
  /// Period of `float` in milliseconds.
  pub float_ms: f32,
  /// Scale pulse as a fraction of the base size, 0 to 0.5.
  pub pulse: f32,
  /// Period of `pulse` in milliseconds.
  pub pulse_ms: f32,
  /// Opacity fade depth, 0 to 1. The item cycles between full and `1 - fade`.
  pub fade: f32,
  /// Period of `fade` in milliseconds.
  pub fade_ms: f32,
  /// Phase offset in turns, 0 to 1.
  pub phase: f32,
  /// One-shot character reveal time for text decor, in milliseconds. Zero
  /// disables it. The reveal is linear, because a typewriter should not ease:
  /// easing makes the rhythm feel like it stutters near the end.
  pub typer_ms: f32,
  /// Square-wave blink period for text and image decor, in milliseconds. Zero
  /// disables it. The first half of each period is visible, the second half is
  /// not, which is the classic terminal cursor rhythm rather than a smooth
  /// fade.
  pub blink_ms: f32,
  /// Time for a ticker to travel one full tile, in milliseconds. Zero disables
  /// scrolling. A ticker overrides the typewriter on the same item, since a
  /// marquee is already showing the whole run.
  pub ticker_ms: f32,
  /// Width of the ticker strip in logical pixels. Zero leaves the ticker off
  /// even when `ticker_ms` is set, because there is nowhere to draw it.
  pub ticker_width: i32,
  /// Space between two copies of the ticker text, in logical pixels. The gap
  /// is part of the tile, so the loop stays seamless.
  pub ticker_gap: i32,
}

impl Default for DecorMotion {
  fn default() -> Self {
    Self {
      spin: 0.0,
      sway: 0.0,
      sway_ms: 6000.0,
      float: (0, 0),
      float_ms: 7000.0,
      pulse: 0.0,
      pulse_ms: 2400.0,
      fade: 0.0,
      fade_ms: 3000.0,
      phase: 0.0,
      typer_ms: 0.0,
      blink_ms: 0.0,
      ticker_ms: 0.0,
      ticker_width: 0,
      ticker_gap: 64,
    }
  }
}

/// The values one decor item uses at a single instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorSample {
  /// Drift from the configured anchor, in logical pixels.
  pub offset: (f32, f32),
  /// Additional rotation from `spin` and `sway`, in degrees.
  pub angle: f32,
  /// Scale multiplier around the item centre.
  pub scale: f32,
  /// Opacity multiplier from `fade`, 0 to 1.
  pub opacity: f32,
  /// Portion of the text visible from the typewriter, 0 to 1. It is 1 when
  /// `typer_ms` is off, so a static item never looks half-written.
  pub reveal: f32,
  /// Whether the blink wave is in its visible half. It is true when blink is
  /// off, which keeps the common case free of an extra branch in the renderer.
  pub blink_on: bool,
  /// Fraction of one ticker tile already travelled, 0 to 1. Zero when the
  /// ticker is off.
  pub loop_frac: f32,
}

impl DecorMotion {
  /// True when any component can change over time.
  #[must_use]
  pub fn active(self) -> bool {
    self.spin.abs() > f32::EPSILON
      || self.sway > 0.0
      || self.float != (0, 0)
      || self.pulse > 0.0
      || self.fade > 0.0
      || self.typer_ms > 0.0
      || self.blink_ms > 0.0
      || (self.ticker_ms > 0.0 && self.ticker_width > 0)
  }

  /// Samples the motion at `elapsed` seconds since the UI started.
  ///
  /// The formulas are simple on purpose: phase 0 starts at the neutral
  /// position, so a screenshot (`elapsed = 0`) matches the first live frame.
  #[must_use]
  pub fn sample(self, elapsed: f32) -> DecorSample {
    if !elapsed.is_finite() {
      return DecorSample {
        offset: (0.0, 0.0),
        angle: 0.0,
        scale: 1.0,
        opacity: 1.0,
        reveal: 1.0,
        blink_on: true,
        loop_frac: 0.0,
      };
    }
    let turns = elapsed.max(0.0);
    let ms = turns * 1000.0;
    let phase = self.phase.rem_euclid(1.0) * std::f32::consts::TAU;
    let wave = |period_ms: f32| std::f32::consts::TAU * turns / (period_ms.max(1.0) / 1000.0);
    // Every oscillation subtracts its value at phase alone, so `elapsed = 0`
    // is the neutral pose no matter how the item is phase-shifted. Without
    // that, a staggered row would start mid-stride and screenshots would
    // disagree with the first live frame.
    let float_x = self.float.0 as f32 * ((wave(self.float_ms) + phase).sin() - phase.sin());
    let float_y = self.float.1 as f32 * ((wave(self.float_ms) + phase).cos() - phase.cos());
    let angle = self.spin * turns + self.sway * ((wave(self.sway_ms) + phase).sin() - phase.sin());
    let scale = 1.0 + self.pulse * ((wave(self.pulse_ms) + phase).sin() - phase.sin());
    let rest = 0.5 - 0.5 * phase.cos();
    let opacity = 1.0 - self.fade * ((0.5 - 0.5 * (wave(self.fade_ms) + phase).cos()) - rest);
    DecorSample {
      offset: (float_x, float_y),
      angle,
      scale: scale.clamp(0.1, 2.0),
      opacity: opacity.clamp(0.0, 1.0),
      reveal: if self.typer_ms <= 0.0 {
        1.0
      } else {
        (ms / self.typer_ms).clamp(0.0, 1.0)
      },
      blink_on: if self.blink_ms <= 0.0 {
        true
      } else {
        ((ms / self.blink_ms) as u64).is_multiple_of(2)
      },
      loop_frac: if self.ticker_ms <= 0.0 {
        0.0
      } else {
        (ms / self.ticker_ms) % 1.0
      },
    }
  }
}

/// Shape of the focused input caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaretStyle {
  /// A thin vertical line, the familiar text-editing caret.
  #[default]
  Bar,
  /// A filled rectangle as wide as a character cell. This is the arcade and
  /// terminal look, and it stays readable on top of a busy HUD background.
  Block,
}

impl CaretStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "bar" => Some(Self::Bar),
      "block" => Some(Self::Block),
      _ => None,
    }
  }
}

/// One input field's text and styling.
///
/// A zero value in `font_size`, `height`, `radius`, `padding`, or
/// `caret_width` means "derive it from `ui.font_size`". That is what keeps a
/// complete-looking config short: you only write the values you want to change.
#[derive(Debug, Clone)]
pub struct FieldConfig {
  /// Optional label drawn above the field. Empty hides it.
  pub label: String,
  /// Label colour. `None` falls back to the theme muted token.
  pub label_color: Option<Rgb>,
  pub placeholder: String,
  pub font_size: f32,
  pub height: f32,
  pub radius: f32,
  pub padding: f32,
  pub background: Option<Rgb>,
  /// Optional second fill colour. When set, the field is filled with a
  /// horizontal gradient from `background` to `background_end`.
  pub background_end: Option<Rgb>,
  pub border: Option<Rgb>,
  /// A rule under the field instead of a full box. When set, the normal border
  /// stroke is skipped and focus thickens the rule instead of drawing a ring.
  pub underline: Option<Rgb>,
  /// Underline thickness in logical pixels. Zero derives a one-pixel rule.
  pub underline_size: f32,
  /// Broken targeting bracket drawn outside the field. `None` hides it.
  pub bracket: Option<Rgb>,
  /// How far the bracket sits outside the field, in logical pixels.
  pub bracket_gap: f32,
  pub text: Option<Rgb>,
  pub placeholder_color: Option<Rgb>,
  pub focus: Option<Rgb>,
  pub caret: Option<Rgb>,
  pub caret_width: f32,
  /// Bar or block. The block style uses the same ink height and a wider
  /// derived width, so a theme can switch the whole form to an arcade caret
  /// without measuring anything again.
  pub caret_style: CaretStyle,
  /// Leading glyph inside the field: `"user"`, `"lock"`, or empty.
  pub icon: String,
  /// Zero derives the icon size from the field text size.
  pub icon_size: f32,
  /// Gap between the icon and the typed text. Zero derives it.
  pub icon_gap: f32,
  pub icon_color: Option<Rgb>,
  /// Rotation of the field, its label, and its bracket, in degrees.
  pub rotation: f32,
  /// Password masking glyphs. One glyph per typed character is drawn.
  pub mask: String,
}

impl FieldConfig {
  fn default_username() -> Self {
    Self {
      label: String::new(),
      label_color: None,
      placeholder: "Username".to_string(),
      font_size: 0.0,
      height: 0.0,
      radius: 0.0,
      padding: 0.0,
      background: None,
      background_end: None,
      border: None,
      underline: None,
      underline_size: 0.0,
      bracket: None,
      bracket_gap: 0.0,
      text: None,
      placeholder_color: None,
      focus: None,
      caret: None,
      caret_width: 0.0,
      caret_style: CaretStyle::Bar,
      icon: String::new(),
      icon_size: 0.0,
      icon_gap: 0.0,
      icon_color: None,
      rotation: 0.0,
      mask: String::new(),
    }
  }

  fn default_password() -> Self {
    Self {
      placeholder: "Password".to_string(),
      mask: "\u{2022}".to_string(),
      ..Self::default_username()
    }
  }
}

#[derive(Debug, Clone)]
pub struct SessionRowConfig {
  pub label: String,
  pub change_hint: String,
  pub empty: String,
  pub show: bool,
}

#[derive(Debug, Clone)]
pub struct HintConfig {
  pub username: String,
  pub password: String,
  pub picker: String,
  pub busy: String,
}

#[derive(Debug, Clone)]
pub struct LayoutConfig {
  /// Persist dragged positions between runs. Off by default: a greeter should
  /// not write files unless the administrator asked for it.
  pub save: bool,
  /// Absolute path, or `None` to use the per-user state directory.
  pub file: Option<PathBuf>,
}

impl LayoutConfig {
  /// The file used when `save = true` and no explicit path was given.
  ///
  /// We prefer the user state directory so a test run in a normal desktop
  /// session stays inside the user's home. If neither `XDG_STATE_HOME` nor
  /// `HOME` is set, we fall back to `/var/lib/dawn`; an unprivileged greeter
  /// cannot create that directory, so the save fails with a warning and the
  /// form itself keeps working.
  #[must_use]
  pub fn resolved_file(&self) -> PathBuf {
    if let Some(path) = &self.file {
      return path.clone();
    }
    if let Some(state) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
      return PathBuf::from(state).join("dawn").join("layout.toml");
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
      return PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("dawn")
        .join("layout.toml");
    }
    PathBuf::from("/var/lib/dawn/layout.toml")
  }
}

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
}

impl PlateStyle {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "none" | "transparent" => Some(Self::None),
      "tint" | "glass" => Some(Self::Tint),
      "solid" => Some(Self::Solid),
      "gradient" => Some(Self::Gradient),
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
  /// Fill colour for `Solid` and `Gradient`. `None` follows the theme tint.
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
/// transport row, but those glyphs are deliberately not clickable yet: pointer
/// hit regions and the D-Bus control wiring are a separate change, and a
/// greeter must never expose a control it cannot actually drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MediaLook {
  pub layout: MediaLayout,
  pub art: MediaArt,
  pub progress: MediaProgress,
  /// Draw previous, play/pause, and next glyphs under the progress. Visual
  /// only for now.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundKind {
  /// Detect from the file: a multi-frame GIF/WebP is animated, anything else
  /// is a still image. No image means the colour or gradient.
  #[default]
  Auto,
  Solid,
  Gradient,
  Image,
  Animated,
}

impl BackgroundKind {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "solid" => Some(Self::Solid),
      "gradient" => Some(Self::Gradient),
      "image" => Some(Self::Image),
      "animated" | "video" => Some(Self::Animated),
      _ => None,
    }
  }
}

#[derive(Debug, Clone)]
pub struct BackgroundConfig {
  pub kind: BackgroundKind,
  pub color: Rgb,
  /// A very subtle vertical gradient derived from `color`. Set to false for a
  /// perfectly flat background.
  pub gradient: bool,
  pub image: Option<PathBuf>,
  pub fit: Fit,
  /// Upper bound on animated playback. The file's own delays are respected
  /// when they are slower than this.
  pub fps: u32,
  /// Hard budget for decoded wallpaper frames.
  pub memory_limit: u64,
  /// Freeze animation after this many seconds without input. Zero disables it.
  pub idle_pause: Duration,
  /// Per-output overrides keyed by output name, for example `DP-1`.
  pub monitors: BTreeMap<String, BackgroundOverride>,
}

impl BackgroundConfig {
  /// The resolved base colour for one output. This is the cheap lookup the
  /// renderer uses every frame; `for_monitor` clones the whole config and is
  /// only needed when starting a load.
  #[must_use]
  pub fn color_for_monitor(&self, monitor: Option<&str>) -> Rgb {
    monitor
      .and_then(|name| self.monitors.get(name))
      .and_then(|over| over.color)
      .unwrap_or(self.color)
  }

  /// Base config with the named output's overrides applied. Unknown outputs
  /// simply keep the base values.
  #[must_use]
  pub fn for_monitor(&self, monitor: Option<&str>) -> Self {
    let mut resolved = self.clone();
    let Some(monitor) = monitor else {
      return resolved;
    };
    let Some(over) = self.monitors.get(monitor) else {
      return resolved;
    };
    if let Some(kind) = over.kind {
      resolved.kind = kind;
    }
    if let Some(color) = over.color {
      resolved.color = color;
    }
    if let Some(image) = &over.image {
      resolved.image = Some(image.clone());
    }
    if let Some(fit) = over.fit {
      resolved.fit = fit;
    }
    resolved
  }
}

#[derive(Debug, Clone, Default)]
pub struct BackgroundOverride {
  pub kind: Option<BackgroundKind>,
  pub color: Option<Rgb>,
  pub image: Option<PathBuf>,
  pub fit: Option<Fit>,
}

/// One of the shipped design presets. A preset only supplies defaults; every
/// token can still be overridden in `[theme]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
  #[default]
  GlassDark,
  GlassLight,
  Minimal,
}

impl ThemePreset {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "glass-dark" | "dark" => Some(Self::GlassDark),
      "glass-light" | "light" => Some(Self::GlassLight),
      "minimal" => Some(Self::Minimal),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::GlassDark => "glass-dark",
      Self::GlassLight => "glass-light",
      Self::Minimal => "minimal",
    }
  }

  /// The background colour used when the user did not set one. This is what
  /// makes the light preset usable with zero config.
  #[must_use]
  pub fn background_color(self) -> Rgb {
    match self {
      Self::GlassDark => Rgb::solid(11, 16, 32),
      Self::GlassLight => Rgb::solid(223, 232, 245),
      Self::Minimal => Rgb::solid(16, 17, 20),
    }
  }

  /// Resolved design tokens for this preset.
  #[must_use]
  pub fn tokens(self) -> ThemeConfig {
    match self {
      Self::GlassDark => ThemeConfig {
        preset: self,
        text: Rgb::solid(242, 246, 255),
        muted: Rgb::solid(169, 180, 200),
        accent: Rgb::solid(125, 177, 255),
        error: Rgb::solid(255, 138, 128),
        tint: Rgb::rgba(16, 23, 38, 204),
        border: Rgb::rgba(255, 255, 255, 64),
        shadow: Rgb::rgba(0, 0, 0, 102),
        focus_ring: Rgb::solid(125, 177, 255),
        blur: BlurConfig {
          refresh: Duration::from_millis(250),
          ..BlurConfig::default()
        },
        corner_radius: 18.0,
        spacing: 1.0,
        noise: true,
        min_contrast: 4.5,
      },
      Self::GlassLight => ThemeConfig {
        preset: self,
        text: Rgb::solid(16, 19, 24),
        muted: Rgb::solid(77, 86, 101),
        accent: Rgb::solid(31, 111, 235),
        error: Rgb::solid(180, 35, 24),
        tint: Rgb::rgba(247, 249, 252, 210),
        border: Rgb::rgba(255, 255, 255, 204),
        shadow: Rgb::rgba(0, 0, 0, 51),
        focus_ring: Rgb::solid(31, 111, 235),
        blur: BlurConfig {
          refresh: Duration::from_millis(250),
          ..BlurConfig::default()
        },
        corner_radius: 18.0,
        spacing: 1.0,
        noise: true,
        min_contrast: 4.5,
      },
      Self::Minimal => ThemeConfig {
        preset: self,
        text: Rgb::solid(245, 245, 245),
        muted: Rgb::solid(160, 160, 160),
        accent: Rgb::solid(158, 203, 255),
        error: Rgb::solid(255, 148, 140),
        tint: Rgb::rgba(23, 24, 28, 242),
        border: Rgb::rgba(255, 255, 255, 34),
        shadow: Rgb::rgba(0, 0, 0, 85),
        focus_ring: Rgb::solid(158, 203, 255),
        blur: BlurConfig {
          enabled: false,
          refresh: Duration::from_millis(500),
          ..BlurConfig::default()
        },
        corner_radius: 10.0,
        spacing: 0.95,
        noise: false,
        min_contrast: 4.5,
      },
    }
  }
}

#[derive(Debug, Clone)]
pub struct ThemeConfig {
  pub preset: ThemePreset,
  pub text: Rgb,
  pub muted: Rgb,
  pub accent: Rgb,
  pub error: Rgb,
  /// Glass tint. The alpha channel is the tint opacity.
  pub tint: Rgb,
  pub border: Rgb,
  pub shadow: Rgb,
  pub focus_ring: Rgb,
  /// Backdrop blur, using Hyprland's `decoration:blur` vocabulary and a CPU
  /// port of its dual Kawase pyramid.
  pub blur: BlurConfig,
  pub corner_radius: f32,
  /// Multiplier for padding and gaps.
  pub spacing: f32,
  pub noise: bool,
  /// Minimum contrast ratio for text against the glass tint.
  pub min_contrast: f32,
}

/// Backdrop blur settings.
///
/// The names and defaults mirror Hyprland's `decoration:blur` block, because
/// that is the look people ask for when they say "blur like my compositor".
/// The CPU implementation uses the same prepare, downsample, upsample, and
/// finish stages; only colour management and xray are not applicable to a
/// single full-screen greeter.
#[derive(Debug, Clone)]
pub struct BlurConfig {
  pub enabled: bool,
  /// Blur radius in logical pixels, Hyprland's `size`.
  pub size: f32,
  /// Number of downsample/upsample pairs, 1 to 8.
  pub passes: u32,
  /// Per-pixel grain amount, Hyprland's `noise`.
  pub noise: f32,
  /// Contrast curve applied before blurring, Hyprland's `contrast`.
  pub contrast: f32,
  /// Brightness applied before and after blurring, Hyprland's `brightness`.
  pub brightness: f32,
  /// Saturation boost during the downsample, Hyprland's `vibrancy`.
  pub vibrancy: f32,
  /// How much dark colours are protected from the vibrancy boost.
  pub vibrancy_darkness: f32,
  /// How often an animated wallpaper refreshes the cached blur.
  pub refresh: Duration,
}

impl Default for BlurConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      size: 8.0,
      passes: 3,
      noise: 0.0117,
      contrast: 0.8916,
      brightness: 0.8172,
      vibrancy: 0.1696,
      vibrancy_darkness: 0.0,
      refresh: Duration::from_millis(300),
    }
  }
}

#[derive(Debug, Clone)]
pub struct MotionConfig {
  pub enabled: bool,
  /// Skip all motion, including background animation, even if enabled.
  pub reduced: bool,
  pub boot: Duration,
  pub focus: Duration,
  pub shake: Duration,
  /// Time constant for the widget drag easing. Zero makes dragging snap.
  pub drag: Duration,
  /// Refresh rate for animated decor only. Ambient ornaments are slow, so
  /// capping their overlay redraws keeps the CPU free for the wallpaper while
  /// focus, typing, and button effects still run at the full motion rate.
  pub decor_fps: u32,
}

impl MotionConfig {
  /// The wall-clock gap between two decor-only overlay frames.
  #[must_use]
  pub fn decor_frame(&self) -> Duration {
    Duration::from_millis(1000 / u64::from(self.decor_fps.max(1)))
  }
}

impl Default for MotionConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      reduced: false,
      boot: Duration::from_millis(450),
      focus: Duration::from_millis(160),
      shake: Duration::from_millis(320),
      drag: Duration::from_millis(120),
      decor_fps: 30,
    }
  }
}

#[derive(Debug, Clone, Default)]
pub struct PowerConfig {
  /// Freeze animation and motion. Intended for battery-powered or remote
  /// greeter hardware; it never affects login responsiveness.
  pub low_power: bool,
}

/// Which graphics API the live compositor may use.
///
/// This is deliberately separate from the software compositor used for
/// screenshots and the terminal fallback: `[gpu]` only influences the window's
/// GPU path. A driver that enumerates but cannot present still falls back to
/// softbuffer, so a bad value here can never stop a login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuBackend {
  /// Try Vulkan and then GL, whichever can present to the window.
  #[default]
  Auto,
  /// Vulkan only. Useful when a broken GL driver wins the automatic order.
  Vulkan,
  /// OpenGL only. The usual rescue on older hardware and some remote sessions.
  Gl,
  /// Keep the GPU pipeline but restrict it to CPU adapters such as lavapipe or
  /// llvmpipe. The caller still falls back to softbuffer if none is present.
  Software,
}

impl GpuBackend {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "vulkan" | "vk" => Some(Self::Vulkan),
      "gl" | "opengl" | "gles" => Some(Self::Gl),
      "software" | "sw" => Some(Self::Software),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Vulkan => "vulkan",
      Self::Gl => "gl",
      Self::Software => "software",
    }
  }
}

/// Which class of adapter the compositor should prefer.
///
/// `Auto` keeps every surface-capable adapter. The other values are a hard
/// filter, not a hint: `device = "discrete"` on a laptop with only integrated
/// graphics produces a readable error instead of silently using the iGPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuDevice {
  /// Keep every surface-capable adapter and let `power` decide the order.
  #[default]
  Auto,
  /// A graphics card with its own memory, usually the fastest option.
  Discrete,
  /// Shared-memory graphics built into the CPU package.
  Integrated,
  /// A virtualised or hosted GPU, common inside VMs.
  Virtual,
  /// A CPU implementation such as lavapipe or llvmpipe.
  Cpu,
}

impl GpuDevice {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "discrete" => Some(Self::Discrete),
      "integrated" => Some(Self::Integrated),
      "virtual" => Some(Self::Virtual),
      "cpu" | "software" => Some(Self::Cpu),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Discrete => "discrete",
      Self::Integrated => "integrated",
      Self::Virtual => "virtual",
      Self::Cpu => "cpu",
    }
  }
}

/// How the compositor ranks adapters when `adapter = "auto"`.
///
/// Low power is the default because a greeter spends almost all its life idle.
/// It is a preference, not a filter: when the preferred class is missing the
/// next one in the ranking is still used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuPower {
  /// Prefer integrated, then other, virtual, discrete, and finally CPU.
  #[default]
  Low,
  /// Prefer discrete, then integrated, virtual, other, and finally CPU.
  High,
  /// Keep the order the backend enumerated, which is useful for debugging.
  Default,
}

impl GpuPower {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "low" => Some(Self::Low),
      "high" => Some(Self::High),
      "default" => Some(Self::Default),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Low => "low",
      Self::High => "high",
      Self::Default => "default",
    }
  }
}

/// The swapchain presentation mode requested from the surface.
///
/// `Fifo` is the only mode every backend must support, so it is the default.
/// A mode the adapter does not advertise is ignored with a warning rather than
/// turning into a startup failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuPresent {
  /// Vsync-style queueing with no tearing.
  #[default]
  Fifo,
  /// Tear-free but replaces the queued frame instead of blocking, when offered.
  Mailbox,
  /// Present immediately and allow tearing, when offered.
  Immediate,
}

impl GpuPresent {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "fifo" | "vsync" => Some(Self::Fifo),
      "mailbox" => Some(Self::Mailbox),
      "immediate" | "tearing" => Some(Self::Immediate),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Fifo => "fifo",
      Self::Mailbox => "mailbox",
      Self::Immediate => "immediate",
    }
  }
}

/// What to do when the requested GPU path cannot start.
///
/// The default chain is the safe policy for a greeter: try the configured
/// adapter, then the software adapter, then softbuffer. `None` is for admins
/// who would rather drop to the terminal than show a CPU-painted window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuFallback {
  /// Configured adapter, then software GPU, then softbuffer.
  #[default]
  Chain,
  /// Configured adapter, then software GPU, then fail to the terminal.
  Software,
  /// Skip the GPU entirely and use softbuffer immediately.
  Cpu,
  /// No fallback: a GPU failure goes straight to the terminal fallback.
  None,
}

impl GpuFallback {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "chain" => Some(Self::Chain),
      "software" => Some(Self::Software),
      "cpu" => Some(Self::Cpu),
      "none" => Some(Self::None),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Chain => "chain",
      Self::Software => "software",
      Self::Cpu => "cpu",
      Self::None => "none",
    }
  }
}

/// How the live window chooses and initialises its GPU compositor.
///
/// Screenshots and the CPU compositor still run on tiny-skia, so this table
/// never changes the pixels in a `--screenshot` file. It only decides which
/// adapter the interactive window uses and what happens when that choice is
/// unavailable at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuConfig {
  /// Set false to use softbuffer without touching a driver. `DAWN_GPU=0` is
  /// the environment equivalent.
  pub enabled: bool,
  /// Graphics API to enable during adapter enumeration.
  pub backend: GpuBackend,
  /// `"auto"`, `"index:N"` into the filtered adapter list, or a
  /// case-insensitive substring of the adapter name or driver.
  pub adapter: String,
  /// Adapter class filter. `backend = "software"` forces this to `Cpu`.
  pub device: GpuDevice,
  /// Ranking preference when `adapter = "auto"`.
  pub power: GpuPower,
  /// Requested swapchain mode. Unsupported modes fall back without failing.
  pub present: GpuPresent,
  /// What to try when the selected adapter cannot produce a device or surface.
  pub fallback: GpuFallback,
}

impl Default for GpuConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      backend: GpuBackend::Auto,
      adapter: "auto".to_string(),
      device: GpuDevice::Auto,
      power: GpuPower::Low,
      present: GpuPresent::Fifo,
      fallback: GpuFallback::Chain,
    }
  }
}

impl GpuConfig {
  /// Applies one `DAWN_GPU` value without touching the process environment.
  ///
  /// This exists as a pure method because `std::env::set_var` is unsafe in the
  /// 2024 edition and tests should not race the rest of the test binary. The
  /// accepted spellings are intentionally small: `0`/`cpu` disables the GPU,
  /// a backend name selects that backend, `auto` clears a previous adapter
  /// choice, and anything else is treated as an adapter name.
  pub fn apply_env_value(&mut self, value: Option<&str>) {
    let Some(value) = value else {
      return;
    };
    let value = value.trim();
    if value.is_empty() {
      return;
    }
    if value == "0" || value.eq_ignore_ascii_case("cpu") {
      self.enabled = false;
      return;
    }
    if value.eq_ignore_ascii_case("auto") {
      self.adapter = "auto".to_string();
      return;
    }
    let normalised = value.to_ascii_lowercase();
    if let Some(backend) = GpuBackend::parse(&normalised) {
      self.backend = backend;
      if backend == GpuBackend::Software {
        self.device = GpuDevice::Cpu;
        self.fallback = GpuFallback::Software;
      }
      return;
    }
    self.adapter = value.to_string();
  }

  /// Reads `DAWN_GPU` once and applies it with [`Self::apply_env_value`].
  ///
  /// Non-Unicode values are ignored on purpose: a broken environment variable
  /// should not keep the greeter from starting.
  pub fn apply_env(&mut self) {
    let value = std::env::var("DAWN_GPU").ok();
    self.apply_env_value(value.as_deref());
  }

  /// One-line lowercase summary shared by `--check-config` and `--list-gpus`.
  /// Using the same method in both places keeps the admin-facing spelling in
  /// sync with the values accepted in TOML.
  #[must_use]
  pub fn summary(&self) -> String {
    format!(
      "enabled={} backend={} adapter={} device={} power={} present={} fallback={}",
      self.enabled,
      self.backend.as_str(),
      self.adapter,
      self.device.as_str(),
      self.power.as_str(),
      self.present.as_str(),
      self.fallback.as_str(),
    )
  }
}

#[derive(Debug, Clone)]
pub struct SessionsConfig {
  pub dirs: Vec<PathBuf>,
  /// Desktop file id to preselect. Unknown ids fall back to the first session.
  pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginsConfig {
  pub enabled: bool,
  pub dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
  /// Empty means "use GREETD_SOCK".
  pub socket: Option<PathBuf>,
  pub timeout: Duration,
}

impl Default for Config {
  fn default() -> Self {
    let theme = ThemePreset::GlassDark.tokens();
    Self {
      ui: UiConfig {
        font: "Inter".to_string(),
        font_size: 17.0,
        card_width: 380.0,
        fullscreen: true,
        greeting: String::new(),
      },
      login: LoginConfig {
        layout: LoginLayout::Card,
        placement: Placement {
          anchor: Anchor::Center,
          offset: (0, 0),
          monitor: None,
          rotation: 0.0,
        },
        width: 380.0,
        height: 0.0,
        padding: 26.0,
        gap: 14.0,
        radius: 18.0,
        panel: None,
        panel_fade: (1.0, 1.0),
        border: None,
        surface: SurfaceStyle::Glass,
        show_shadow: true,
        show_border: true,
        rotation: 0.0,
        draggable: false,
        greeting: String::new(),
        show_hints: true,
        show_picker: true,
        picker_title: "Choose a session".to_string(),
        busy_text: "Signing in\u{2026}".to_string(),
        dry_run_badge: "DRY RUN".to_string(),
        show_dry_run_badge: true,
        no_sessions: "No desktop sessions found. Install one or set [sessions].dirs.".to_string(),
        field: FieldConfig::default_username(),
        username: FieldConfig::default_username(),
        password: FieldConfig::default_password(),
        session: SessionRowConfig {
          label: "Session".to_string(),
          change_hint: "Tab to change".to_string(),
          empty: "No sessions found".to_string(),
          show: true,
        },
        hints: HintConfig {
          username: "Enter  Continue      Tab  Password      Esc  Clear".to_string(),
          password: "Enter  Sign in      Tab  Session      Esc  Clear".to_string(),
          picker: "Up/Down  Choose      Enter  Select      Esc  Cancel".to_string(),
          busy: String::new(),
        },
        tab: String::new(),
        form_offset: (0, 0),
        form_width: 0.0,
        button: ButtonConfig::default(),
        decor: Vec::new(),
      },
      clock: ClockConfig {
        enabled: true,
        style: ClockStyle::Digital,
        layer: WidgetLayer::Behind,
        look: WidgetLook::default(),
        time_format: "%H:%M".to_string(),
        date_format: "%A, %B %d".to_string(),
        hour12: false,
        seconds: false,
        timezone: None,
        posix_locale: false,
        placement: Placement {
          anchor: Anchor::TopCenter,
          offset: (0, 48),
          monitor: None,
          rotation: 0.0,
        },
        size: 54.0,
        date_size: 18.0,
        color: None,
        date_color: None,
        ghost: false,
        glow: None,
        glow_size: 0.0,
        colon_blink_ms: 0.0,
        second_hand: SecondHand::Tick,
        draggable: true,
      },
      media: MediaConfig {
        enabled: true,
        placement: Placement {
          anchor: Anchor::BottomLeft,
          offset: (32, 32),
          monitor: None,
          rotation: 0.0,
        },
        size: 16.0,
        show_art: true,
        look: MediaLook::default(),
        layer: WidgetLayer::Behind,
        presentation: WidgetLook::default(),
        color: None,
        show_before_login: true,
        keep_last: true,
        draggable: true,
      },
      background: BackgroundConfig {
        kind: BackgroundKind::Auto,
        color: theme.preset.background_color(),
        gradient: true,
        image: None,
        fit: Fit::Cover,
        // Zero means "follow the source". A positive value is still honoured
        // as a cap for people who want to save power on a slow panel.
        fps: 0,
        memory_limit: 128 * 1024 * 1024,
        idle_pause: Duration::from_secs(45),
        monitors: BTreeMap::new(),
      },
      theme,
      motion: MotionConfig::default(),
      power: PowerConfig::default(),
      gpu: GpuConfig::default(),
      layout: LayoutConfig {
        save: false,
        file: None,
      },
      sessions: SessionsConfig {
        dirs: vec![
          PathBuf::from("/usr/share/wayland-sessions"),
          PathBuf::from("/usr/share/xsessions"),
        ],
        default: None,
      },
      plugins: PluginsConfig {
        enabled: true,
        dirs: default_plugin_dirs(),
      },
      auth: AuthConfig {
        socket: None,
        timeout: Duration::from_secs(120),
      },
    }
  }
}

/// Returns the system plugins directory plus the user's, when HOME exists.
/// Missing HOME is not an error: a greeter running as root with a minimal
/// environment should simply not look in a nonexistent directory.
fn default_plugin_dirs() -> Vec<PathBuf> {
  let mut dirs = vec![PathBuf::from("/usr/share/dawn/plugins")];
  if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
    dirs.push(PathBuf::from(home).join(".config/dawn/plugins"));
  }
  dirs
}

/// Loads config, always returning a usable configuration.
#[must_use]
pub fn load(explicit: Option<&Path>) -> LoadedConfig {
  let path = explicit.map(Path::to_path_buf).or_else(discover);
  match path {
    Some(path) => load_path(&path, true),
    None => LoadedConfig {
      config: Config::default(),
      path: None,
      issues: Vec::new(),
      package: None,
    },
  }
}

/// Loads a complete config from a theme bundle.
///
/// A bundle cannot select another bundle. That keeps `dawn add` validation
/// finite and makes every installed theme self-contained.
#[must_use]
pub fn load_theme(path: &Path) -> LoadedConfig {
  load_path(path, false)
}

fn load_path(path: &Path, allow_package: bool) -> LoadedConfig {
  let source = match read_bounded(path) {
    Ok(source) => source,
    Err(message) => {
      return LoadedConfig {
        config: Config::default(),
        path: Some(path.to_path_buf()),
        issues: vec![Issue {
          key: "config".to_string(),
          location: None,
          message: format!("{}: {message}; using built-in defaults", path.display()),
        }],
        package: None,
      };
    }
  };

  let raw: RawConfig = match toml::from_str(&source) {
    Ok(raw) => raw,
    Err(error) => {
      let location = error.span().and_then(|span| {
        location_of(&source, span.start).map(|(line, column)| Location {
          file: path.to_path_buf(),
          line,
          column,
        })
      });
      return LoadedConfig {
        config: Config::default(),
        path: Some(path.to_path_buf()),
        issues: vec![Issue {
          key: "config".to_string(),
          location,
          message: format!("{}; using built-in defaults", error.message()),
        }],
        package: None,
      };
    }
  };

  let mut early = Vec::new();
  let mut package = None;
  if let Some(field) = &raw.theme.package {
    let location = field.span().start;
    let location = location_of(&source, location).map(|(line, column)| Location {
      file: path.to_path_buf(),
      line,
      column,
    });
    match field.get_ref() {
      toml::Value::String(id) if !id.trim().is_empty() => {
        if allow_package {
          package = Some(id.clone());
        } else {
          early.push(Issue {
            key: "theme.package".to_string(),
            location,
            message: "a theme bundle cannot select another theme".to_string(),
          });
        }
      }
      _ => early.push(Issue {
        key: "theme.package".to_string(),
        location,
        message: "expected a theme id string".to_string(),
      }),
    }
  }

  let base = path.parent().map(Path::to_path_buf);
  let mut validator = Validator::new(path, &source);
  let config = validator.build(raw, base.as_deref());
  let mut issues = early;
  issues.extend(validator.into_issues());

  LoadedConfig {
    config,
    path: Some(path.to_path_buf()),
    issues,
    package,
  }
}

fn discover() -> Option<PathBuf> {
  if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
    let candidate = PathBuf::from(xdg).join("dawn").join("config.toml");
    if candidate.is_file() {
      return Some(candidate);
    }
  }
  if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
    let candidate = PathBuf::from(home).join(".config/dawn/config.toml");
    if candidate.is_file() {
      return Some(candidate);
    }
  }
  let system = PathBuf::from("/etc/dawn/config.toml");
  system.is_file().then_some(system)
}

/// Config is read once at boot. A cap keeps a mistaken `--config /dev/zero`
/// from turning into an out-of-memory at the worst possible moment.
fn read_bounded(path: &Path) -> Result<String, String> {
  const MAX_BYTES: u64 = 1024 * 1024;
  let metadata = std::fs::metadata(path).map_err(|error| format!("cannot read config: {error}"))?;
  if !metadata.is_file() {
    return Err("not a regular file".to_string());
  }
  if metadata.len() > MAX_BYTES {
    return Err(format!(
      "file is {} bytes, larger than the {MAX_BYTES} byte limit",
      metadata.len()
    ));
  }
  std::fs::read_to_string(path).map_err(|error| format!("cannot read config: {error}"))
}

/// Converts a byte offset into a 1-based line and column for error messages.
pub(crate) fn location_of(source: &str, offset: usize) -> Option<(usize, usize)> {
  let prefix = source.get(..offset).unwrap_or(source);
  let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
  let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
  let column = prefix
    .get(line_start..)
    .map(|text| text.chars().count() + 1)
    .unwrap_or(1);
  Some((line, column))
}

#[cfg(test)]
mod tests {
  use super::{BackgroundKind, Config, ThemePreset, load};
  use crate::layout::{Anchor, Fit};
  use std::path::PathBuf;

  fn temp_config(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dawn-config-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, contents).unwrap();
    path
  }

  #[test]
  fn no_file_means_pure_defaults() {
    let loaded = load(Some(&PathBuf::from("/nonexistent/dawn/config.toml")));
    assert!(loaded.issues.iter().any(|issue| issue.key == "config"));
    assert_eq!(loaded.config.ui.font_size, 17.0);
    assert_eq!(loaded.config.theme.preset, ThemePreset::GlassDark);
    assert!(loaded.config.theme.blur.enabled);
    assert!(loaded.config.theme.blur.size > 0.0);
  }

  #[test]
  fn valid_values_are_applied() {
    let path = temp_config(
      "valid",
      r##"
[ui]
font_size = 22
card_width = 420.5
greeting = "Hello there"

[clock]
time_format = "%H:%M:%S"
date_format = ""
seconds = true
hour12 = true
style = "analog"
anchor = "top-left"
offset = [10, 20]

[background]
kind = "image"
fit = "contain"
fps = 24
memory_limit_mb = 64
idle_pause_secs = 10

[background.monitors.DP-1]
fit = "fill"

[theme]
accent = "#ff8800"
preset = "minimal"
corner_radius = 9

[media]
enabled = false
anchor = "bottom-right"
size = 18

[motion]
boot_ms = 100
decor_fps = 24
[power]
low_power = true
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.config.ui.font_size, 22.0);
    assert_eq!(loaded.config.ui.card_width, 420.5);
    assert_eq!(loaded.config.clock.date_format, "");
    assert!(loaded.config.clock.seconds);
    assert!(loaded.config.clock.hour12);
    assert_eq!(loaded.config.clock.style, super::ClockStyle::Analog);
    assert_eq!(loaded.config.clock.placement.anchor, Anchor::TopLeft);
    assert_eq!(loaded.config.background.kind, BackgroundKind::Image);
    assert_eq!(loaded.config.background.fit, Fit::Contain);
    assert_eq!(loaded.config.background.fps, 24);
    assert_eq!(loaded.config.background.memory_limit, 64 * 1024 * 1024);
    assert_eq!(
      loaded.config.background.for_monitor(Some("DP-1")).fit,
      Fit::Fill
    );
    assert_eq!(loaded.config.theme.preset, ThemePreset::Minimal);
    assert_eq!(loaded.config.theme.corner_radius, 9.0);
    assert_eq!(
      loaded.config.theme.accent,
      crate::color::Rgb::solid(255, 136, 0)
    );
    assert!(!loaded.config.media.enabled);
    assert_eq!(loaded.config.media.placement.anchor, Anchor::BottomRight);
    assert!(loaded.config.power.low_power);
    assert_eq!(loaded.config.motion.boot.as_millis(), 100);
    assert_eq!(loaded.config.motion.decor_fps, 24);
  }

  #[test]
  fn login_form_and_layout_are_configurable() {
    let path = temp_config(
      "login",
      r##"
[ui]
greeting = "Old greeting"
card_width = 410

[login]
anchor = "top-left"
offset = [20, 30]
width = 500
padding = 40
gap = 8
radius = 12
draggable = true
greeting = "Hello"
picker_title = "Pick a desktop"
busy_text = "Working"
dry_run_badge = "TEST"
show_hints = false
show_session = false
show_picker = false
no_sessions = "Nothing to start"

[login.field]
height = 40
radius = 8
background = "#112233"
caret_width = 3

[login.username]
placeholder = "User"
label = "Account"
text = "#ffffff"

[login.password]
mask = "*"
placeholder_color = "#778899"

[login.session]
label = "Desktop"
empty = "No desktop"
change_hint = "Press Tab"

[login.hints]
username = "Enter!"
password = "Sign in!"
picker = "Choose!"
busy = "Please wait"

[layout]
save = true
file = "state/layout.toml"
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let login = &loaded.config.login;
    assert_eq!(login.placement.anchor, Anchor::TopLeft);
    assert_eq!(login.placement.offset, (20, 30));
    assert_eq!(login.width, 500.0);
    assert_eq!(login.padding, 40.0);
    assert_eq!(login.gap, 8.0);
    assert_eq!(login.radius, 12.0);
    assert!(login.draggable);
    assert_eq!(login.greeting, "Hello");
    assert_eq!(login.picker_title, "Pick a desktop");
    assert_eq!(login.busy_text, "Working");
    assert_eq!(login.dry_run_badge, "TEST");
    assert!(!login.show_hints);
    assert!(!login.session.show);
    assert!(!login.show_picker);
    assert_eq!(login.no_sessions, "Nothing to start");
    assert_eq!(login.field.height, 40.0);
    assert_eq!(login.field.radius, 8.0);
    assert_eq!(login.field.caret_width, 3.0);
    assert_eq!(login.username.label, "Account");
    assert_eq!(login.username.placeholder, "User");
    assert_eq!(login.username.height, 40.0);
    assert_eq!(login.password.mask, "*");
    assert_eq!(login.session.label, "Desktop");
    assert_eq!(login.hints.username, "Enter!");
    assert!(loaded.config.layout.save);
    let expected = path.parent().unwrap().join("state").join("layout.toml");
    assert_eq!(
      loaded.config.layout.file.as_deref(),
      Some(expected.as_path())
    );
  }

  #[test]
  fn hero_layout_button_and_decor_are_configurable() {
    let path = temp_config(
      "hero",
      r##"
[login]
layout = "hero"
width = 1160
height = 663
padding = 0
panel = "#ff4655e6"
panel_fade = [0.38, 0.62]
border = "#ffffff33"
tab = "Login"
form_offset = [610, 246]
form_width = 495
show_session = false

[login.username]
label = "Account"
label_color = "#22ff88"
underline = "#ff0000"
underline_size = 2.5
placeholder = "User Name"
icon = "user"
icon_size = 20
icon_gap = 12
background = "#ffffff73"
background_end = "#ffffff33"
bracket = "#0f1923"
bracket_gap = 12

[login.button]
show = true
text = "GET STARTED"
gap = 42
height = 48
radius = 0
font_size = 19
bold = true
tracking = 0.08
background = "#ff4655"
text_color = "#0f1923"

[[login.decor]]
kind = "image"
image = "logo.png"
width = 174
anchor = "top-left"
offset = [430, 42]

[[login.decor]]
kind = "text"
text = "Footer"
size = 12
color = "#ffffff"
anchor = "bottom-right"
offset = [20, 20]

[login.decor.motion]
spin = 8.0
sway = 4.0
sway_ms = 5000
float = [14, -10]
float_ms = 7000
pulse = 0.06
pulse_ms = 2200
fade = 0.25
fade_ms = 3200
phase = 0.25
"##,
    );
    // The decor validator checks that image paths exist and are real files.
    let png = [
      137u8, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
      0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31,
      0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    std::fs::write(path.parent().unwrap().join("logo.png"), png).unwrap();

    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let login = &loaded.config.login;
    assert_eq!(login.layout, super::LoginLayout::Hero);
    assert_eq!(login.height, 663.0);
    assert_eq!(login.panel_fade, (0.38, 0.62));
    assert_eq!(login.tab, "Login");
    assert_eq!(login.form_offset, (610, 246));
    assert_eq!(login.form_width, 495.0);
    assert!(login.button.show);
    assert_eq!(login.button.text, "GET STARTED");
    assert!(login.button.bold);
    assert_eq!(login.button.tracking, 0.08);
    assert_eq!(login.username.icon, "user");
    assert_eq!(login.username.icon_size, 20.0);
    assert_eq!(login.username.bracket_gap, 12.0);
    assert_eq!(login.username.label, "Account");
    assert_eq!(
      login.username.label_color,
      Some(crate::color::Rgb::solid(0x22, 0xff, 0x88))
    );
    assert_eq!(
      login.username.underline,
      Some(crate::color::Rgb::solid(0xff, 0, 0))
    );
    assert_eq!(login.username.underline_size, 2.5);
    assert_eq!(
      login.username.background_end,
      Some(crate::color::Rgb::solid(0xff, 0xff, 0xff).with_alpha(0x33))
    );
    assert_eq!(login.decor.len(), 2);
    match &login.decor[0] {
      super::DecorSpec::Image { width, offset, .. } => {
        assert_eq!(*width, 174.0);
        assert_eq!(*offset, (430, 42));
      }
      other => panic!("expected an image decor, got {other:?}"),
    }
    match &login.decor[1] {
      super::DecorSpec::Text {
        text,
        size,
        anchor,
        motion,
        ..
      } => {
        assert_eq!(text, "Footer");
        assert_eq!(*size, 12.0);
        assert_eq!(*anchor, Anchor::BottomRight);
        assert_eq!(motion.spin, 8.0);
        assert_eq!(motion.sway, 4.0);
        assert_eq!(motion.sway_ms, 5000.0);
        assert_eq!(motion.float, (14, -10));
        assert_eq!(motion.float_ms, 7000.0);
        assert_eq!(motion.pulse, 0.06);
        assert_eq!(motion.pulse_ms, 2200.0);
        assert_eq!(motion.fade, 0.25);
        assert_eq!(motion.fade_ms, 3200.0);
        assert_eq!(motion.phase, 0.25);
        assert!(motion.active());
      }
      other => panic!("expected a text decor, got {other:?}"),
    }
  }

  #[test]
  fn decor_motion_is_neutral_at_zero_and_uses_its_phase() {
    let motion = super::DecorMotion {
      spin: 90.0,
      sway: 10.0,
      sway_ms: 4000.0,
      float: (20, -10),
      float_ms: 8000.0,
      pulse: 0.1,
      pulse_ms: 2000.0,
      fade: 0.4,
      fade_ms: 4000.0,
      phase: 0.0,
      typer_ms: 0.0,
      blink_ms: 0.0,
      ticker_ms: 0.0,
      ticker_width: 0,
      ticker_gap: 64,
    };
    let start = motion.sample(0.0);
    assert_eq!(start.offset, (0.0, 0.0));
    assert_eq!(start.angle, 0.0);
    assert_eq!(start.scale, 1.0);
    assert_eq!(start.opacity, 1.0);
    // 90 deg/s of spin plus a 10 degree sway at the top of its 4 s cycle.
    assert!((motion.sample(1.0).angle - 100.0).abs() < 0.01);
    // Pulse peaks a quarter of the way through its 2 s period.
    assert!((motion.sample(0.5).scale - 1.1).abs() < 0.001);
    // Fade reaches its deepest halfway through its 4 s period.
    assert!((motion.sample(2.0).opacity - 0.6).abs() < 0.001);
    // A phase offset moves the cycle forward without moving the start pose.
    let staggered = super::DecorMotion {
      phase: 0.25,
      ..motion
    };
    let shifted = staggered.sample(0.0);
    assert_eq!(shifted.offset, (0.0, 0.0));
    assert_eq!(shifted.angle, 0.0);
    assert!((shifted.scale - 1.0).abs() < 0.001);
    assert!((shifted.opacity - 1.0).abs() < 0.001);
    assert!(!super::DecorMotion::default().active());
  }

  #[test]
  fn decor_typer_reveal_is_linear_and_off_by_default() {
    let motion = super::DecorMotion {
      typer_ms: 1000.0,
      ..super::DecorMotion::default()
    };
    assert_eq!(motion.sample(0.0).reveal, 0.0);
    assert!((motion.sample(0.5).reveal - 0.5).abs() < 0.001);
    assert_eq!(motion.sample(1.0).reveal, 1.0);
    // Beyond the duration the text stays complete instead of restarting.
    assert_eq!(motion.sample(4.0).reveal, 1.0);
    // With the typer off, expect a full reveal at any age.
    assert_eq!(super::DecorMotion::default().sample(99.0).reveal, 1.0);
  }

  #[test]
  fn decor_blink_is_a_square_wave() {
    let motion = super::DecorMotion {
      blink_ms: 1000.0,
      ..super::DecorMotion::default()
    };
    assert!(motion.sample(0.0).blink_on);
    // The second half of the first period is dark.
    assert!(!motion.sample(1.1).blink_on);
    // The first half of the second period is visible again.
    assert!(motion.sample(2.3).blink_on);
    assert!(super::DecorMotion::default().sample(1.1).blink_on);
  }

  #[test]
  fn decor_ticker_loop_fraction_wraps_within_one_tile() {
    let motion = super::DecorMotion {
      ticker_ms: 1000.0,
      ticker_width: 300,
      ..super::DecorMotion::default()
    };
    assert_eq!(motion.sample(0.0).loop_frac, 0.0);
    assert!((motion.sample(0.25).loop_frac - 0.25).abs() < 0.001);
    let wrapped = motion.sample(1.25).loop_frac;
    assert!((wrapped - 0.25).abs() < 0.001);
    assert!((0.0..1.0).contains(&wrapped));
    assert_eq!(motion.sample(2.0).loop_frac, 0.0);
    assert_eq!(super::DecorMotion::default().sample(1.0).loop_frac, 0.0);
  }

  #[test]
  fn decor_motion_active_tracks_the_text_motions() {
    let typer = super::DecorMotion {
      typer_ms: 1.0,
      ..super::DecorMotion::default()
    };
    assert!(typer.active());
    let blink = super::DecorMotion {
      blink_ms: 1.0,
      ..super::DecorMotion::default()
    };
    assert!(blink.active());
    let ticker = super::DecorMotion {
      ticker_ms: 1.0,
      ticker_width: 100,
      ..super::DecorMotion::default()
    };
    assert!(ticker.active());
    // A ticker with no width cannot draw itself, so it must not request
    // animation frames that would only repaint the same pixels.
    let nowhere = super::DecorMotion {
      ticker_ms: 1.0,
      ..super::DecorMotion::default()
    };
    assert!(!nowhere.active());
  }

  #[test]
  fn text_decor_motions_and_block_caret_are_configurable() {
    let path = temp_config(
      "text-motions",
      r##"
[[login.decor]]
kind = "text"
text = "HUD"
[login.decor.motion]
typer_ms = 1200
blink_ms = 900
ticker_ms = 3000
ticker_width = 300
ticker_gap = 80

[login.username]
caret_style = "block"
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    match &loaded.config.login.decor[0] {
      super::DecorSpec::Text { motion, .. } => {
        assert_eq!(motion.typer_ms, 1200.0);
        assert_eq!(motion.blink_ms, 900.0);
        assert_eq!(motion.ticker_ms, 3000.0);
        assert_eq!(motion.ticker_width, 300);
        assert_eq!(motion.ticker_gap, 80);
      }
      other => panic!("expected a text decor, got {other:?}"),
    }
    assert_eq!(
      loaded.config.login.username.caret_style,
      super::CaretStyle::Block
    );
  }

  #[test]
  fn bad_text_motions_and_caret_style_fall_back_with_issues() {
    let path = temp_config(
      "bad-text-motions",
      r##"
[login.field]
caret_style = "sparkle"

[[login.decor]]
kind = "text"
text = "HUD"
[login.decor.motion]
typer_ms = -1
blink_ms = 30000
ticker_ms = 200000
ticker_width = 9999
ticker_gap = 9999
"##,
    );
    let loaded = load(Some(&path));
    let motion = match &loaded.config.login.decor[0] {
      super::DecorSpec::Text { motion, .. } => *motion,
      other => panic!("expected a text decor, got {other:?}"),
    };
    assert_eq!(motion.typer_ms, 0.0);
    assert_eq!(motion.blink_ms, 0.0);
    assert_eq!(motion.ticker_ms, 0.0);
    assert_eq!(motion.ticker_width, 0);
    assert_eq!(motion.ticker_gap, 64);
    assert_eq!(
      loaded.config.login.field.caret_style,
      super::CaretStyle::Bar
    );
    assert_eq!(
      loaded.config.login.username.caret_style,
      super::CaretStyle::Bar
    );
    for key in [
      "login.decor[0].motion.typer_ms",
      "login.decor[0].motion.blink_ms",
      "login.decor[0].motion.ticker_ms",
      "login.decor[0].motion.ticker_width",
      "login.decor[0].motion.ticker_gap",
      "login.field.caret_style",
    ] {
      assert!(
        loaded.issues.iter().any(|issue| issue.key == key),
        "missing issue {key}: {:?}",
        loaded.issues
      );
    }
  }

  #[test]
  fn a_ticker_without_a_width_is_reported() {
    let path = temp_config(
      "ticker-width",
      r##"
[[login.decor]]
kind = "text"
text = "Breaking news"
[login.decor.motion]
ticker_ms = 3000
"##,
    );
    let loaded = load(Some(&path));
    let issue = loaded
      .issues
      .iter()
      .find(|issue| issue.key == "login.decor[0].motion.ticker_width")
      .unwrap_or_else(|| panic!("expected a ticker_width issue, got {:?}", loaded.issues));
    assert!(issue.message.contains("ticker_width"), "{issue:?}");
  }

  #[test]
  fn caret_style_defaults_to_bar() {
    let config = Config::default();
    assert_eq!(config.login.field.caret_style, super::CaretStyle::Bar);
    assert_eq!(config.login.username.caret_style, super::CaretStyle::Bar);
    assert_eq!(config.login.password.caret_style, super::CaretStyle::Bar);
  }

  #[test]
  fn surface_rotation_button_effects_and_blur_are_configurable() {
    let path = temp_config(
      "effects",
      r##"
[login]
surface = "none"
show_shadow = false
show_border = false
rotation = 7.5

[login.field]
rotation = -3

[login.username]
placeholder = "User"

[login.button]
show = true
text = "PLAY"
background = "#112233"
background_end = "#445566"
gradient_angle = 90
hover_background = "#778899"
hover_background_end = "#aabbcc"
hover_text_color = "#ffffff"
hover_direction = "left"
hover_ms = 240
hover_scale = 1.05
hover_lift = 3
glow = "#ff4655"
glow_size = 12
effect = "sweep"
effect_color = "#ffffff"
effect_ms = 1800
effect_angle = 115
effect_strength = 0.4
rotation = 6

[[login.decor]]
kind = "text"
text = "Sideways"
anchor = "top-left"
offset = [10, 10]
rotation = -20

[theme.blur]
enabled = true
size = 12
passes = 4
noise = 0.02
contrast = 0.9
brightness = 0.8
vibrancy = 0.2
vibrancy_darkness = 0.1
refresh_ms = 400

[clock]
rotation = 5

[media]
rotation = -5
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let login = &loaded.config.login;
    assert_eq!(login.surface, super::SurfaceStyle::None);
    assert!(!login.show_shadow);
    assert!(!login.show_border);
    assert_eq!(login.rotation, 7.5);
    assert_eq!(login.username.rotation, -3.0);
    let button = &login.button;
    assert_eq!(
      button.background_end,
      Some(crate::color::Rgb::solid(0x44, 0x55, 0x66))
    );
    assert_eq!(button.gradient_angle, 90.0);
    assert_eq!(button.hover_direction, super::HoverReveal::Left);
    assert_eq!(button.hover_ms, 240.0);
    assert_eq!(button.hover_scale, 1.05);
    assert_eq!(button.hover_lift, 3.0);
    assert_eq!(button.effect, super::ButtonEffect::Sweep);
    assert_eq!(button.effect_ms, 1800.0);
    assert_eq!(button.effect_angle, 115.0);
    assert_eq!(button.effect_strength, 0.4);
    assert_eq!(button.rotation, 6.0);
    match &login.decor[0] {
      super::DecorSpec::Text { rotation, .. } => assert_eq!(*rotation, -20.0),
      other => panic!("expected text decor, got {other:?}"),
    }
    let blur = &loaded.config.theme.blur;
    assert!(blur.enabled);
    assert_eq!(blur.size, 12.0);
    assert_eq!(blur.passes, 4);
    assert!((blur.noise - 0.02).abs() < f32::EPSILON);
    assert!((blur.contrast - 0.9).abs() < f32::EPSILON);
    assert!((blur.brightness - 0.8).abs() < f32::EPSILON);
    assert!((blur.vibrancy - 0.2).abs() < f32::EPSILON);
    assert!((blur.vibrancy_darkness - 0.1).abs() < f32::EPSILON);
    assert_eq!(blur.refresh, std::time::Duration::from_millis(400));
    assert_eq!(loaded.config.clock.placement.rotation, 5.0);
    assert_eq!(loaded.config.media.placement.rotation, -5.0);
  }

  #[test]
  fn legacy_ui_aliases_still_supply_login_defaults() {
    let path = temp_config(
      "legacy-login",
      "[ui]\ngreeting = \"Hi\"\ncard_width = 410\n",
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.config.login.greeting, "Hi");
    assert_eq!(loaded.config.login.width, 410.0);
  }

  #[test]
  fn a_bad_field_falls_back_without_discarding_good_ones() {
    let path = temp_config(
      "partial",
      r#"
[ui]
font_size = "huge"
card_width = 500

[clock]
time_format = "%E"

[background]
kind = "videoo"
fps = 500
"#,
    );
    let loaded = load(Some(&path));
    assert_eq!(loaded.config.ui.font_size, 17.0);
    assert_eq!(loaded.config.ui.card_width, 500.0);
    assert_eq!(loaded.config.clock.time_format, "%H:%M");
    assert_eq!(loaded.config.background.kind, BackgroundKind::Auto);
    assert_eq!(loaded.config.background.fps, 0);
    assert_eq!(loaded.issues.len(), 4, "{:?}", loaded.issues);
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "ui.font_size")
    );
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "background.kind")
    );
  }

  #[test]
  fn a_syntax_error_still_yields_defaults() {
    let path = temp_config("syntax", "[ui\nfont_size = 20\n");
    let loaded = load(Some(&path));
    assert_eq!(loaded.config.ui.font_size, 17.0);
    assert_eq!(loaded.issues.len(), 1);
    assert_eq!(loaded.issues[0].key, "config");
    assert!(loaded.issues[0].location.is_some());
  }

  #[test]
  fn unknown_keys_are_reported_with_a_line() {
    let path = temp_config("unknown", "[ui]\nfont_sizee = 20\n");
    let loaded = load(Some(&path));
    assert_eq!(loaded.issues.len(), 1);
    assert!(loaded.issues[0].to_string().contains("font_sizee"));
    assert!(loaded.issues[0].to_string().contains(":2:"));
  }

  #[test]
  fn empty_session_list_is_rejected_in_favour_of_defaults() {
    let path = temp_config("sessions", "[sessions]\ndirs = []\n");
    let loaded = load(Some(&path));
    assert!(!loaded.config.sessions.dirs.is_empty());
    assert!(loaded.issues.iter().any(|i| i.key == "sessions.dirs"));
  }

  #[test]
  fn light_preset_supplies_a_light_background() {
    let path = temp_config("light", "[theme]\npreset = \"glass-light\"\n");
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(
      loaded.config.background.color,
      ThemePreset::GlassLight.background_color()
    );
  }

  #[test]
  fn bundled_defaults_are_sane() {
    let config = Config::default();
    assert!(config.ui.fullscreen);
    assert!(config.auth.timeout.as_secs() >= 5);
    assert!(config.plugins.dirs.iter().any(|dir| dir.is_absolute()));
    assert!(config.clock.enabled);
    assert!(config.media.enabled);
    assert!(config.motion.enabled);
    assert!(!config.power.low_power);
  }

  #[test]
  fn per_monitor_issues_name_the_exact_table() {
    // The admin needs to know which output override is wrong, not just that
    // "background.monitors" has a problem somewhere.
    let path = temp_config(
      "monitors-bad",
      "[background]\nkind = \"image\"\nimage = \"wall.png\"\n\n[background.monitors.DP-1]\nfit = \"stretch\"\n",
    );
    let loaded = load(Some(&path));
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "background.monitors.DP-1.fit"),
      "issue keys were {:?}",
      loaded
        .issues
        .iter()
        .map(|issue| issue.key.as_str())
        .collect::<Vec<_>>()
    );
    // The invalid override falls back without discarding the valid base.
    assert_eq!(loaded.config.background.fit, crate::layout::Fit::Cover);
  }

  #[test]
  fn built_in_widgets_can_paint_in_front_of_decor() {
    let path = temp_config(
      "widget-layer-front",
      r##"
[clock]
layer = "front"

[media]
layer = "front"
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.config.clock.layer, super::WidgetLayer::Front);
    assert_eq!(loaded.config.media.layer, super::WidgetLayer::Front);
    assert_eq!(Config::default().clock.layer, super::WidgetLayer::Behind);
    assert_eq!(Config::default().media.layer, super::WidgetLayer::Behind);
  }

  #[test]
  fn an_unknown_widget_layer_falls_back_behind() {
    let path = temp_config(
      "widget-layer-bad",
      r##"
[clock]
layer = "middle"

[media]
layer = 3
"##,
    );
    let loaded = load(Some(&path));
    assert_eq!(loaded.config.clock.layer, super::WidgetLayer::Behind);
    assert_eq!(loaded.config.media.layer, super::WidgetLayer::Behind);
    assert!(loaded.issues.iter().any(|issue| issue.key == "clock.layer"));
    assert!(loaded.issues.iter().any(|issue| issue.key == "media.layer"));
  }

  #[test]
  fn widget_looks_default_to_the_historical_scrim() {
    let config = Config::default();
    let look = &config.clock.look;
    assert_eq!(look.plate.style, super::PlateStyle::Tint);
    assert_eq!(look.plate.radius, 14.0);
    assert_eq!(look.plate.padding, 12.0);
    assert_eq!(look.plate.opacity, 1.0);
    assert_eq!(look.plate.gradient_angle, 0.0);
    assert_eq!(look.plate.glow_size, 0.0);
    assert!(look.plate.color.is_none());
    assert!(look.plate.color_end.is_none());
    assert!(look.plate.border.is_none());
    assert!(look.plate.glow.is_none());
    assert!(!look.plate.shadow);
    assert_eq!(look.motion, super::DecorMotion::default());
    assert!(look.show_on.is_empty());
    assert!(look.hide_on.is_empty());
    // Media gets the same defaults, so an unconfigured widget of either kind
    // looks exactly like it did before plates existed.
    assert_eq!(config.media.presentation, config.clock.look);
  }

  #[test]
  fn clock_plate_motion_and_state_lists_parse_into_the_look() {
    let path = temp_config(
      "clock-look",
      r##"
[clock]
show_on = ["typing", "busy"]
hide_on = ["idle"]

[clock.plate]
style = "gradient"
color = "#112233"
color_end = "#445566"
gradient_angle = 45
radius = 8
border = "#ffffff"
padding = 20
glow = "#00ff00"
glow_size = 12
shadow = true
opacity = 0.75

[clock.motion]
spin = 90
float = [4, 2]
blink_ms = 1000
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let look = &loaded.config.clock.look;
    assert_eq!(look.plate.style, super::PlateStyle::Gradient);
    assert_eq!(
      look.plate.color,
      Some(crate::color::Rgb::solid(0x11, 0x22, 0x33))
    );
    assert_eq!(
      look.plate.color_end,
      Some(crate::color::Rgb::solid(0x44, 0x55, 0x66))
    );
    assert_eq!(look.plate.gradient_angle, 45.0);
    assert_eq!(look.plate.radius, 8.0);
    assert_eq!(
      look.plate.border,
      Some(crate::color::Rgb::solid(0xff, 0xff, 0xff))
    );
    assert_eq!(look.plate.padding, 20.0);
    assert_eq!(
      look.plate.glow,
      Some(crate::color::Rgb::solid(0x00, 0xff, 0x00))
    );
    assert_eq!(look.plate.glow_size, 12.0);
    assert!(look.plate.shadow);
    assert_eq!(look.plate.opacity, 0.75);
    assert_eq!(look.motion.spin, 90.0);
    assert_eq!(look.motion.float, (4, 2));
    assert_eq!(look.motion.blink_ms, 1000.0);
    assert_eq!(
      look.show_on,
      [super::WidgetState::Typing, super::WidgetState::Busy]
    );
    assert_eq!(look.hide_on, [super::WidgetState::Idle]);
  }

  #[test]
  fn invalid_plate_values_fall_back_with_issues() {
    let path = temp_config(
      "clock-plate-bad",
      r##"
[clock.plate]
style = "fancy"
radius = 99
padding = -1
gradient_angle = 999
opacity = 2
glow_size = 99
"##,
    );
    let loaded = load(Some(&path));
    let plate = &loaded.config.clock.look.plate;
    assert_eq!(plate.style, super::PlateStyle::Tint);
    assert_eq!(plate.radius, 14.0);
    assert_eq!(plate.padding, 12.0);
    assert_eq!(plate.gradient_angle, 0.0);
    assert_eq!(plate.opacity, 1.0);
    assert_eq!(plate.glow_size, 0.0);
    for key in [
      "clock.plate.style",
      "clock.plate.radius",
      "clock.plate.padding",
      "clock.plate.gradient_angle",
      "clock.plate.opacity",
      "clock.plate.glow_size",
    ] {
      assert!(
        loaded.issues.iter().any(|issue| issue.key == key),
        "missing issue {key}: {:?}",
        loaded.issues
      );
    }
  }

  #[test]
  fn media_look_parses_and_bad_values_fall_back_with_issues() {
    let path = temp_config(
      "media-look",
      r##"
[media]
layout = "vinyl"
art = "disc"
progress = "wave"
controls = true
tint_from_art = true
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.config.media.look.layout, super::MediaLayout::Vinyl);
    assert_eq!(loaded.config.media.look.art, super::MediaArt::Disc);
    assert_eq!(
      loaded.config.media.look.progress,
      super::MediaProgress::Wave
    );
    assert!(loaded.config.media.look.controls);
    assert!(loaded.config.media.look.tint_from_art);

    let bad = temp_config(
      "media-look-bad",
      r##"
[media]
layout = "wall"
art = "sleeve"
progress = "bars"
controls = "yes"
tint_from_art = 3
"##,
    );
    let loaded = load(Some(&bad));
    let look = loaded.config.media.look;
    assert_eq!(look.layout, super::MediaLayout::Card);
    assert_eq!(look.art, super::MediaArt::Cover);
    assert_eq!(look.progress, super::MediaProgress::Line);
    assert!(!look.controls);
    assert!(!look.tint_from_art);
    for key in [
      "media.layout",
      "media.art",
      "media.progress",
      "media.controls",
      "media.tint_from_art",
    ] {
      assert!(
        loaded.issues.iter().any(|issue| issue.key == key),
        "missing issue {key}: {:?}",
        loaded.issues
      );
    }
  }

  #[test]
  fn clock_decorative_keys_parse_and_fall_back() {
    let path = temp_config(
      "clock-faces",
      r##"
[clock]
style = "segment"
ghost = true
glow = "#22D3EE"
glow_size = 8
colon_blink_ms = 900
second_hand = "sweep"
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.config.clock.style, super::ClockStyle::Segment);
    assert!(loaded.config.clock.ghost);
    assert!(loaded.config.clock.glow.is_some());
    assert_eq!(loaded.config.clock.glow_size, 8.0);
    assert_eq!(loaded.config.clock.colon_blink_ms, 900.0);
    assert_eq!(loaded.config.clock.second_hand, super::SecondHand::Sweep);

    let bad = temp_config(
      "clock-faces-bad",
      r##"
[clock]
style = "sundial"
glow_size = 99
colon_blink_ms = 9000
second_hand = "wiggle"
"##,
    );
    let loaded = load(Some(&bad));
    assert_eq!(loaded.config.clock.style, super::ClockStyle::Digital);
    assert_eq!(loaded.config.clock.glow_size, 0.0);
    assert_eq!(loaded.config.clock.colon_blink_ms, 0.0);
    assert_eq!(loaded.config.clock.second_hand, super::SecondHand::Tick);
    for key in [
      "clock.style",
      "clock.glow_size",
      "clock.colon_blink_ms",
      "clock.second_hand",
    ] {
      assert!(
        loaded.issues.iter().any(|issue| issue.key == key),
        "missing issue {key}: {:?}",
        loaded.issues
      );
    }
  }

  #[test]
  fn widget_state_lists_dedupe_and_report_unknown_names() {
    let path = temp_config(
      "clock-states",
      r##"
[clock]
show_on = ["typing", "typing", "napping"]
hide_on = ["busy", "busy"]

[media]
show_on = "typing"
"##,
    );
    let loaded = load(Some(&path));
    assert_eq!(
      loaded.config.clock.look.show_on,
      [super::WidgetState::Typing]
    );
    assert_eq!(loaded.config.clock.look.hide_on, [super::WidgetState::Busy]);
    let unknown = loaded
      .issues
      .iter()
      .filter(|issue| issue.key == "clock.show_on")
      .count();
    assert_eq!(unknown, 1, "{:?}", loaded.issues);
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "media.show_on")
    );
    assert!(loaded.config.media.presentation.show_on.is_empty());
  }

  #[test]
  fn panel_decor_parses_its_plate_keys() {
    let path = temp_config(
      "panel",
      r##"
[[login.decor]]
kind = "panel"
width = 320
height = 96
color = "#112233"
color_end = "#445566"
gradient_angle = 90
radius = 6
border = "#ffffff"
opacity = 0.5
anchor = "bottom-left"
offset = [10, 20]
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    match &loaded.config.login.decor[0] {
      super::DecorSpec::Panel {
        width,
        height,
        color,
        color_end,
        gradient_angle,
        radius,
        border,
        opacity,
        anchor,
        offset,
        ..
      } => {
        assert_eq!(*width, 320.0);
        assert_eq!(*height, 96.0);
        assert_eq!(*color, Some(crate::color::Rgb::solid(0x11, 0x22, 0x33)));
        assert_eq!(*color_end, Some(crate::color::Rgb::solid(0x44, 0x55, 0x66)));
        assert_eq!(*gradient_angle, 90.0);
        assert_eq!(*radius, 6.0);
        assert_eq!(*border, Some(crate::color::Rgb::solid(0xff, 0xff, 0xff)));
        assert_eq!(*opacity, 0.5);
        assert_eq!(*anchor, Anchor::BottomLeft);
        assert_eq!(*offset, (10, 20));
      }
      other => panic!("expected a panel decor, got {other:?}"),
    }
  }

  #[test]
  fn out_of_range_panel_size_falls_back() {
    let path = temp_config(
      "panel-size",
      r##"
[[login.decor]]
kind = "panel"
width = 1
height = 99999
"##,
    );
    let loaded = load(Some(&path));
    match &loaded.config.login.decor[0] {
      super::DecorSpec::Panel { width, height, .. } => {
        assert_eq!(*width, 320.0);
        assert_eq!(*height, 120.0);
      }
      other => panic!("expected a panel decor, got {other:?}"),
    }
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "login.decor[0].width")
    );
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "login.decor[0].height")
    );
  }

  #[test]
  fn a_panel_with_a_text_payload_reports_one_issue() {
    let path = temp_config(
      "panel-text",
      r##"
[[login.decor]]
kind = "panel"
width = 120
height = 40
text = "ignored"
"##,
    );
    let loaded = load(Some(&path));
    let panel_issues: Vec<_> = loaded
      .issues
      .iter()
      .filter(|issue| issue.key.starts_with("login.decor[0]"))
      .collect();
    assert_eq!(panel_issues.len(), 1, "{:?}", loaded.issues);
    assert_eq!(panel_issues[0].key, "login.decor[0].kind");
    assert!(panel_issues[0].message.contains("bare panel"));
    assert!(matches!(
      &loaded.config.login.decor[0],
      super::DecorSpec::Panel { .. }
    ));
  }

  #[test]
  fn text_stroke_parses_and_defaults_its_width() {
    let path = temp_config(
      "text-stroke",
      r##"
[[login.decor]]
kind = "text"
text = "Outline"
stroke = "#000000"

[[login.decor]]
kind = "text"
text = "No outline"
stroke_width = 99
"##,
    );
    let loaded = load(Some(&path));
    match &loaded.config.login.decor[0] {
      super::DecorSpec::Text {
        stroke,
        stroke_width,
        ..
      } => {
        assert_eq!(*stroke, Some(crate::color::Rgb::solid(0, 0, 0)));
        assert_eq!(*stroke_width, 2.0);
      }
      other => panic!("expected text decor, got {other:?}"),
    }
    match &loaded.config.login.decor[1] {
      super::DecorSpec::Text {
        stroke,
        stroke_width,
        ..
      } => {
        assert_eq!(*stroke, None);
        assert_eq!(*stroke_width, 0.0);
      }
      other => panic!("expected text decor, got {other:?}"),
    }
    assert!(
      loaded
        .issues
        .iter()
        .any(|issue| issue.key == "login.decor[1].stroke_width")
    );
  }

  #[test]
  fn gpu_section_parses_and_defaults() {
    let defaults = super::GpuConfig::default();
    assert_eq!(Config::default().gpu, defaults);
    assert!(defaults.enabled);
    assert_eq!(defaults.backend, super::GpuBackend::Auto);
    assert_eq!(defaults.adapter, "auto");
    assert_eq!(defaults.device, super::GpuDevice::Auto);
    assert_eq!(defaults.power, super::GpuPower::Low);
    assert_eq!(defaults.present, super::GpuPresent::Fifo);
    assert_eq!(defaults.fallback, super::GpuFallback::Chain);

    let path = temp_config(
      "gpu-section",
      r##"
[gpu]
enabled = false
backend = "vulkan"
adapter = "index:2"
device = "discrete"
power = "high"
present = "mailbox"
fallback = "software"
"##,
    );
    let loaded = load(Some(&path));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(
      loaded.config.gpu,
      super::GpuConfig {
        enabled: false,
        backend: super::GpuBackend::Vulkan,
        adapter: "index:2".to_string(),
        device: super::GpuDevice::Discrete,
        power: super::GpuPower::High,
        present: super::GpuPresent::Mailbox,
        fallback: super::GpuFallback::Software,
      }
    );
  }

  #[test]
  fn invalid_gpu_choices_fall_back_with_issues() {
    let path = temp_config(
      "gpu-bad-choices",
      r##"
[gpu]
backend = "metal"
device = "toaster"
power = "medium"
present = "vsync-free"
fallback = "pray"
"##,
    );
    let loaded = load(Some(&path));
    let gpu = &loaded.config.gpu;
    assert_eq!(gpu.backend, super::GpuBackend::Auto);
    assert_eq!(gpu.device, super::GpuDevice::Auto);
    assert_eq!(gpu.power, super::GpuPower::Low);
    assert_eq!(gpu.present, super::GpuPresent::Fifo);
    assert_eq!(gpu.fallback, super::GpuFallback::Chain);
    for key in [
      "gpu.backend",
      "gpu.device",
      "gpu.power",
      "gpu.present",
      "gpu.fallback",
    ] {
      assert!(
        loaded.issues.iter().any(|issue| issue.key == key),
        "missing issue {key}: {:?}",
        loaded.issues
      );
    }
  }

  #[test]
  fn gpu_adapter_index_and_name_rules() {
    let index = load(Some(&temp_config(
      "gpu-adapter-index",
      r#"
[gpu]
adapter = "index:2"
"#,
    )));
    assert!(index.issues.is_empty(), "{:?}", index.issues);
    assert_eq!(index.config.gpu.adapter, "index:2");

    let bad = load(Some(&temp_config(
      "gpu-adapter-bad-index",
      r#"
[gpu]
adapter = "index:x"
"#,
    )));
    assert_eq!(bad.config.gpu.adapter, "auto");
    assert!(
      bad.issues.iter().any(|issue| issue.key == "gpu.adapter"),
      "{:?}",
      bad.issues
    );

    let name = load(Some(&temp_config(
      "gpu-adapter-name",
      r#"
[gpu]
adapter = "RTX"
"#,
    )));
    assert!(name.issues.is_empty(), "{:?}", name.issues);
    assert_eq!(name.config.gpu.adapter, "RTX");
  }

  #[test]
  fn gpu_env_override_is_pure() {
    let mut gpu = super::GpuConfig::default();
    gpu.apply_env_value(None);
    assert_eq!(gpu, super::GpuConfig::default());

    gpu.apply_env_value(Some("0"));
    assert!(!gpu.enabled);

    let mut gpu = super::GpuConfig::default();
    gpu.apply_env_value(Some("vulkan"));
    assert_eq!(gpu.backend, super::GpuBackend::Vulkan);

    let mut gpu = super::GpuConfig::default();
    gpu.apply_env_value(Some("software"));
    assert_eq!(gpu.backend, super::GpuBackend::Software);
    assert_eq!(gpu.device, super::GpuDevice::Cpu);
    assert_eq!(gpu.fallback, super::GpuFallback::Software);

    let mut gpu = super::GpuConfig::default();
    gpu.apply_env_value(Some("NVIDIA"));
    assert_eq!(gpu.adapter, "NVIDIA");

    gpu.apply_env_value(Some("auto"));
    assert_eq!(gpu.adapter, "auto");
  }
}
