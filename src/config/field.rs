//! Form field, caret, hint, and layout types.

use super::*;

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
  /// Label size in logical pixels. Zero derives 80 percent of the field size,
  /// which is the historical look. A micro label sets a small fixed value.
  pub label_size: f32,
  /// Label letter spacing in em. Zero keeps the font's natural tracking.
  pub label_tracking: f32,
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
  /// Duration of the one-shot keystroke pop in milliseconds. Zero disables it,
  /// which is the default and keeps every existing field pixel-identical.
  pub key_pop_ms: f32,
  /// Peak scale of the popping character. It settles back to 1.0.
  pub key_pop_scale: f32,
  /// Colour of the popping character. `None` follows the theme accent.
  pub key_pop_color: Option<Rgb>,
}

impl FieldConfig {
  pub(crate) fn default_username() -> Self {
    Self {
      label: String::new(),
      label_color: None,
      label_size: 0.0,
      label_tracking: 0.0,
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
      key_pop_ms: 0.0,
      key_pop_scale: 1.18,
      key_pop_color: None,
    }
  }

  pub(crate) fn default_password() -> Self {
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
