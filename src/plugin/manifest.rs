//! Manifest parsing and validation for the v1 plugin API.
//!
//! A v1 plugin is a directory containing `plugin.toml` and, if it wants an
//! image, a file under its own directory. There is no executable code and no
//! host callback: the plugin describes inert widgets and the greeter paints
//! them. That is the entire sandbox, and it is why a plugin cannot see a
//! keystroke, the username, or the password.
//!
//! Parsing is strict (`deny_unknown_fields`) so a typo fails loudly instead of
//! silently doing nothing. Range problems inside a widget skip that widget, but
//! a structurally broken manifest disables the plugin.

use std::collections::HashSet;
use std::path::{Component, Path};

use jiff::tz::TimeZone;
use serde::Deserialize;
use tracing::warn;

use crate::color::Rgb;
use crate::config::ClockStyle;
use crate::layout::Anchor;
use crate::ui::clock::ClockSpec;

/// The only plugin API version this build understands.
pub const API_VERSION: u32 = 1;
pub(crate) const MAX_WIDGETS: usize = 16;
pub(crate) const MAX_TEXT_CHARS: usize = 200;
const MAX_MANIFEST_GRANTS: usize = 8;

/// A validated widget before image paths are resolved.
#[derive(Debug)]
pub(crate) enum ParsedWidget {
  Text {
    text: String,
    size: f32,
    color: Rgb,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
    max_width: Option<f32>,
  },
  Clock {
    spec: ClockSpec,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
  },
  Image {
    path: String,
    width: f32,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
  },
  /// A first-party media widget. The manifest can place it, but the live
  /// player state comes from the host, exactly as it does for the built-in
  /// widget built from `[media]`.
  Media {
    size: f32,
    show_art: bool,
    /// Draw the last visible track before authentication. Shared machines
    /// should set this to `false`, exactly like `media.show_before_login`.
    show_before_login: bool,
    color: Rgb,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
  },
}

/// A manifest that passed structural validation.
#[derive(Debug)]
pub(crate) struct Manifest {
  pub name: String,
  pub version: String,
  pub description: String,
  pub grants: Vec<String>,
  pub widgets: Vec<ParsedWidget>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
  api_version: u32,
  name: String,
  #[serde(default)]
  version: Option<String>,
  #[serde(default)]
  description: Option<String>,
  #[serde(default)]
  permissions: RawPermissions,
  #[serde(default, rename = "widget")]
  widgets: Vec<RawWidget>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
  #[serde(default)]
  filesystem: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawWidget {
  Text {
    text: String,
    #[serde(default)]
    size: Option<Num>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    offset: Option<[i32; 2]>,
    #[serde(default)]
    monitor: Option<String>,
    #[serde(default)]
    rotation: Option<Num>,
    #[serde(default)]
    max_width: Option<Num>,
  },
  Clock {
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    date_format: Option<String>,
    #[serde(default)]
    hour12: bool,
    #[serde(default)]
    seconds: bool,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    posix_locale: bool,
    #[serde(default)]
    size: Option<Num>,
    #[serde(default)]
    date_size: Option<Num>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    date_color: Option<String>,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    offset: Option<[i32; 2]>,
    #[serde(default)]
    monitor: Option<String>,
    #[serde(default)]
    rotation: Option<Num>,
  },
  Image {
    path: String,
    #[serde(default)]
    width: Option<Num>,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    offset: Option<[i32; 2]>,
    #[serde(default)]
    monitor: Option<String>,
    #[serde(default)]
    rotation: Option<Num>,
  },
  Media {
    #[serde(default)]
    size: Option<Num>,
    #[serde(default = "default_true")]
    show_art: bool,
    #[serde(default = "default_true")]
    show_before_login: bool,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    offset: Option<[i32; 2]>,
    #[serde(default)]
    monitor: Option<String>,
    #[serde(default)]
    rotation: Option<Num>,
  },
}

fn default_true() -> bool {
  true
}

/// Accepts `16` and `16.0`. TOML authors should not have to remember which
/// numeric type a Rust field happens to use.
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(untagged)]
enum Num {
  Integer(i64),
  Float(f64),
}

impl Num {
  fn to_f32(self) -> Option<f32> {
    let value = match self {
      Self::Integer(value) => value as f64,
      Self::Float(value) => value,
    };
    let value = value as f32;
    value.is_finite().then_some(value)
  }
}

/// Parses and validates a manifest. `default_color` is the theme's muted
/// colour, used when a widget does not specify one.
pub(crate) fn parse(text: &str, default_color: Rgb) -> Result<Manifest, String> {
  let raw: RawManifest =
    toml::from_str(text).map_err(|error| format!("invalid manifest: {error}"))?;

  if raw.api_version != API_VERSION {
    return Err(format!(
      "plugin API version {} is not supported (this build supports {API_VERSION})",
      raw.api_version
    ));
  }

  let name = checked_string(&raw.name, "name", 1, 64)?;
  let version = raw
    .version
    .as_deref()
    .map(|value| checked_string(value, "version", 0, 32))
    .transpose()?
    .unwrap_or_else(|| "unspecified".to_string());
  let description = raw
    .description
    .as_deref()
    .map(|value| checked_string(value, "description", 0, 200))
    .transpose()?
    .unwrap_or_default();

  let grants = validate_grants(&raw.permissions.filesystem)?;
  if raw.widgets.len() > MAX_WIDGETS {
    return Err(format!("at most {MAX_WIDGETS} widgets are supported"));
  }

  let mut widgets = Vec::with_capacity(raw.widgets.len());
  for (index, raw_widget) in raw.widgets.into_iter().enumerate() {
    match validate_widget(raw_widget, default_color) {
      Ok(widget) => widgets.push(widget),
      Err(message) => warn!(widget = index + 1, %message, "skipping plugin widget"),
    }
  }

  Ok(Manifest {
    name,
    version,
    description,
    grants,
    widgets,
  })
}

fn validate_widget(raw: RawWidget, default_color: Rgb) -> Result<ParsedWidget, String> {
  let anchor = match raw_anchor(&raw) {
    Some(text) => Anchor::parse(text).ok_or_else(|| {
      format!(
        "unknown anchor '{text}'; use top-left, top-center, top-right, \
         bottom-left, bottom-center, or bottom-right"
      )
    })?,
    None => Anchor::default(),
  };
  let offset = raw_offset(&raw);
  validate_offset(offset)?;
  let monitor = raw_monitor(&raw).map(validate_monitor).transpose()?;
  let rotation = validate_rotation(raw_rotation(&raw))?;
  let color = raw_color(&raw, default_color)?;

  match raw {
    RawWidget::Text {
      text,
      size,
      max_width,
      ..
    } => {
      let text = checked_string(&text, "text", 1, MAX_TEXT_CHARS)?;
      let size = validate_size(size.and_then(Num::to_f32).unwrap_or(16.0))?;
      let max_width = max_width
        .map(|value| {
          value
            .to_f32()
            .filter(|value| (40.0..=2000.0).contains(value))
            .ok_or_else(|| "max_width must be between 40 and 2000".to_string())
        })
        .transpose()?;
      Ok(ParsedWidget::Text {
        text,
        size,
        color,
        anchor,
        offset,
        monitor,
        rotation,
        max_width,
      })
    }
    RawWidget::Clock {
      style,
      format,
      date_format,
      hour12,
      seconds,
      timezone,
      posix_locale,
      size,
      date_size,
      date_color,
      ..
    } => {
      let style = match style.as_deref() {
        Some(style) => ClockStyle::parse(style)
          .ok_or_else(|| format!("unknown clock style {style:?}; use digital or analog"))?,
        None => ClockStyle::Digital,
      };
      let format = format.unwrap_or_else(|| "%H:%M".to_string());
      if format.chars().count() > 64 {
        return Err("clock format must be at most 64 characters".to_string());
      }
      jiff::fmt::strtime::format(&format, &jiff::Zoned::now())
        .map_err(|error| format!("invalid clock format: {error}"))?;
      let date_format = date_format.unwrap_or_default();
      if date_format.chars().count() > 64 {
        return Err("clock date_format must be at most 64 characters".to_string());
      }
      if !date_format.is_empty() {
        jiff::fmt::strtime::format(&date_format, &jiff::Zoned::now())
          .map_err(|error| format!("invalid clock date_format: {error}"))?;
      }
      let timezone = timezone
        .as_deref()
        .map(|timezone| {
          TimeZone::get(timezone)
            .map_err(|error| format!("cannot load time zone {timezone:?}: {error}"))
        })
        .transpose()?;
      let date_color = date_color
        .as_deref()
        .map(|text| Rgb::parse(text).map_err(|error| error.to_string()))
        .transpose()?;
      let spec = ClockSpec {
        style,
        format,
        date_format,
        hour12,
        seconds,
        timezone,
        posix_locale,
        size: validate_size(size.and_then(Num::to_f32).unwrap_or(16.0))?,
        date_size: validate_size(date_size.and_then(Num::to_f32).unwrap_or(12.0))?,
        color: Some(color),
        date_color,
      };
      Ok(ParsedWidget::Clock {
        spec,
        anchor,
        offset,
        monitor,
        rotation,
      })
    }
    RawWidget::Image { path, width, .. } => {
      let path = normalize_relative(&path, "image path")?;
      let width = width.and_then(Num::to_f32).unwrap_or(64.0);
      if !(8.0..=512.0).contains(&width) {
        return Err("image width must be between 8 and 512".to_string());
      }
      Ok(ParsedWidget::Image {
        path,
        width,
        anchor,
        offset,
        monitor,
        rotation,
      })
    }
    RawWidget::Media {
      size,
      show_art,
      show_before_login,
      ..
    } => {
      let size = size.and_then(Num::to_f32).unwrap_or(16.0);
      if !(8.0..=48.0).contains(&size) {
        return Err("media size must be between 8 and 48".to_string());
      }
      Ok(ParsedWidget::Media {
        size,
        show_art,
        show_before_login,
        color,
        anchor,
        offset,
        monitor,
        rotation,
      })
    }
  }
}

fn raw_anchor(raw: &RawWidget) -> Option<&str> {
  match raw {
    RawWidget::Text { anchor, .. }
    | RawWidget::Clock { anchor, .. }
    | RawWidget::Image { anchor, .. }
    | RawWidget::Media { anchor, .. } => anchor.as_deref(),
  }
}

fn raw_rotation(raw: &RawWidget) -> Option<f32> {
  let value = match raw {
    RawWidget::Text { rotation, .. }
    | RawWidget::Clock { rotation, .. }
    | RawWidget::Image { rotation, .. }
    | RawWidget::Media { rotation, .. } => rotation.as_ref(),
  }?;
  value.to_f32()
}

fn validate_rotation(value: Option<f32>) -> Result<f32, String> {
  match value {
    None => Ok(0.0),
    Some(value) if value.is_finite() && (-360.0..=360.0).contains(&value) => Ok(value),
    Some(_) => Err("rotation must be between -360 and 360".to_string()),
  }
}

fn raw_offset(raw: &RawWidget) -> (i32, i32) {
  match raw {
    RawWidget::Text { offset, .. }
    | RawWidget::Clock { offset, .. }
    | RawWidget::Image { offset, .. }
    | RawWidget::Media { offset, .. } => offset.map_or((24, 24), |pair| (pair[0], pair[1])),
  }
}

fn raw_monitor(raw: &RawWidget) -> Option<&str> {
  match raw {
    RawWidget::Text { monitor, .. }
    | RawWidget::Clock { monitor, .. }
    | RawWidget::Image { monitor, .. }
    | RawWidget::Media { monitor, .. } => monitor.as_deref(),
  }
}

fn raw_color(raw: &RawWidget, default: Rgb) -> Result<Rgb, String> {
  match raw {
    RawWidget::Text { color, .. }
    | RawWidget::Clock { color, .. }
    | RawWidget::Media { color, .. } => match color {
      Some(text) => Rgb::parse(text).map_err(|error| error.to_string()),
      None => Ok(default),
    },
    RawWidget::Image { .. } => Ok(default),
  }
}

fn validate_size(size: f32) -> Result<f32, String> {
  if (8.0..=160.0).contains(&size) {
    Ok(size)
  } else {
    Err(format!("size must be between 8 and 160 (got {size})"))
  }
}

fn validate_offset(offset: (i32, i32)) -> Result<(), String> {
  let valid = |value: i32| (-2048..=4096).contains(&value);
  if valid(offset.0) && valid(offset.1) {
    Ok(())
  } else {
    Err("offset components must be between -2048 and 4096".to_string())
  }
}

fn validate_monitor(value: &str) -> Result<String, String> {
  let value = value.trim();
  if value.is_empty() || value.chars().count() > 64 || value.chars().any(char::is_control) {
    return Err("monitor must be 1-64 printable characters".to_string());
  }
  Ok(value.to_string())
}

fn checked_string(value: &str, field: &str, min: usize, max: usize) -> Result<String, String> {
  let value = value.trim();
  let count = value.chars().count();
  if count < min {
    return Err(format!("{field} must not be empty"));
  }
  if count > max {
    return Err(format!("{field} must be at most {max} characters"));
  }
  if value.chars().any(char::is_control) {
    return Err(format!("{field} must not contain control characters"));
  }
  Ok(value.to_string())
}

fn validate_grants(raw: &[String]) -> Result<Vec<String>, String> {
  if raw.len() > MAX_MANIFEST_GRANTS {
    return Err(format!(
      "at most {MAX_MANIFEST_GRANTS} filesystem grants are supported"
    ));
  }
  let mut grants = Vec::with_capacity(raw.len());
  let mut seen = HashSet::new();
  for grant in raw {
    let grant = normalize_relative(grant, "filesystem grant")?;
    if seen.insert(grant.clone()) {
      grants.push(grant);
    }
  }
  Ok(grants)
}

/// Normalises a plugin-relative path and rejects anything that could escape
/// the plugin directory. Absolute paths, `..`, `.` components, and Windows
/// prefixes are all denied; the later canonicalisation check is defence in
/// depth for symlinks.
pub(crate) fn normalize_relative(raw: &str, what: &str) -> Result<String, String> {
  let raw = raw.trim();
  if raw.is_empty() {
    return Err(format!("{what} must not be empty"));
  }
  if raw.chars().count() > 256 {
    return Err(format!("{what} must be at most 256 characters"));
  }
  if raw.chars().any(char::is_control) {
    return Err(format!("{what} must not contain control characters"));
  }
  let mut parts = Vec::new();
  for component in Path::new(raw).components() {
    match component {
      Component::Normal(part) => {
        let Some(part) = part.to_str() else {
          return Err(format!("{what} must be valid UTF-8"));
        };
        parts.push(part);
      }
      Component::CurDir => {}
      Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
        return Err(format!("{what} must be a relative path inside the plugin"));
      }
    }
  }
  if parts.is_empty() {
    return Err(format!("{what} must not be empty"));
  }
  Ok(parts.join("/"))
}

/// True when `path` is covered by a grant that is either the same file or a
/// parent directory. Grants are deliberately simple prefix matches: no globs,
/// no shell patterns, nothing surprising.
pub(crate) fn is_granted(path: &str, grants: &[String]) -> bool {
  grants.iter().any(|grant| {
    path == grant
      || path
        .strip_prefix(grant.as_str())
        .is_some_and(|rest| rest.starts_with('/'))
  })
}

#[cfg(test)]
mod tests {
  use super::{Anchor, is_granted, normalize_relative, parse};
  use crate::config::ClockStyle;

  const BASE: &str = r##"
api_version = 1
name = "Hello"
version = "1.0.0"

[permissions]
filesystem = ["assets"]

[[widget]]
kind = "text"
text = "Welcome"
"##;

  fn color() -> crate::color::Rgb {
    crate::color::Rgb::solid(1, 2, 3)
  }

  #[test]
  fn parses_a_minimal_manifest() {
    let manifest = parse(BASE, color()).unwrap();
    assert_eq!(manifest.name, "Hello");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.widgets.len(), 1);
    assert_eq!(manifest.grants, ["assets"]);
  }

  #[test]
  fn rejects_unsupported_api_versions() {
    let error = parse("api_version = 2\nname = \"X\"\n", color()).unwrap_err();
    assert!(error.contains("not supported"));
  }

  #[test]
  fn rejects_unknown_fields_loudly() {
    let error = parse("api_version = 1\nname = \"X\"\nnetwrok = true\n", color()).unwrap_err();
    assert!(error.contains("netwrok"));
  }

  #[test]
  fn rejects_no_code_permissions() {
    let error = parse(
      "api_version = 1\nname = \"X\"\n[permissions]\nnetwork = true\n",
      color(),
    )
    .unwrap_err();
    assert!(error.contains("network"));
  }

  #[test]
  fn skips_a_bad_widget_but_keeps_the_plugin() {
    let source = format!("{BASE}\n[[widget]]\nkind = \"text\"\ntext = \"Too big\"\nsize = 1000\n");
    let manifest = parse(&source, color()).unwrap();
    assert_eq!(manifest.widgets.len(), 1);
  }

  #[test]
  fn clock_widgets_support_the_extended_v1_fields() {
    let source = r#"
api_version = 1
name = "Clock"
[[widget]]
kind = "clock"
style = "analog"
seconds = true
hour12 = true
date_format = "%Y"
timezone = "UTC"
monitor = "DP-1"
"#;
    let manifest = parse(source, color()).unwrap();
    match &manifest.widgets[0] {
      super::ParsedWidget::Clock { spec, monitor, .. } => {
        assert_eq!(spec.style, ClockStyle::Analog);
        assert!(spec.seconds);
        assert!(spec.timezone.is_some());
        assert_eq!(monitor.as_deref(), Some("DP-1"));
      }
      other => panic!("expected a clock, got {other:?}"),
    }
  }

  #[test]
  fn media_widgets_use_the_same_manifest_surface() {
    let source = r#"
api_version = 1
name = "Media"
[[widget]]
kind = "media"
size = 18
show_art = false
show_before_login = false
anchor = "bottom-left"
"#;
    let manifest = parse(source, color()).unwrap();
    match &manifest.widgets[0] {
      super::ParsedWidget::Media {
        size,
        show_art,
        show_before_login,
        anchor,
        ..
      } => {
        assert_eq!(*size, 18.0);
        assert!(!show_art);
        assert!(!show_before_login);
        assert_eq!(*anchor, Anchor::BottomLeft);
      }
      other => panic!("expected media, got {other:?}"),
    }

    // The pre-auth default matches the built-in widget. A shared machine has
    // to opt out explicitly, in either the main config or the manifest.
    let defaulted = parse(
      r#"
api_version = 1
name = "Media"
[[widget]]
kind = "media"
"#,
      color(),
    )
    .unwrap();
    match &defaulted.widgets[0] {
      super::ParsedWidget::Media {
        show_before_login, ..
      } => assert!(show_before_login),
      other => panic!("expected media, got {other:?}"),
    }
  }

  #[test]
  fn normalises_safe_relative_paths_and_rejects_escapes() {
    assert!(
      normalize_relative("assets/../x", "path")
        .unwrap_err()
        .contains("relative")
    );
    assert!(
      normalize_relative("/etc/passwd", "path")
        .unwrap_err()
        .contains("relative")
    );
    assert_eq!(
      normalize_relative("./assets/logo.png", "path").unwrap(),
      "assets/logo.png"
    );
    assert!(
      normalize_relative("", "path")
        .unwrap_err()
        .contains("empty")
    );
  }

  #[test]
  fn grants_cover_files_and_directories_but_not_siblings() {
    let grants = vec!["assets".to_string(), "logo.png".to_string()];
    assert!(is_granted("assets/logo.png", &grants));
    assert!(is_granted("logo.png", &grants));
    assert!(!is_granted("assets2/logo.png", &grants));
    assert!(!is_granted("secrets.png", &grants));
  }

  #[test]
  fn anchor_positions_respect_offsets() {
    let (x, y) = Anchor::TopLeft.place((100.0, 50.0), (10.0, 5.0), (2, 3));
    assert_eq!((x, y), (2.0, 3.0));
    let (x, y) = Anchor::BottomRight.place((100.0, 50.0), (10.0, 5.0), (2, 3));
    assert_eq!((x, y), (88.0, 42.0));
  }
}
