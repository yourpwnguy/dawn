//! Theme bundle manifests.
//!
//! A theme bundle is a directory, not an archive. Keeping it a directory means
//! `dawn add` can validate it, users can inspect it, and the greeter can read
//! assets straight from the installed location without unpacking anything at
//! boot. The manifest is the small, strict part:
//!
//! ```toml
//! schema = 1
//! id = "midnight"
//! name = "Midnight"
//! version = "1.0.0"
//! description = "A short one-line description."
//! preview = "preview.webp"
//! ```
//!
//! `config.toml` sits next to it and is a complete dawn config. Every asset it
//! names must live inside the bundle, so the directory is the single source of
//! truth and can be copied between machines as-is.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Current manifest schema. Bump this only for a breaking manifest change.
const SCHEMA: u32 = 1;
const MAX_ID: usize = 64;
const MAX_NAME: usize = 128;
const MAX_VERSION: usize = 32;
const MAX_DESCRIPTION: usize = 512;

/// Parsed and validated `theme.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
  pub id: String,
  pub name: String,
  pub version: Option<String>,
  pub description: Option<String>,
  pub preview: Option<String>,
}

/// The raw manifest. Unknown keys are rejected so a typo does not silently
/// become a field nobody reads.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
  #[serde(default = "default_schema")]
  schema: u32,
  id: String,
  name: String,
  #[serde(default)]
  version: Option<String>,
  #[serde(default)]
  description: Option<String>,
  #[serde(default)]
  preview: Option<String>,
}

fn default_schema() -> u32 {
  SCHEMA
}

impl Manifest {
  /// Reads and validates `<dir>/theme.toml`.
  pub fn load(dir: &Path) -> Result<Self, String> {
    let path = dir.join("theme.toml");
    let source = read_text(&path)?;
    let raw: RawManifest =
      toml::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))?;
    Self::validate(raw, &path)
  }

  fn validate(raw: RawManifest, path: &Path) -> Result<Self, String> {
    if raw.schema != SCHEMA {
      return Err(format!(
        "{}: unsupported manifest schema {} (expected {SCHEMA})",
        path.display(),
        raw.schema
      ));
    }
    if !valid_id(&raw.id) {
      return Err(format!(
        "{}: id {:?} must be 1-{MAX_ID} characters of lowercase letters, digits, '.', '_', or '-'",
        path.display(),
        raw.id
      ));
    }
    if raw.name.trim().is_empty() || raw.name.chars().count() > MAX_NAME {
      return Err(format!(
        "{}: name must be 1-{MAX_NAME} characters",
        path.display()
      ));
    }
    if let Some(version) = &raw.version
      && (version.is_empty() || version.chars().count() > MAX_VERSION)
    {
      return Err(format!(
        "{}: version must be 1-{MAX_VERSION} characters when present",
        path.display()
      ));
    }
    if let Some(description) = &raw.description
      && description.chars().count() > MAX_DESCRIPTION
    {
      return Err(format!(
        "{}: description must be at most {MAX_DESCRIPTION} characters",
        path.display()
      ));
    }
    if let Some(preview) = &raw.preview {
      validate_relative_asset(preview)
        .map_err(|message| format!("{}: preview {message}", path.display()))?;
      if !has_image_extension(&dir_of(path).join(preview)) {
        return Err(format!(
          "{}: preview {:?} must be PNG, JPEG, or WebP",
          path.display(),
          preview
        ));
      }
    }
    if !dir_of(path).join("config.toml").is_file() {
      return Err(format!(
        "{}: config.toml is missing from the bundle",
        path.display()
      ));
    }
    Ok(Self {
      id: raw.id,
      name: raw.name,
      version: raw.version,
      description: raw.description,
      preview: raw.preview,
    })
  }

  /// Absolute path to the optional preview image.
  #[must_use]
  pub fn preview_path(&self, dir: &Path) -> Option<PathBuf> {
    self.preview.as_ref().map(|preview| dir.join(preview))
  }
}

/// Theme ids become directory names, so keep the alphabet deliberately narrow.
#[must_use]
pub fn valid_id(id: &str) -> bool {
  if id.is_empty() || id.len() > MAX_ID || id.starts_with('.') {
    return false;
  }
  id.bytes().all(|byte| {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
  })
}

/// Checks a path from a manifest or config that must stay inside the bundle.
pub fn validate_relative_asset(value: &str) -> Result<(), String> {
  if value.is_empty() {
    return Err("must not be empty".to_string());
  }
  if value.starts_with('~') {
    return Err(format!("{value:?} must not use ~ inside a theme bundle"));
  }
  let path = Path::new(value);
  if path.is_absolute() {
    return Err(format!("{value:?} must be relative to the theme directory"));
  }
  for component in path.components() {
    if !matches!(
      component,
      std::path::Component::Normal(_) | std::path::Component::CurDir
    ) {
      return Err(format!("{value:?} must not contain '..' or a root prefix"));
    }
  }
  Ok(())
}

fn has_image_extension(path: &Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp"
      )
    })
}

fn read_text(path: &Path) -> Result<String, String> {
  const MAX_BYTES: u64 = 64 * 1024;
  let metadata =
    std::fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
  if !metadata.is_file() {
    return Err(format!("{} is not a regular file", path.display()));
  }
  if metadata.len() > MAX_BYTES {
    return Err(format!(
      "{} is {} bytes, larger than the {MAX_BYTES} byte manifest limit",
      path.display(),
      metadata.len()
    ));
  }
  std::fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn dir_of(path: &Path) -> &Path {
  path.parent().unwrap_or_else(|| Path::new("."))
}

#[cfg(test)]
mod tests {
  use super::{Manifest, valid_id, validate_relative_asset};

  fn manifest(source: &str) -> Result<Manifest, String> {
    let raw = toml::from_str(source).map_err(|error| error.to_string())?;
    Manifest::validate(raw, std::path::Path::new("theme.toml"))
  }

  #[test]
  fn valid_manifest_parses() {
    let dir = std::env::temp_dir().join(format!(
      "dawn-manifest-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(
      dir.join("config.toml"),
      "[theme]\npreset = \"glass-dark\"\n",
    )
    .expect("config");
    std::fs::write(
      dir.join("theme.toml"),
      r#"
schema = 1
id = "midnight"
name = "Midnight"
version = "1.0.0"
description = "A dark bundle."
"#,
    )
    .expect("manifest");
    let manifest = Manifest::load(&dir).expect("load manifest");
    assert_eq!(manifest.id, "midnight");
    assert_eq!(manifest.name, "Midnight");
    assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
    std::fs::remove_dir_all(dir).expect("cleanup");
  }

  #[test]
  fn bad_ids_are_rejected() {
    assert!(!valid_id(""));
    assert!(!valid_id(".hidden"));
    assert!(!valid_id("../escape"));
    assert!(!valid_id("Upper"));
    assert!(valid_id("midnight-2"));
  }

  #[test]
  fn assets_must_stay_relative() {
    assert!(validate_relative_asset("assets/rule.png").is_ok());
    assert!(validate_relative_asset("/etc/passwd").is_err());
    assert!(validate_relative_asset("../escape.png").is_err());
    assert!(validate_relative_asset("~/rule.png").is_err());
  }

  #[test]
  fn unknown_manifest_keys_are_rejected() {
    assert!(
      manifest(
        r#"
schema = 1
id = "midnight"
name = "Midnight"
typo = true
"#
      )
      .is_err()
    );
  }
}
