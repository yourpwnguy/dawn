//! A tiny, dependency-free colour type used by config, plugins, and drawing.
//!
//! Config and plugin manifests both accept CSS-style hex colours. Parsing them
//! in one place keeps validation messages consistent and means the UI never
//! has to deal with unparsed strings at draw time.

use std::fmt;

/// Straight (non-premultiplied) sRGB colour with 8 bits per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
  pub r: u8,
  pub g: u8,
  pub b: u8,
  pub a: u8,
}

impl Rgb {
  pub const fn solid(r: u8, g: u8, b: u8) -> Self {
    Self { r, g, b, a: 255 }
  }

  pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
    Self { r, g, b, a }
  }

  /// Parses `#rgb`, `#rgba`, `#rrggbb`, and `#rrggbbaa` (case insensitive).
  ///
  /// We do not accept named colours or `rgb()`: a greeter config is usually
  /// copied from a theme file and hex is the one format every theme uses.
  pub fn parse(input: &str) -> Result<Self, ColorError> {
    let hex = input
      .strip_prefix('#')
      .ok_or_else(|| ColorError::new(input, "colour must start with '#'"))?;

    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
      return Err(ColorError::new(input, "colour contains non-hex characters"));
    }

    // Expand the short forms by doubling each nibble: #abc -> #aabbcc.
    let expanded: String = match hex.len() {
      3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
      6 | 8 => hex.to_string(),
      _ => {
        return Err(ColorError::new(
          input,
          "colour must have 3, 4, 6, or 8 hex digits",
        ));
      }
    };

    let channel = |start: usize| -> u8 {
      // Safe to unwrap conceptually: the length check above guarantees
      // two hex digits exist. `from_str_radix` is still fallible, so we
      // fall back to zero instead of panicking if the impossible happens.
      expanded
        .get(start..start + 2)
        .and_then(|part| u8::from_str_radix(part, 16).ok())
        .unwrap_or(0)
    };

    let alpha = if expanded.len() == 8 { channel(6) } else { 255 };
    Ok(Self::rgba(channel(0), channel(2), channel(4), alpha))
  }

  /// Same colour with a different alpha value.
  pub const fn with_alpha(self, a: u8) -> Self {
    Self { a, ..self }
  }

  /// Straight-line blend from `self` to `other`. `t` is clamped to 0..1.
  /// Used by hover transitions and the button's colour effects.
  #[must_use]
  pub fn mix(self, other: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| {
      (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
        .round()
        .clamp(0.0, 255.0) as u8
    };
    Rgb::rgba(
      channel(self.r, other.r),
      channel(self.g, other.g),
      channel(self.b, other.b),
      channel(self.a, other.a),
    )
  }

  /// Moves a colour toward white. `amount` is clamped to 0..1, so a hovered
  /// button still keeps its identity instead of turning into a flat white slab.
  #[must_use]
  pub fn lighten(self, amount: f32) -> Rgb {
    self.mix(Rgb::solid(255, 255, 255), amount)
  }

  /// Multiplies the RGB channels, keeping alpha. The pulse effect uses this
  /// to breathe the fill without changing its hue.
  #[must_use]
  pub fn scale_rgb(self, factor: f32) -> Rgb {
    let channel = |value: u8| {
      (f32::from(value) * factor.clamp(0.0, 2.0))
        .round()
        .clamp(0.0, 255.0) as u8
    };
    Rgb::rgba(channel(self.r), channel(self.g), channel(self.b), self.a)
  }

  /// Composites a straight-alpha colour over an opaque backdrop. Used to
  /// reason about the effective colour of a translucent glass panel, which is
  /// what text contrast has to be checked against.
  #[must_use]
  pub fn over(self, backdrop: Rgb) -> Rgb {
    let alpha = f32::from(self.a) / 255.0;
    let mix = |front: u8, back: u8| {
      (f32::from(front) * alpha + f32::from(back) * (1.0 - alpha))
        .round()
        .clamp(0.0, 255.0) as u8
    };
    Rgb::solid(
      mix(self.r, backdrop.r),
      mix(self.g, backdrop.g),
      mix(self.b, backdrop.b),
    )
  }

  /// WCAG relative luminance. Kept here instead of in the renderer so both the
  /// theme and the tests can use the same definition.
  #[must_use]
  pub fn luminance(self) -> f32 {
    fn linear(channel: u8) -> f32 {
      let value = f32::from(channel) / 255.0;
      if value <= 0.040_45 {
        value / 12.92
      } else {
        ((value + 0.055) / 1.055).powf(2.4)
      }
    }
    0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
  }

  /// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
  #[must_use]
  pub fn contrast_ratio(self, other: Rgb) -> f32 {
    let (lighter, darker) = {
      let (a, b) = (self.luminance(), other.luminance());
      if a >= b { (a, b) } else { (b, a) }
    };
    (lighter + 0.05) / (darker + 0.05)
  }

  /// Returns `self` when it meets `minimum` against `backdrop`, otherwise the
  /// black or white variant with the better ratio. This is the last line of
  /// defence for glass legibility: a preset that looks fine over a dark
  /// wallpaper must not become unreadable over a bright one.
  #[must_use]
  pub fn ensure_contrast(self, backdrop: Rgb, minimum: f32) -> Rgb {
    if self.contrast_ratio(backdrop) >= minimum {
      return self;
    }
    let dark = Rgb::solid(0, 0, 0);
    let light = Rgb::solid(255, 255, 255);
    if dark.contrast_ratio(backdrop) >= light.contrast_ratio(backdrop) {
      dark
    } else {
      light
    }
  }

  /// Converts to the colour type tiny-skia paints with.
  #[must_use]
  pub fn to_skia(self) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
  }
}

/// Why a colour string could not be parsed. Kept small on purpose: it is
/// rendered inside a config warning, not matched on by callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorError {
  input: String,
  reason: &'static str,
}

impl ColorError {
  fn new(input: &str, reason: &'static str) -> Self {
    // Bound the echo so a pathological config value cannot flood the UI.
    let mut input = input.to_string();
    if input.len() > 32 {
      input.truncate(29);
      input.push_str("...");
    }
    Self { input, reason }
  }

  #[must_use]
  pub fn reason(&self) -> &'static str {
    self.reason
  }
}

impl fmt::Display for ColorError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{} (got {:?})", self.reason, self.input)
  }
}

impl std::error::Error for ColorError {}

#[cfg(test)]
mod tests {
  use super::Rgb;

  #[test]
  fn parses_all_supported_forms() {
    assert_eq!(Rgb::parse("#fff"), Ok(Rgb::solid(255, 255, 255)));
    assert_eq!(Rgb::parse("#f00a"), Ok(Rgb::rgba(255, 0, 0, 0xaa)));
    assert_eq!(Rgb::parse("#102030"), Ok(Rgb::solid(0x10, 0x20, 0x30)));
    assert_eq!(
      Rgb::parse("#10203040"),
      Ok(Rgb::rgba(0x10, 0x20, 0x30, 0x40))
    );
    assert_eq!(Rgb::parse("#AaBbCc"), Ok(Rgb::solid(0xaa, 0xbb, 0xcc)));
  }

  #[test]
  fn rejects_bad_input_with_readable_reasons() {
    for bad in ["fff", "#ggg", "#12", "#12345", "#ffffffffff"] {
      let error = Rgb::parse(bad).unwrap_err();
      assert!(!error.reason().is_empty(), "missing reason for {bad}");
    }
  }

  #[test]
  fn error_echo_is_bounded() {
    let long = format!("#{}", "z".repeat(200));
    let error = Rgb::parse(&long).unwrap_err();
    assert!(error.to_string().len() < 80);
  }

  #[test]
  fn alpha_compositing_and_contrast_behave() {
    let black = Rgb::solid(0, 0, 0);
    let white = Rgb::solid(255, 255, 255);
    assert!((black.contrast_ratio(white) - 21.0).abs() < 0.01);
    assert_eq!(black.over(white), black);
    assert_eq!(Rgb::rgba(255, 255, 255, 128).over(black).r, 128);
    // Mid grey on white is below the WCAG AA ratio; the guard must pick black.
    let guarded = Rgb::solid(160, 160, 160).ensure_contrast(white, 4.5);
    assert!(guarded.contrast_ratio(white) >= 4.5);
  }
}
