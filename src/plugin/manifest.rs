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
use crate::config::{ClockStyle, MediaLook, SecondHand, WidgetLayer};
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
    layer: WidgetLayer,
  },
  Clock {
    spec: ClockSpec,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
    layer: WidgetLayer,
  },
  Image {
    path: String,
    width: f32,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
    layer: WidgetLayer,
  },
  /// A first-party media widget. The manifest can place it, but the live
  /// player state comes from the host, exactly as it does for the built-in
  /// widget built from `[media]`.
  Media {
    size: f32,
    show_art: bool,
    /// Manifest media widgets get the main-config defaults until the v1
    /// surface grows layout keys of its own.
    look: MediaLook,
    /// Draw the last visible track before authentication. Shared machines
    /// should set this to `false`, exactly like `media.show_before_login`.
    show_before_login: bool,
    color: Rgb,
    anchor: Anchor,
    offset: (i32, i32),
    monitor: Option<String>,
    rotation: f32,
    layer: WidgetLayer,
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
    #[serde(default)]
    layer: Option<String>,
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
    #[serde(default)]
    layer: Option<String>,
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
    #[serde(default)]
    layer: Option<String>,
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
    #[serde(default)]
    layer: Option<String>,
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
  // An unknown layer is a per-widget error, not a whole-manifest failure:
  // the rest of the plugin still loads without the misplaced widget.
  let layer = match raw_layer(&raw) {
    Some(text) => WidgetLayer::parse(text)
      .ok_or_else(|| format!("unknown widget layer '{text}'; use behind or front"))?,
    None => WidgetLayer::Behind,
  };

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
        layer,
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
        Some(style) => ClockStyle::parse(style).ok_or_else(|| {
          format!("unknown clock style {style:?}; use digital, analog, flip, or segment")
        })?,
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
        label: String::new(),
        label_color: None,
        label_size: 0.0,
        label_tracking: 0.0,
        // The v1 manifest does not expose the decorative face options yet.
        // Defaults keep a plugin clock on the same object model as a built-in
        // clock without inventing a second configuration surface.
        ghost: false,
        glow: None,
        glow_size: 0.0,
        colon_blink_ms: 0.0,
        second_hand: SecondHand::Tick,
      };
      Ok(ParsedWidget::Clock {
        spec,
        anchor,
        offset,
        monitor,
        rotation,
        layer,
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
        layer,
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
        look: MediaLook::default(),
        show_before_login,
        color,
        anchor,
        offset,
        monitor,
        rotation,
        layer,
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

fn raw_layer(raw: &RawWidget) -> Option<&str> {
  match raw {
    RawWidget::Text { layer, .. }
    | RawWidget::Clock { layer, .. }
    | RawWidget::Image { layer, .. }
    | RawWidget::Media { layer, .. } => layer.as_deref(),
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
mod tests;
