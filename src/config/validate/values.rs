//! Values validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn color_list(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: &[Rgb],
    max: usize,
  ) -> Vec<Rgb> {
    let Some(field) = field else {
      return default.to_vec();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(
        key,
        Some(field),
        "expected an array of hex colours such as [\"#ffffff\", \"#d41830\"]",
      );
      return default.to_vec();
    };
    if values.len() > max {
      self.issue(key, Some(field), format!("accepts at most {max} colours"));
    }
    let mut colours = Vec::new();
    for value in values.iter().take(max) {
      let Some(text) = value.as_str() else {
        self.issue(key, Some(field), "every entry must be a hex colour string");
        continue;
      };
      match Rgb::parse(text) {
        Ok(colour) => colours.push(colour),
        Err(error) => self.issue(key, Some(field), error.to_string()),
      }
    }
    if colours.is_empty() {
      self.issue(key, Some(field), "no usable colours; using the defaults");
      default.to_vec()
    } else {
      colours
    }
  }

  /// A bounded list of short text runs for the ticker. Empty entries are
  /// dropped because they would only add an invisible gap.
  pub(super) fn text_list(
    &mut self,
    field: Option<&Field>,
    key: &str,
    max_items: usize,
    max_chars: usize,
  ) -> Vec<String> {
    let Some(field) = field else {
      return Vec::new();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(
        key,
        Some(field),
        "expected an array of strings such as [\"FIRST\", \"SECOND\"]",
      );
      return Vec::new();
    };
    if values.len() > max_items {
      self.issue(
        key,
        Some(field),
        format!("accepts at most {max_items} items"),
      );
    }
    let mut items = Vec::new();
    for value in values.iter().take(max_items) {
      let Some(text) = value.as_str() else {
        self.issue(key, Some(field), "every item must be a string");
        continue;
      };
      let text = text.trim();
      if text.is_empty() {
        continue;
      }
      if text.chars().count() > max_chars {
        self.issue(
          key,
          Some(field),
          format!("every item must be at most {max_chars} characters"),
        );
        continue;
      }
      if text.chars().any(char::is_control) {
        self.issue(
          key,
          Some(field),
          "items must not contain control characters",
        );
        continue;
      }
      items.push(text.to_string());
    }
    items
  }

  pub(super) fn location(&self, field: &Field) -> Option<Location> {
    let (line, column) = crate::config::location_of(self.source, field.span().start)?;
    Some(Location {
      file: self.path.to_path_buf(),
      line,
      column,
    })
  }

  pub(super) fn issue(
    &mut self,
    key: impl Into<String>,
    field: Option<&Field>,
    message: impl Into<String>,
  ) {
    self.issues.push(Issue {
      key: key.into(),
      location: field.and_then(|field| self.location(field)),
      message: message.into(),
    });
  }

  /// Validates the shape of a string value. Returns `None` when the field is
  /// absent or invalid; an empty string is a valid value here so callers that
  /// allow blank settings can decide what it means.
  pub(super) fn string_value(
    &mut self,
    field: Option<&Field>,
    key: &str,
    expectation: &str,
    max_chars: usize,
  ) -> Option<String> {
    let field = field?;
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), expectation);
      return None;
    };
    let value = value.trim();
    if value.chars().count() > max_chars {
      self.issue(
        key,
        Some(field),
        format!("must be at most {max_chars} characters"),
      );
      return None;
    }
    if value.chars().any(char::is_control) {
      self.issue(key, Some(field), "must not contain control characters");
      return None;
    }
    Some(value.to_string())
  }

  /// Like [`Self::string_value`], but keeps leading and trailing spaces. The
  /// ticker separator is one of the few values where the whitespace is the
  /// design, so trimming it would silently change the marquee rhythm.
  pub(super) fn literal_value(
    &mut self,
    field: Option<&Field>,
    key: &str,
    expectation: &str,
    max_chars: usize,
  ) -> Option<String> {
    let field = field?;
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), expectation);
      return None;
    };
    if value.chars().count() > max_chars {
      self.issue(
        key,
        Some(field),
        format!("must be at most {max_chars} characters"),
      );
      return None;
    }
    if value.chars().any(char::is_control) {
      self.issue(key, Some(field), "must not contain control characters");
      return None;
    }
    Some(value.to_string())
  }

  pub(super) fn text(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: &str,
    expectation: &str,
    max_chars: usize,
  ) -> String {
    let Some(value) = self.string_value(field, key, expectation, max_chars) else {
      return default.to_string();
    };
    if value.is_empty() && !default.is_empty() {
      self.issue(key, field, "must not be empty");
      return default.to_string();
    }
    value
  }

  pub(super) fn optional_text(
    &mut self,
    field: Option<&Field>,
    key: &str,
    expectation: &str,
    max_chars: usize,
  ) -> Option<String> {
    let value = self.text(field, key, "", expectation, max_chars);
    (!value.is_empty()).then_some(value)
  }

  pub(super) fn boolean(&mut self, field: Option<&Field>, key: &str, default: bool) -> bool {
    let Some(field) = field else {
      return default;
    };
    match field.get_ref().as_bool() {
      Some(value) => value,
      None => {
        self.issue(key, Some(field), "expected true or false");
        default
      }
    }
  }

  pub(super) fn number(&mut self, field: &Field, key: &str) -> Option<f64> {
    let value = match field.get_ref() {
      Value::Integer(number) => *number as f64,
      Value::Float(number) if number.is_finite() => *number,
      _ => {
        self.issue(key, Some(field), "expected a number");
        return None;
      }
    };
    Some(value)
  }

  /// BPM has a documented zero value that means "off", so it cannot use the
  /// plain range helper: a present `0` is valid even though the musical range
  /// starts at 20. Anything between 0 and 20 gets the usual issue and fallback.
  pub(super) fn beat_bpm(&mut self, field: Option<&Field>, default: f32) -> f32 {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = self.number(field, "motion.beat.bpm") else {
      return default;
    };
    if value == 0.0 {
      return 0.0;
    }
    if (20.0..=300.0).contains(&value) {
      return value as f32;
    }
    self.issue(
      "motion.beat.bpm",
      Some(field),
      format!("must be 0 or between 20 and 300 (got {value})"),
    );
    default
  }

  pub(super) fn range(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: f64,
    min: f64,
    max: f64,
  ) -> f64 {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = self.number(field, key) else {
      return default;
    };
    if !(min..=max).contains(&value) {
      self.issue(
        key,
        Some(field),
        format!("must be between {min} and {max} (got {value})"),
      );
      return default;
    }
    value
  }

  pub(super) fn integer_range(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
  ) -> u64 {
    let Some(field) = field else {
      return default;
    };
    let value = match field.get_ref() {
      Value::Integer(number) => u64::try_from(*number).ok(),
      Value::Float(number) if number.is_finite() && number.fract() == 0.0 && *number >= 0.0 => {
        Some(*number as u64)
      }
      _ => None,
    };
    let Some(value) = value else {
      self.issue(key, Some(field), "expected a whole number");
      return default;
    };
    if !(min..=max).contains(&value) {
      self.issue(
        key,
        Some(field),
        format!("must be between {min} and {max} (got {value})"),
      );
      return default;
    }
    value
  }

  /// A panel needs both dimensions. Missing or invalid values fall back to a
  /// sane plate instead of dropping the item, so one typo does not remove the
  /// backdrop that other decor was positioned against.
  pub(super) fn panel_size(&mut self, field: Option<&Field>, key: &str, default: u64) -> f32 {
    let Some(field) = field else {
      self.issue(key, None, "panel needs a whole number from 4 to 4000");
      return default as f32;
    };
    self.integer_range(Some(field), key, default, 4, 4000) as f32
  }

  pub(super) fn color(&mut self, field: Option<&Field>, key: &'static str, default: Rgb) -> Rgb {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        key,
        Some(field),
        "expected a hex colour such as \"#4c8dff\"",
      );
      return default;
    };
    match Rgb::parse(value) {
      Ok(color) => color,
      Err(error) => {
        self.issue(key, Some(field), error.to_string());
        default
      }
    }
  }

  pub(super) fn optional_color(&mut self, field: Option<&Field>, key: &str) -> Option<Rgb> {
    let field = field?;
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        key,
        Some(field),
        "expected a hex colour such as \"#4c8dff\"",
      );
      return None;
    };
    match Rgb::parse(value) {
      Ok(color) => Some(color),
      Err(error) => {
        self.issue(key, Some(field), error.to_string());
        None
      }
    }
  }

  pub(super) fn preset(&mut self, field: Option<&Field>) -> ThemePreset {
    let Some(field) = field else {
      return ThemePreset::default();
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(
        "theme.preset",
        Some(field),
        "expected glass-dark, glass-light, or minimal",
      );
      return ThemePreset::default();
    };
    ThemePreset::parse(value).unwrap_or_else(|| {
      self.issue(
        "theme.preset",
        Some(field),
        format!("unknown preset {value:?}; use glass-dark, glass-light, or minimal"),
      );
      ThemePreset::default()
    })
  }
}
