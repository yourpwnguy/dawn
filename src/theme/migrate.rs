//! Mechanical DSB-1 to DSB-2 migration for `dawn add --migrate`.
//!
//! Migration is copy-first. We copy the tree into a sibling staging directory,
//! move root-level media into `assets/`, patch the two config keys that pointed
//! at those files, rewrite the manifest as revision 2, and run the DSB-2
//! validator on the result. Only a fully valid staging tree may replace the
//! source, and only when `--force` was passed.
//!
//! There is no clever in-place editing. If a legacy config uses a reference we
//! do not understand, validation fails and the source is untouched, which is a
//! much better outcome than a half-migrated theme.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tempfile::Builder;

use super::install;
use super::manifest::{Manifest, valid_semver};
use super::scaffold;
use super::spec::{PREVIEW_FILE, check_tree};

/// What one upgrade did, for the report `dawn add --migrate` prints.
#[derive(Debug)]
pub struct Upgrade {
  /// The migrated tree to install. This is either the rewritten source or a
  /// live staging directory owned by this value.
  pub tree: PathBuf,
  /// `(before, after)` pairs for root files moved into assets/.
  pub moved: Vec<(String, String)>,
  pub generated_preview: bool,
  pub replaced_source: bool,
  /// Dropping this cleans the staging directory in the non-destructive case.
  _staging: Option<tempfile::TempDir>,
}

/// Upgrades one revision 1 bundle tree.
///
/// `replace_source` is the CLI's `--force`. When it is false the original tree
/// is left alone and the caller installs the staged copy; when it is true the
/// staged tree is renamed over the original after validation.
pub fn upgrade(bundle: &Path, replace_source: bool) -> Result<Upgrade, String> {
  let manifest = Manifest::load(bundle)?;
  if manifest.revision != 1 {
    return Err(format!(
      "{}: already revision {}; nothing to migrate",
      bundle.display(),
      manifest.revision
    ));
  }
  let parent = bundle
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));
  let staging = Builder::new()
    .prefix(".dawn-migrate-")
    .tempdir_in(parent)
    .map_err(|error| {
      format!(
        "cannot create a staging directory in {}: {error}",
        parent.display()
      )
    })?;
  // The staging tree itself must carry the bundle id: DSB-2 rule 1 checks the
  // directory name, and the staged tree is validated before it replaces the
  // source. A temp parent with an id-named child keeps both true.
  let staged = staging.path().join(&manifest.id);
  std::fs::create_dir(&staged)
    .map_err(|error| format!("cannot create {}: {error}", staged.display()))?;
  let mut total = 0;
  install::copy_tree(bundle, &staged, &mut total)?;

  let generated_preview = ensure_preview(&staged, &manifest)?;
  let moved = move_root_media(&staged)?;
  patch_config_references(&staged, &moved)?;
  write_manifest(&staged, &manifest)?;

  let check = check_tree(&staged);
  if !check.ok() {
    return Err(format!(
      "{}: migrated tree does not satisfy DSB-2\n{}",
      bundle.display(),
      check.report()
    ));
  }

  if replace_source {
    replace(bundle, &staged)?;
    Ok(Upgrade {
      tree: bundle.to_path_buf(),
      moved,
      generated_preview,
      replaced_source: true,
      _staging: Some(staging),
    })
  } else {
    Ok(Upgrade {
      tree: staged,
      moved,
      generated_preview,
      replaced_source: false,
      _staging: Some(staging),
    })
  }
}

/// Swaps a validated staging tree over the source. The old tree is kept in a
/// temp sibling until the new one is in place, so a failed rename can roll
/// back instead of leaving the user with nothing.
fn replace(source: &Path, staging: &Path) -> Result<(), String> {
  let parent = source
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));
  let backup = Builder::new()
    .prefix(".dawn-migrate-old-")
    .tempdir_in(parent)
    .map_err(|error| {
      format!(
        "cannot create a backup directory in {}: {error}",
        parent.display()
      )
    })?
    .keep();
  std::fs::rename(source, &backup)
    .map_err(|error| format!("cannot move the old bundle aside: {error}"))?;
  if let Err(error) = std::fs::rename(staging, source) {
    let _ = std::fs::rename(&backup, source);
    return Err(format!(
      "cannot move the migrated bundle into place: {error}"
    ));
  }
  if let Err(error) = std::fs::remove_dir_all(&backup) {
    eprintln!(
      "warning: migrated the bundle but could not remove {}: {error}",
      backup.display()
    );
  }
  Ok(())
}

/// DSB-2 requires a WebP preview. A legacy tree may have none or may have a
/// differently named one; in both cases we generate the canonical file. The
/// old preview, if it existed, is treated like any other root-level media and
/// moved into assets/ by [`move_root_media`].
fn ensure_preview(root: &Path, manifest: &Manifest) -> Result<bool, String> {
  let path = root.join(PREVIEW_FILE);
  if manifest.preview.as_deref() == Some(PREVIEW_FILE)
    && path.is_file()
    && super::spec::preview_issue(&path).is_none()
  {
    return Ok(false);
  }
  std::fs::write(&path, scaffold::generated_preview()?)
    .map_err(|error| format!("cannot write {PREVIEW_FILE}: {error}"))?;
  Ok(true)
}

/// Moves every root-level file except the three manifest-level artifacts into
/// assets/. Directories are left alone; DSB-2 only cares about files that
/// config paths could reference.
fn move_root_media(root: &Path) -> Result<Vec<(String, String)>, String> {
  let assets = root.join("assets");
  std::fs::create_dir_all(&assets)
    .map_err(|error| format!("cannot create {}: {error}", assets.display()))?;
  let entries =
    std::fs::read_dir(root).map_err(|error| format!("cannot read {}: {error}", root.display()))?;
  let mut names = Vec::new();
  for entry in entries {
    let entry = entry.map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    let name = entry.file_name().to_string_lossy().into_owned();
    if matches!(
      name.as_str(),
      "theme.toml" | "config.toml" | PREVIEW_FILE | "assets"
    ) {
      continue;
    }
    let metadata = std::fs::symlink_metadata(entry.path())
      .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
    if metadata.is_file() {
      names.push(name);
    }
  }
  names.sort();
  let mut moved = Vec::new();
  for name in names {
    let target = assets.join(&name);
    if target.exists() {
      return Err(format!(
        "cannot move {name:?} into assets/: a file with that name already exists"
      ));
    }
    std::fs::rename(root.join(&name), &target)
      .map_err(|error| format!("cannot move {name:?} into assets/: {error}"))?;
    moved.push((name.clone(), format!("assets/{name}")));
  }
  Ok(moved)
}

/// Rewrites the two path-valued keys migration knows about: `image` (the only
/// asset key in the config vocabulary) and `art` (kept because older themes
/// used it for album art files). Matching is exact, so a value like "cover" or
/// an already-correct "assets/foo.png" is never touched.
fn patch_config_references(root: &Path, moved: &[(String, String)]) -> Result<(), String> {
  if moved.is_empty() {
    return Ok(());
  }
  let map: BTreeMap<&str, &str> = moved
    .iter()
    .map(|(from, to)| (from.as_str(), to.as_str()))
    .collect();
  let path = root.join("config.toml");
  let source = std::fs::read_to_string(&path)
    .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
  let patched = patch_source(&source, &map);
  std::fs::write(&path, patched)
    .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn patch_source(source: &str, moved: &BTreeMap<&str, &str>) -> String {
  let mut output = String::with_capacity(source.len());
  for line in source.split_inclusive('\n') {
    output.push_str(&patch_line(line, moved));
  }
  output
}

fn patch_line(line: &str, moved: &BTreeMap<&str, &str>) -> String {
  let trimmed = line.trim_start();
  let Some((key, value)) = trimmed.split_once('=') else {
    return line.to_string();
  };
  let leaf = key.trim().rsplit('.').next().unwrap_or_default();
  if leaf != "image" && leaf != "art" {
    return line.to_string();
  }
  let value = value.trim_start();
  let (quote, rest) = match value.chars().next() {
    Some(quote @ ('"' | '\'')) => (quote, &value[1..]),
    _ => return line.to_string(),
  };
  let Some(end) = rest.find(quote) else {
    return line.to_string();
  };
  let reference = &rest[..end];
  let Some(target) = moved.get(reference) else {
    return line.to_string();
  };
  let literal = format!("{quote}{reference}{quote}");
  let replacement = format!("{quote}{target}{quote}");
  line.replacen(&literal, &replacement, 1)
}

/// Emits a revision 2 manifest. Missing required fields get a visible TODO
/// instead of a silent default, because an author name is not something the
/// tool should invent on a real bundle.
fn write_manifest(root: &Path, manifest: &Manifest) -> Result<(), String> {
  let version = manifest
    .version
    .as_deref()
    .filter(|version| valid_semver(version))
    .unwrap_or("0.1.0");
  let version_note = if manifest.version.as_deref().is_some_and(valid_semver) {
    ""
  } else {
    " # TODO: your version"
  };
  let description = manifest
    .description
    .as_deref()
    .filter(|description| !description.trim().is_empty())
    .unwrap_or("TODO: describe this theme in one line.");
  let description_note = if manifest
    .description
    .as_deref()
    .is_some_and(|description| !description.trim().is_empty())
  {
    ""
  } else {
    " # TODO: your description"
  };
  let engine_min = manifest
    .engine_min
    .as_deref()
    .filter(|min| valid_semver(min))
    .unwrap_or("0.5.0");

  let mut text = String::new();
  text.push_str("# DSB-2 bundle manifest. The normative text is docs/bundle-spec.md.\n");
  text.push_str("[spec]\nrevision = 2\n\n[bundle]\n");
  text.push_str(&format!("id = {}\n", toml_string(&manifest.id)));
  text.push_str(&format!("name = {}\n", toml_string(&manifest.name)));
  text.push_str(&format!(
    "version = {}{version_note}\n",
    toml_string(version)
  ));
  text.push_str(&format!(
    "description = {}{description_note}\n",
    toml_string(description)
  ));
  text.push_str(&format!("preview = {}\n\n", toml_string(PREVIEW_FILE)));
  text.push_str("[bundle.engine]\n");
  text.push_str(&format!("min = {}\n\n", toml_string(engine_min)));

  if manifest.authors.is_empty() {
    text.push_str("# TODO: replace this placeholder with your name and role.\n");
    text.push_str("[[author]]\nname = \"Your Name\"\nrole = \"author\"\n");
  } else {
    for author in &manifest.authors {
      text.push_str("[[author]]\n");
      text.push_str(&format!("name = {}\n", toml_string(&author.name)));
      text.push_str(&format!("role = {}\n", toml_string(&author.role)));
      if let Some(contact) = &author.contact {
        text.push_str(&format!("contact = {}\n", toml_string(contact)));
      }
    }
  }
  for credit in &manifest.credits {
    text.push_str("\n[[credit]]\n");
    text.push_str(&format!("name = {}\n", toml_string(&credit.name)));
    text.push_str(&format!("for = {}\n", toml_string(&credit.what)));
    if let Some(via) = &credit.via {
      text.push_str(&format!("via = {}\n", toml_string(via)));
    }
  }

  std::fs::write(root.join("theme.toml"), text)
    .map_err(|error| format!("cannot write theme.toml: {error}"))
}

fn toml_string(value: &str) -> String {
  let mut escaped = String::with_capacity(value.len() + 2);
  escaped.push('"');
  for character in value.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      '\n' => escaped.push_str("\\n"),
      '\r' => escaped.push_str("\\r"),
      '\t' => escaped.push_str("\\t"),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::path::PathBuf;

  use super::{patch_source, upgrade};
  use crate::theme::spec::check_tree;

  fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-migrate-{name}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
  }

  fn legacy_bundle(source: &std::path::Path, id: &str) -> PathBuf {
    let dir = source.join(id);
    std::fs::create_dir_all(&dir).expect("create bundle");
    std::fs::write(
      dir.join("theme.toml"),
      format!("schema = 1\nid = \"{id}\"\nname = \"Legacy\"\n"),
    )
    .expect("manifest");
    std::fs::write(
      dir.join("config.toml"),
      "[background]\nkind = \"image\"\nimage = \"wallpaper.mp4\"\n\n[layout]\nsave = false\n",
    )
    .expect("config");
    std::fs::write(dir.join("wallpaper.mp4"), b"not really a video").expect("wallpaper");
    dir
  }

  #[test]
  fn migration_moves_media_patches_config_and_passes_check() {
    let source = temp_dir("complete");
    let dir = legacy_bundle(&source, "legacy");
    let upgrade = upgrade(&dir, false).expect("migrate");
    let check = check_tree(&upgrade.tree);
    assert!(check.ok(), "{:?}", check.issues);
    assert_eq!(check.revision, Some(2));
    assert_eq!(
      upgrade.moved,
      vec![("wallpaper.mp4".into(), "assets/wallpaper.mp4".into())]
    );
    let config = std::fs::read_to_string(upgrade.tree.join("config.toml")).expect("config");
    assert!(
      config.contains("image = \"assets/wallpaper.mp4\""),
      "{config}"
    );
    assert!(upgrade.tree.join("assets/wallpaper.mp4").is_file());
    // The source tree is untouched until --force says otherwise.
    assert!(dir.join("wallpaper.mp4").is_file());
    assert!(!dir.join("assets").exists());
    std::fs::remove_dir_all(&source).expect("cleanup");
  }

  #[test]
  fn force_replaces_the_source_tree() {
    let source = temp_dir("force");
    let dir = legacy_bundle(&source, "legacy");
    let upgrade = upgrade(&dir, true).expect("migrate");
    assert!(upgrade.replaced_source);
    assert!(dir.join("assets/wallpaper.mp4").is_file());
    assert!(!dir.join("wallpaper.mp4").exists());
    let manifest = std::fs::read_to_string(dir.join("theme.toml")).expect("manifest");
    assert!(manifest.contains("revision = 2"), "{manifest}");
    assert!(manifest.contains("Your Name"), "{manifest}");
    std::fs::remove_dir_all(&source).expect("cleanup");
  }

  #[test]
  fn a_missing_preview_is_generated() {
    let source = temp_dir("preview");
    let dir = legacy_bundle(&source, "legacy");
    std::fs::write(
      dir.join("theme.toml"),
      "schema = 1\nid = \"legacy\"\nname = \"Legacy\"\n",
    )
    .expect("manifest");
    let upgrade = upgrade(&dir, true).expect("migrate");
    assert!(upgrade.generated_preview);
    assert!(dir.join("preview.webp").is_file());
    std::fs::remove_dir_all(&source).expect("cleanup");
  }

  #[test]
  fn an_out_of_bounds_preview_is_regenerated() {
    let source = temp_dir("big-preview");
    let dir = legacy_bundle(&source, "legacy");
    let pixels = vec![0u8; 1920 * 1080 * 3];
    let bytes = crate::theme::scaffold::encode_webp(&pixels, 1920, 1080).expect("encode");
    std::fs::write(dir.join("preview.webp"), bytes).expect("preview");
    std::fs::write(
      dir.join("theme.toml"),
      "schema = 1\nid = \"legacy\"\nname = \"Legacy\"\npreview = \"preview.webp\"\n",
    )
    .expect("manifest");
    let upgrade = upgrade(&dir, true).expect("migrate");
    assert!(upgrade.generated_preview);
    assert_eq!(
      image::image_dimensions(dir.join("preview.webp")).expect("dimensions"),
      (640, 400)
    );
    std::fs::remove_dir_all(&source).expect("cleanup");
  }

  #[test]
  fn only_exact_path_values_are_patched() {
    let source = "[media]\nart = \"cover\"\n[background]\nimage = 'wallpaper.mp4' # keep\n";
    let moved = BTreeMap::from([("wallpaper.mp4", "assets/wallpaper.mp4")]);
    let patched = patch_source(source, &moved);
    assert!(patched.contains("art = \"cover\""), "{patched}");
    assert!(
      patched.contains("image = 'assets/wallpaper.mp4' # keep"),
      "{patched}"
    );
  }
}
