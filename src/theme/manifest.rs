//! Theme bundle manifests.
//!
//! A bundle is a directory, not an archive. Keeping it a directory means
//! `dawn add` can validate it, users can inspect it, and the greeter can read
//! assets straight from the installed location without unpacking anything at
//! boot. The manifest is the small, strict part.
//!
//! Two manifest generations are read:
//!
//! * DSB-2 (current), documented in `docs/bundle-spec.md`:
//!
//! ```toml
//! [spec]
//! revision = 2
//!
//! [bundle]
//! id = "midnight"
//! name = "Midnight"
//! version = "1.0.0"
//! description = "A short one-line description."
//! preview = "preview.webp"
//!
//! [bundle.engine]
//! min = "0.5.0"
//!
//! [[author]]
//! name = "Ada Grey"
//! role = "layout"
//! ```
//!
//! * DSB-1 (legacy), either with `[spec] revision = 1` and a `[bundle]` table,
//!   or with the older flat `schema = 1` keys. The six example bundles still
//!   use the flat form, and `dawn add` keeps accepting them until the admin
//!   runs the migration.
//!
//! A missing `[spec]` counts as revision 1 on purpose: a directory written
//! before the manifest carried a revision must keep working, not vanish from
//! the store. The spec validator is the only place that decides whether a
//! particular revision's required fields are present.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Highest spec revision this engine implements.
pub const CURRENT_REVISION: u32 = 2;

/// Legacy flat manifests declare this schema. They are revision 1.
const LEGACY_SCHEMA: u32 = 1;
const MAX_ID: usize = 64;
const MAX_NAME: usize = 128;
const MAX_VERSION: usize = 64;
const MAX_DESCRIPTION: usize = 512;
const MAX_AUTHORS: usize = 32;
const MAX_CREDITS: usize = 64;

/// Parsed and validated `theme.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
  /// DSB revision this bundle was written against.
  pub revision: u32,
  pub id: String,
  pub name: String,
  pub version: Option<String>,
  pub description: Option<String>,
  pub preview: Option<String>,
  /// `[bundle.engine].min` (or the legacy top-level `[engine].min`).
  pub engine_min: Option<String>,
  /// Authors in display order. At least one is required by DSB-2 rule 7.
  pub authors: Vec<Author>,
  pub credits: Vec<Credit>,
}

/// One `[[author]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
  pub name: String,
  /// What this person did. Required by DSB-2 so credit is meaningful.
  pub role: String,
  pub contact: Option<String>,
}

/// One optional `[[credit]]` entry. DSB-2 deliberately has no license fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
  pub name: String,
  /// The `for` key. Renamed in Rust because `for` is a keyword.
  pub what: String,
  pub via: Option<String>,
}

/// The raw manifest. Unknown keys are rejected so a typo does not silently
/// become a field nobody reads. Every field is optional here because the
/// revision decides which ones are required; that decision belongs to
/// [`validate`], not to serde.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
  #[serde(default)]
  schema: Option<u32>,
  #[serde(default)]
  spec: Option<RawSpec>,
  #[serde(default)]
  bundle: Option<RawBundle>,
  /// DSB-1 documented a top-level `[engine]`; DSB-2 nests it under the bundle.
  #[serde(default)]
  engine: Option<RawEngine>,
  #[serde(default)]
  id: Option<String>,
  #[serde(default)]
  name: Option<String>,
  #[serde(default)]
  version: Option<String>,
  #[serde(default)]
  description: Option<String>,
  #[serde(default)]
  preview: Option<String>,
  #[serde(default)]
  author: Vec<RawAuthor>,
  #[serde(default)]
  credit: Vec<RawCredit>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpec {
  #[serde(default)]
  revision: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBundle {
  #[serde(default)]
  id: Option<String>,
  #[serde(default)]
  name: Option<String>,
  #[serde(default)]
  version: Option<String>,
  #[serde(default)]
  description: Option<String>,
  #[serde(default)]
  preview: Option<String>,
  #[serde(default)]
  engine: Option<RawEngine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEngine {
  #[serde(default)]
  min: Option<String>,
  /// Accepted for DSB-1 compatibility. The engine does not read prose notes.
  #[serde(default, rename = "notes")]
  _notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthor {
  name: String,
  #[serde(default)]
  role: String,
  #[serde(default)]
  contact: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCredit {
  name: String,
  #[serde(rename = "for")]
  what: String,
  #[serde(default)]
  via: Option<String>,
}

impl Manifest {
  /// Reads and validates `<dir>/theme.toml`.
  ///
  /// Filesystem checks (config.toml and the preview file) are deliberately not
  /// done here. The spec validator owns those so a missing file is reported as
  /// its own rule instead of hiding behind a manifest parse error.
  pub fn load(dir: &Path) -> Result<Self, String> {
    let path = dir.join("theme.toml");
    let source = read_text(&path)?;
    let raw: RawManifest =
      toml::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))?;
    Self::validate(raw, &path)
  }

  fn validate(raw: RawManifest, path: &Path) -> Result<Self, String> {
    let revision = match &raw.spec {
      Some(spec) => spec.revision.unwrap_or(LEGACY_SCHEMA),
      None => raw.schema.unwrap_or(LEGACY_SCHEMA),
    };
    if revision != 1 && revision != CURRENT_REVISION {
      return Err(format!(
        "{}: unsupported spec revision {revision} (this engine knows 1 and {CURRENT_REVISION})",
        path.display()
      ));
    }
    if revision == CURRENT_REVISION && raw.schema.is_some() {
      return Err(format!(
        "{}: revision {CURRENT_REVISION} manifests must not use the legacy schema key",
        path.display()
      ));
    }
    if let Some(schema) = raw.schema
      && schema != LEGACY_SCHEMA
    {
      return Err(format!(
        "{}: unsupported legacy schema {schema} (expected {LEGACY_SCHEMA})",
        path.display()
      ));
    }

    let bundle = raw.bundle.as_ref();
    // Revision 1 accepts both the old flat keys and the documented DSB-1
    // `[bundle]` table. Revision 2 only accepts the table form.
    if revision == CURRENT_REVISION {
      if bundle.is_none() {
        return Err(format!(
          "{}: revision {CURRENT_REVISION} requires a [bundle] table",
          path.display()
        ));
      }
      if raw.id.is_some()
        || raw.name.is_some()
        || raw.version.is_some()
        || raw.description.is_some()
        || raw.preview.is_some()
      {
        return Err(format!(
          "{}: revision {CURRENT_REVISION} requires bundle fields under [bundle], not at the top level",
          path.display()
        ));
      }
    }

    let id = bundle
      .and_then(|bundle| bundle.id.clone())
      .or(raw.id)
      .ok_or_else(|| format!("{}: bundle id is missing", path.display()))?;
    let name = bundle
      .and_then(|bundle| bundle.name.clone())
      .or(raw.name)
      .ok_or_else(|| format!("{}: bundle name is missing", path.display()))?;
    let version = bundle
      .and_then(|bundle| bundle.version.clone())
      .or(raw.version);
    let description = bundle
      .and_then(|bundle| bundle.description.clone())
      .or(raw.description);
    let preview = bundle
      .and_then(|bundle| bundle.preview.clone())
      .or(raw.preview);
    let engine_min = bundle
      .and_then(|bundle| bundle.engine.as_ref())
      .or(raw.engine.as_ref())
      .and_then(|engine| engine.min.clone());

    if revision == CURRENT_REVISION {
      if !valid_spec_id(&id) {
        return Err(format!(
          "{}: bundle id {:?} must match [a-z0-9][a-z0-9_-]* and be 1-{MAX_ID} characters",
          path.display(),
          id
        ));
      }
      if !valid_semver(&version.clone().unwrap_or_default()) {
        return Err(format!(
          "{}: bundle version must be semantic versioning, for example 1.0.0",
          path.display()
        ));
      }
      if description
        .as_deref()
        .is_none_or(|text| text.trim().is_empty())
      {
        return Err(format!(
          "{}: bundle description is required by DSB-{CURRENT_REVISION}",
          path.display()
        ));
      }
      let min = engine_min.as_deref().unwrap_or_default();
      if !valid_semver(min) {
        return Err(format!(
          "{}: [bundle.engine] min must be semantic versioning, for example 0.5.0",
          path.display()
        ));
      }
    } else if !valid_id(&id) {
      return Err(format!(
        "{}: id {:?} must be 1-{MAX_ID} characters of lowercase letters, digits, '.', '_', or '-'",
        path.display(),
        id
      ));
    }

    if name.trim().is_empty() || name.chars().count() > MAX_NAME {
      return Err(format!(
        "{}: name must be 1-{MAX_NAME} characters",
        path.display()
      ));
    }
    if let Some(version) = &version
      && (version.is_empty() || version.chars().count() > MAX_VERSION)
    {
      return Err(format!(
        "{}: version must be 1-{MAX_VERSION} characters when present",
        path.display()
      ));
    }
    if let Some(description) = &description
      && description.chars().count() > MAX_DESCRIPTION
    {
      return Err(format!(
        "{}: description must be at most {MAX_DESCRIPTION} characters",
        path.display()
      ));
    }
    if let Some(preview) = &preview {
      validate_relative_asset(preview)
        .map_err(|message| format!("{}: preview {message}", path.display()))?;
      if !has_image_extension(Path::new(preview)) {
        return Err(format!(
          "{}: preview {:?} must be PNG, JPEG, or WebP",
          path.display(),
          preview
        ));
      }
    }
    if raw.author.len() > MAX_AUTHORS {
      return Err(format!(
        "{}: at most {MAX_AUTHORS} authors are allowed",
        path.display()
      ));
    }
    if raw.credit.len() > MAX_CREDITS {
      return Err(format!(
        "{}: at most {MAX_CREDITS} credits are allowed",
        path.display()
      ));
    }
    for author in &raw.author {
      if author.name.trim().is_empty() || author.role.trim().is_empty() {
        return Err(format!(
          "{}: every [[author]] needs a non-empty name and role",
          path.display()
        ));
      }
    }
    for credit in &raw.credit {
      if credit.name.trim().is_empty() || credit.what.trim().is_empty() {
        return Err(format!(
          "{}: every [[credit]] needs a non-empty name and for",
          path.display()
        ));
      }
    }

    Ok(Self {
      revision,
      id,
      name,
      version,
      description,
      preview,
      engine_min,
      authors: raw
        .author
        .into_iter()
        .map(|author| Author {
          name: author.name,
          role: author.role,
          contact: author.contact,
        })
        .collect(),
      credits: raw
        .credit
        .into_iter()
        .map(|credit| Credit {
          name: credit.name,
          what: credit.what,
          via: credit.via,
        })
        .collect(),
    })
  }

  /// Absolute path to the optional preview image.
  #[must_use]
  pub fn preview_path(&self, dir: &Path) -> Option<PathBuf> {
    self.preview.as_ref().map(|preview| dir.join(preview))
  }
}

/// Theme ids become directory names, so keep the alphabet deliberately narrow.
/// This is the legacy alphabet; DSB-2 uses [`valid_spec_id`].
#[must_use]
pub fn valid_id(id: &str) -> bool {
  if id.is_empty() || id.len() > MAX_ID || id.starts_with('.') {
    return false;
  }
  id.bytes().all(|byte| {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
  })
}

/// DSB-2 id alphabet: `[a-z0-9][a-z0-9_-]*`. No dots, and the first character
/// must be alphanumeric so an id can never look like a hidden directory.
#[must_use]
pub fn valid_spec_id(id: &str) -> bool {
  let mut bytes = id.bytes();
  let Some(first) = bytes.next() else {
    return false;
  };
  if id.len() > MAX_ID || !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
    return false;
  }
  bytes
    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
}

/// A small semver check. We do not need ordering for anything except the
/// engine minimum, so this accepts the shape and keeps the pre-release and
/// build suffixes verbatim instead of pretending to be a full semver library.
#[must_use]
pub fn valid_semver(value: &str) -> bool {
  parse_semver(value).is_some()
}

/// Parses `major.minor.patch` and ignores any `-pre` or `+build` suffix. This
/// is enough for the one comparison DSB-2 needs: does the running engine meet
/// `[bundle.engine].min`?
#[must_use]
pub fn parse_semver(value: &str) -> Option<(u64, u64, u64)> {
  let core = value.split(['-', '+']).next()?;
  let mut parts = core.split('.');
  let major = parts.next()?.parse().ok()?;
  let minor = parts.next()?.parse().ok()?;
  let patch = parts.next()?.parse().ok()?;
  if parts.next().is_some() {
    return None;
  }
  Some((major, minor, patch))
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

#[cfg(test)]
mod tests {
  use super::{Manifest, valid_id, valid_semver, valid_spec_id, validate_relative_asset};

  fn manifest(source: &str) -> Result<Manifest, String> {
    let raw = toml::from_str(source).map_err(|error| error.to_string())?;
    Manifest::validate(raw, std::path::Path::new("theme.toml"))
  }

  #[test]
  fn legacy_flat_manifest_parses_as_revision_one() {
    let manifest = manifest(
      r#"
schema = 1
id = "midnight"
name = "Midnight"
version = "1.0.0"
description = "A dark bundle."
"#,
    )
    .expect("legacy manifest");
    assert_eq!(manifest.revision, 1);
    assert_eq!(manifest.id, "midnight");
    assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
  }

  #[test]
  fn revision_two_bundle_table_parses() {
    let manifest = manifest(
      r#"
[spec]
revision = 2

[bundle]
id = "midnight"
name = "Midnight"
version = "1.0.0"
description = "A dark bundle."
preview = "preview.webp"

[bundle.engine]
min = "0.5.0"

[[author]]
name = "Ada Grey"
role = "layout"

[[credit]]
name = "Ilya"
for = "Illustration"
via = "https://example.com"
"#,
    )
    .expect("revision 2 manifest");
    assert_eq!(manifest.revision, 2);
    assert_eq!(manifest.engine_min.as_deref(), Some("0.5.0"));
    assert_eq!(manifest.authors.len(), 1);
    assert_eq!(manifest.credits.len(), 1);
  }

  #[test]
  fn absent_revision_is_legacy_and_unknown_revisions_are_rejected() {
    let legacy = manifest("id = \"x\"\nname = \"X\"\n").expect("absent revision");
    assert_eq!(legacy.revision, 1);
    let error = manifest("[spec]\nrevision = 3\n[bundle]\nid = \"x\"\nname = \"X\"\n")
      .expect_err("unknown revision");
    assert!(error.contains("unsupported spec revision 3"), "{error}");
  }

  #[test]
  fn bad_ids_are_rejected() {
    assert!(!valid_id(""));
    assert!(!valid_id(".hidden"));
    assert!(!valid_id("../escape"));
    assert!(!valid_id("Upper"));
    assert!(valid_id("midnight-2"));

    assert!(valid_spec_id("midnight-2"));
    assert!(valid_spec_id("2fast"));
    assert!(!valid_spec_id(".hidden"));
    assert!(!valid_spec_id("with.dot"));
    assert!(!valid_spec_id("-leading"));
    assert!(!valid_spec_id("Upper"));
  }

  #[test]
  fn semver_shape_is_checked_without_ordering() {
    assert!(valid_semver("1.0.0"));
    assert!(valid_semver("0.5.0-rc.1"));
    assert!(valid_semver("1.2.3+build.7"));
    assert!(!valid_semver("1.0"));
    assert!(!valid_semver("one.two.three"));
    assert!(!valid_semver("1.2.3.4"));
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
