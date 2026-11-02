//! Login form, button, and card presentation types.
//!
//! These are data only. The renderer reads them, the validator fills them, and
//! nothing here knows about greetd or the window.

use super::*;

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
  /// A full-bleed, fixed frame with no implied surface. It shares the hero
  /// geometry (content-derived position is off, `form_offset` is on) but the
  /// name says what an author wants: a poster with the form placed directly
  /// on it. A theme pairs it with `surface = "none"`.
  Poster,
}

impl LoginLayout {
  /// True for the two fixed-frame layouts. Both place decor and the form in
  /// screen space and scale the whole frame to fit a smaller output.
  #[must_use]
  pub fn is_full_bleed(self) -> bool {
    matches!(self, Self::Hero | Self::Poster)
  }
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

/// How the username field starts.
///
/// A greeter is normally used by the person who owns the machine, so asking
/// them to type their own name every boot is just friction. `Auto` resolves a
/// local account at startup, `None` keeps the historical empty field, and a
/// literal name pins one account for a single-user kiosk.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UsernamePrefill {
  /// Use the last local login, then the first regular account.
  #[default]
  Auto,
  /// Start with an empty username field.
  None,
  /// Always start with this username.
  Name(String),
}

impl UsernamePrefill {
  /// Parses the `login.prefill` value.
  ///
  /// `auto` and `none` are the two keywords. Anything else is treated as a
  /// literal account name. We keep the parser here so the validator, the app,
  /// and the docs all agree on the same spelling.
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") {
      return Some(Self::Auto);
    }
    if value.eq_ignore_ascii_case("none") || value.is_empty() {
      return Some(Self::None);
    }
    valid_username(value).then(|| Self::Name(value.to_string()))
  }
}

/// A conservative username check for a local account name.
///
/// This is not an authentication rule, and PAM still has the final say. It only
/// keeps control characters, paths, and absurd lengths out of the field before
/// anything reaches the auth worker.
#[must_use]
pub fn valid_username(value: &str) -> bool {
  !value.is_empty()
    && value.len() <= 64
    && value
      .chars()
      .next()
      .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.'))
    && value
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
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
  /// How the username field is populated before the first key.
  pub prefill: UsernamePrefill,
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
  /// Optional boot title card. Disabled by default, so an existing config
  /// still opens straight onto the login form.
  pub splash: SplashConfig,
  /// Cosmetic images and text placed inside the card. The host draws these
  /// after the panel and before the form, so they never cover an input.
  pub decor: Vec<DecorSpec>,
  /// The optional login cut sequence. Disabled by default and never part of
  /// the auth path: it only changes what the window paints while auth runs.
  pub transition: TransitionConfig,
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
