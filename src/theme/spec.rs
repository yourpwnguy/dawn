//! The DSB-2 bundle validator.
//!
//! `dawn check` and `dawn add` both call [`check_tree`]. Keeping one validator
//! means an uninstalled tree is judged by exactly the same code that decides
//! whether it may enter a store, which is the whole point of a bundle spec.
//!
//! Every rejection carries a rule number from `docs/bundle-spec.md` and the
//! file it came from. That shape is deliberate: an admin reading
//! `rule 2: config.toml: image "wallpaper.mp4" must live under assets/` can go
//! straight to the normative sentence instead of guessing which check failed.
//!
//! Revision 1 trees are still accepted. They get one warning that names the
//! migration command, and the documented DSB-1 looseness applies: authors are
//! optional and the preview has no dimension or size ceiling. Revision 2 gets
//! the strict checks.

use std::fmt;
use std::path::Path;

use walkdir::WalkDir;

use super::manifest::{CURRENT_REVISION, Manifest, parse_semver, valid_spec_id};

/// The preview cap from DSB-2 rule 1.
const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PREVIEW_WIDTH: u32 = 1600;
const MAX_PREVIEW_HEIGHT: u32 = 1080;
/// The file name the layout mandates. The manifest has to agree.
pub const PREVIEW_FILE: &str = "preview.webp";

/// One rule violation, ready to print as `rule <n>: <file>: <what>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleIssue {
  /// 1-based rule number from `docs/bundle-spec.md`.
  pub rule: u8,
  /// Bundle-relative file or directory, for example `preview.webp`.
  pub file: String,
  pub message: String,
}

impl fmt::Display for RuleIssue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "rule {}: {}: {}", self.rule, self.file, self.message)
  }
}

/// Everything the validator learned about one tree.
#[derive(Debug, Clone)]
pub struct TreeCheck {
  /// `None` when `theme.toml` could not be parsed at all.
  pub revision: Option<u32>,
  pub manifest: Option<Manifest>,
  pub issues: Vec<RuleIssue>,
  pub warnings: Vec<String>,
}

impl TreeCheck {
  #[must_use]
  pub fn ok(&self) -> bool {
    self.issues.is_empty()
  }

  /// The report `dawn add` puts after its error prefix. Keeping it one string
  /// makes the install path and `dawn check` print the same text.
  #[must_use]
  pub fn report(&self) -> String {
    self
      .issues
      .iter()
      .map(ToString::to_string)
      .collect::<Vec<_>>()
      .join("\n")
  }
}

/// Validates an uninstalled bundle tree against the highest revision it
/// declares. Filesystem problems become rule issues rather than an early
/// error, so a missing preview does not hide a broken config.
#[must_use]
pub fn check_tree(dir: &Path) -> TreeCheck {
  let mut check = TreeCheck {
    revision: None,
    manifest: None,
    issues: Vec::new(),
    warnings: Vec::new(),
  };

  if !dir.is_dir() {
    check.issues.push(issue(
      1,
      dir.display().to_string(),
      "not a bundle directory",
    ));
    return check;
  }

  let manifest = match Manifest::load(dir) {
    Ok(manifest) => manifest,
    Err(error) => {
      check
        .issues
        .push(issue(5, "theme.toml", manifest_error(&error, dir)));
      return check;
    }
  };
  check.revision = Some(manifest.revision);
  check.manifest = Some(manifest.clone());

  if manifest.revision == CURRENT_REVISION {
    check_artifacts(dir, &manifest, &mut check);
    check_engine(&manifest, &mut check);
    if manifest.authors.is_empty() {
      check.issues.push(issue(
        7,
        "theme.toml",
        "at least one [[author]] is required",
      ));
    }
    check_root_files(dir, &mut check);
  } else {
    // Exactly one warning line. The command is part of the warning so a user
    // who runs `dawn check` on an old bundle knows the next step.
    check.warnings.push(format!(
      "theme.toml: revision {} is legacy; migrate it with: dawn add {} --migrate --force",
      manifest.revision,
      super::shell_quote(dir)
    ));
    check_legacy_artifacts(dir, &manifest, &mut check);
  }

  check_symlinks(dir, &mut check);
  check_config(dir, manifest.revision, &mut check);
  check
}

fn check_artifacts(dir: &Path, manifest: &Manifest, check: &mut TreeCheck) {
  let expected_id = dir.file_name().map(|name| name.to_string_lossy());
  if expected_id.as_deref() != Some(manifest.id.as_str()) {
    check.issues.push(issue(
      1,
      "theme.toml",
      format!(
        "bundle id {:?} must equal the directory name {:?}",
        manifest.id,
        expected_id.as_deref().unwrap_or_default()
      ),
    ));
  }
  if !valid_spec_id(&manifest.id) {
    check.issues.push(issue(
      6,
      "theme.toml",
      format!("bundle id {:?} must match [a-z0-9][a-z0-9_-]*", manifest.id),
    ));
  }
  if !dir.join("config.toml").is_file() {
    check.issues.push(issue(
      1,
      "config.toml",
      "the complete run config is missing",
    ));
  }
  if !dir.join("assets").is_dir() {
    check
      .issues
      .push(issue(1, "assets/", "the assets directory is missing"));
  }
  if manifest.preview.as_deref() != Some(PREVIEW_FILE) {
    check.issues.push(issue(
      1,
      "theme.toml",
      format!("preview must be {PREVIEW_FILE:?}"),
    ));
  }
  check_preview(dir, check);
}

/// The legacy tree only needs a manifest and a config. A declared preview must
/// still exist, but DSB-1 never specified bounds.
fn check_legacy_artifacts(dir: &Path, manifest: &Manifest, check: &mut TreeCheck) {
  if !dir.join("config.toml").is_file() {
    check.issues.push(issue(
      1,
      "config.toml",
      "the complete run config is missing",
    ));
  }
  if let Some(preview) = &manifest.preview
    && !dir.join(preview).is_file()
  {
    check.issues.push(issue(
      1,
      preview,
      "the preview named by the manifest does not exist",
    ));
  }
}

fn check_preview(dir: &Path, check: &mut TreeCheck) {
  if let Some(message) = preview_issue(&dir.join(PREVIEW_FILE)) {
    check.issues.push(issue(1, PREVIEW_FILE, message));
  }
}

/// Returns the reason a preview file is not acceptable, or `None` when it is.
/// Migration calls this too: a legacy preview may be valid DSB-1 media but
/// outside the revision 2 bounds, in which case migration regenerates it.
pub(crate) fn preview_issue(path: &Path) -> Option<String> {
  let metadata = match std::fs::metadata(path) {
    Ok(metadata) if metadata.is_file() => metadata,
    Ok(_) => return Some("not a regular file".to_string()),
    Err(error) => return Some(format!("missing: {error}")),
  };
  if metadata.len() > MAX_PREVIEW_BYTES {
    return Some(format!(
      "{} bytes, larger than the {} byte limit",
      metadata.len(),
      MAX_PREVIEW_BYTES
    ));
  }
  let dimensions = match image::image_dimensions(path) {
    Ok(dimensions) => dimensions,
    Err(error) => return Some(format!("not a readable WebP image: {error}")),
  };
  if dimensions.0 > MAX_PREVIEW_WIDTH || dimensions.1 > MAX_PREVIEW_HEIGHT {
    return Some(format!(
      "{}x{} exceeds the {}x{} limit",
      dimensions.0, dimensions.1, MAX_PREVIEW_WIDTH, MAX_PREVIEW_HEIGHT
    ));
  }
  // Dimensions only read the header. Decode the pixels too so a truncated or
  // corrupt file cannot slip into the store just because its header looks fine.
  decode_webp(path)
    .err()
    .map(|error| format!("not a valid WebP image: {error}"))
}

/// Full decode with the same dimension ceiling as the header check. The file
/// size cap above keeps this bounded even before the decoder sees it.
fn decode_webp(path: &Path) -> Result<(), String> {
  let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
  let mut reader =
    image::ImageReader::with_format(std::io::BufReader::new(file), image::ImageFormat::WebP);
  let mut limits = image::Limits::default();
  limits.max_image_width = Some(MAX_PREVIEW_WIDTH);
  limits.max_image_height = Some(MAX_PREVIEW_HEIGHT);
  reader.limits(limits);
  reader
    .decode()
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn check_engine(manifest: &Manifest, check: &mut TreeCheck) {
  let Some(min) = manifest.engine_min.as_deref() else {
    return;
  };
  let (Some(required), Some(current)) =
    (parse_semver(min), parse_semver(env!("CARGO_PKG_VERSION")))
  else {
    return;
  };
  if required > current {
    check.issues.push(issue(
      5,
      "theme.toml",
      format!(
        "bundle requires dawn >= {min}, this engine is {}",
        env!("CARGO_PKG_VERSION")
      ),
    ));
  }
}

/// DSB-2 keeps media under `assets/`. A file sitting at the bundle root is the
/// DSB-1 layout; migration is the only thing that moves it.
fn check_root_files(dir: &Path, check: &mut TreeCheck) {
  let entries = match std::fs::read_dir(dir) {
    Ok(entries) => entries,
    Err(error) => {
      check.issues.push(issue(
        1,
        "theme.toml",
        format!("cannot read the bundle: {error}"),
      ));
      return;
    }
  };
  for entry in entries.flatten() {
    let name = entry.file_name().to_string_lossy().into_owned();
    let Ok(metadata) = entry.metadata() else {
      continue;
    };
    if metadata.is_file() && !matches!(name.as_str(), "theme.toml" | "config.toml" | PREVIEW_FILE) {
      check.issues.push(issue(
        2,
        &name,
        "root-level media is the DSB-1 layout; move it into assets/ (see dawn add --migrate)",
      ));
    }
  }
}

fn check_symlinks(dir: &Path, check: &mut TreeCheck) {
  for entry in WalkDir::new(dir).follow_links(false).sort_by_file_name() {
    let entry = match entry {
      Ok(entry) => entry,
      Err(error) => {
        check.issues.push(issue(
          1,
          "theme.toml",
          format!("cannot walk the bundle: {error}"),
        ));
        return;
      }
    };
    if entry.file_type().is_symlink() {
      check.issues.push(issue(
        3,
        relative(dir, entry.path()),
        "symbolic links are not allowed in a bundle",
      ));
    }
  }
}

fn check_config(dir: &Path, revision: u32, check: &mut TreeCheck) {
  let path = dir.join("config.toml");
  if !path.is_file() {
    return;
  }
  let loaded = crate::config::load_theme(&path);
  for problem in &loaded.issues {
    check
      .issues
      .push(issue(4, "config.toml", problem.to_string()));
  }
  let source = match std::fs::read_to_string(&path) {
    Ok(source) => source,
    Err(error) => {
      check.issues.push(issue(
        4,
        "config.toml",
        format!("cannot read the file: {error}"),
      ));
      return;
    }
  };
  let value: toml::Value = match toml::from_str(&source) {
    Ok(value) => value,
    // The loader already reported the parse failure as rule 4.
    Err(_) => return,
  };
  // A bundle config is read-only data. Writing dragged positions back into it
  // would fight the package manager that owns the store.
  if value
    .get("layout")
    .and_then(|layout| layout.get("save"))
    .and_then(toml::Value::as_bool)
    == Some(true)
  {
    check.issues.push(issue(
      4,
      "config.toml",
      "layout.save must be false; theme bundles are read-only",
    ));
  }
  check_references(&value, &[], dir, revision, check);
}

/// Walks the parsed config looking for asset references. We only treat `image`
/// keys and `[plugins].dirs` as bundle assets. `sessions.dirs` points at
/// system-wide desktop entries and is intentionally not a bundle path.
fn check_references(
  value: &toml::Value,
  path: &[&str],
  dir: &Path,
  revision: u32,
  check: &mut TreeCheck,
) {
  match value {
    toml::Value::Table(table) => {
      for (key, child) in table {
        let mut next = path.to_vec();
        next.push(key.as_str());
        if key == "image"
          && let Some(reference) = child.as_str()
        {
          check_reference(reference, "image", dir, revision, check);
        }
        if key == "file"
          && path.first() == Some(&"audio")
          && let Some(reference) = child.as_str()
        {
          let ambient = path.get(1) == Some(&"ambient");
          check_reference(reference, "audio file", dir, revision, check);
          check_audio_limits(reference, ambient, dir, check);
        }
        if key == "dirs"
          && path == ["plugins"]
          && let Some(entries) = child.as_array()
        {
          for entry in entries {
            if let Some(reference) = entry.as_str() {
              check_reference(reference, "plugin directory", dir, revision, check);
            }
          }
        }
        check_references(child, &next, dir, revision, check);
      }
    }
    toml::Value::Array(values) => {
      for value in values {
        check_references(value, path, dir, revision, check);
      }
    }
    _ => {}
  }
}

/// Audio format and size caps. They mirror the loader so a bundle that passes
/// `dawn check` cannot fail later because of an oversized or unsupported file.
fn check_audio_limits(reference: &str, ambient: bool, dir: &Path, check: &mut TreeCheck) {
  let lower = reference.to_ascii_lowercase();
  if !(lower.ends_with(".ogg") || lower.ends_with(".wav")) {
    check.issues.push(issue(
      2,
      "config.toml",
      format!("audio file {reference:?} must be .ogg or .wav"),
    ));
    return;
  }
  let Ok(metadata) = std::fs::metadata(dir.join(reference)) else {
    return;
  };
  let limit = if ambient {
    16 * 1024 * 1024
  } else {
    2 * 1024 * 1024
  };
  if metadata.len() > limit {
    check.issues.push(issue(
      2,
      "config.toml",
      format!(
        "audio file {reference:?} is {} bytes; the limit is {limit}",
        metadata.len()
      ),
    ));
  }
}

fn check_reference(reference: &str, what: &str, dir: &Path, revision: u32, check: &mut TreeCheck) {
  if super::manifest::validate_relative_asset(reference).is_err() {
    check.issues.push(issue(
      3,
      "config.toml",
      format!("{what} {reference:?} must be relative and must not contain '..'"),
    ));
    return;
  }
  if revision == CURRENT_REVISION {
    let inside_assets = Path::new(reference)
      .components()
      .next()
      .is_some_and(|component| component.as_os_str() == "assets");
    if !inside_assets {
      check.issues.push(issue(
        2,
        "config.toml",
        format!("{what} {reference:?} must live under assets/"),
      ));
      return;
    }
  }
  let resolved = dir.join(reference);
  if !resolved.exists() {
    check.issues.push(issue(
      2,
      "config.toml",
      format!("{what} {reference:?} does not exist in the bundle"),
    ));
  }
}

fn issue(rule: u8, file: impl Into<String>, message: impl Into<String>) -> RuleIssue {
  RuleIssue {
    rule,
    file: file.into(),
    message: message.into(),
  }
}

fn relative(dir: &Path, path: &Path) -> String {
  path.strip_prefix(dir).unwrap_or(path).display().to_string()
}

/// `Manifest::load` already prefixes the manifest path. The report also names
/// the file, so strip the duplicate prefix once instead of printing it twice.
fn manifest_error(error: &str, dir: &Path) -> String {
  let prefix = format!("{}: ", dir.join("theme.toml").display());
  error.strip_prefix(&prefix).unwrap_or(error).to_string()
}

#[cfg(test)]
mod tests;
