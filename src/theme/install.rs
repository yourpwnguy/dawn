//! Installing, validating, and removing theme bundles.
//!
//! The install is staged first: the whole bundle is copied into a hidden
//! directory under the store, validated there, and only then renamed into
//! place. A validation error or a crash leaves the previous theme untouched.
//! Replacement is two renames (old out, new in) with a rollback if the second
//! one fails.
//!
//! We reject symlinks and special files in a bundle. A greeter runs with a
//! small, known permission set on a machine that may be shared, and a theme is
//! data. Letting a theme point at `/etc/shadow` through a symlink because the
//! admin copied a folder from somewhere is not a tradeoff worth making.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::Builder;
use walkdir::WalkDir;

use super::manifest::{Manifest, validate_relative_asset};
use super::{Origin, ThemeEntry};

/// A theme can carry a video, so the total cap is generous. It still exists so
/// a mistaken `dawn add /` fails before filling the disk.
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug)]
pub struct AddedTheme {
  pub entry: ThemeEntry,
  pub replaced: bool,
}

/// Copies one or more bundles into `root`.
pub fn add(
  root: &Path,
  origin: Origin,
  paths: &[PathBuf],
  force: bool,
) -> Result<Vec<AddedTheme>, String> {
  let mut bundles = Vec::new();
  for path in paths {
    bundles.extend(discover(path)?);
  }
  if bundles.is_empty() {
    return Err("no theme bundles found".to_string());
  }

  fs::create_dir_all(root)
    .map_err(|error| format!("cannot create theme store {}: {error}", root.display()))?;
  set_dir_mode(root)?;

  let mut added = Vec::new();
  for bundle in bundles {
    added.push(add_one(root, origin, &bundle, force)?);
  }
  Ok(added)
}

/// Finds the bundle directories named by one `dawn add` argument.
///
/// A path with `theme.toml` is a single bundle. A directory whose immediate
/// children contain `theme.toml` is a collection. We deliberately do not walk
/// the whole tree: a typo should not install every theme under a home
/// directory.
fn discover(path: &Path) -> Result<Vec<PathBuf>, String> {
  if !path.exists() {
    return Err(format!("{}: no such file or directory", path.display()));
  }
  if path.join("theme.toml").is_file() {
    return Ok(vec![path.to_path_buf()]);
  }
  if !path.is_dir() {
    return Err(format!(
      "{}: expected a theme directory containing theme.toml",
      path.display()
    ));
  }
  let mut bundles = Vec::new();
  let entries =
    fs::read_dir(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
  for entry in entries {
    let entry = entry.map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let child = entry.path();
    if child.is_dir() && child.join("theme.toml").is_file() {
      bundles.push(child);
    }
  }
  bundles.sort();
  if bundles.is_empty() {
    return Err(format!(
      "{}: no theme bundles found (expected theme.toml here or in an immediate child)",
      path.display()
    ));
  }
  Ok(bundles)
}

fn add_one(root: &Path, origin: Origin, bundle: &Path, force: bool) -> Result<AddedTheme, String> {
  let manifest = Manifest::load(bundle)?;
  if manifest
    .preview_path(bundle)
    .is_some_and(|preview| !preview.is_file())
  {
    return Err(format!(
      "{}: preview {:?} does not exist",
      bundle.display(),
      manifest.preview.as_deref().unwrap_or_default()
    ));
  }
  let target = root.join(&manifest.id);
  let replaced = match fs::symlink_metadata(&target) {
    Ok(metadata) => {
      if metadata.file_type().is_symlink() {
        return Err(format!(
          "{}: refusing to replace a symlink in the theme store",
          target.display()
        ));
      }
      if !force {
        return Err(format!(
          "theme {:?} is already installed at {}; pass --force to replace it",
          manifest.id,
          target.display()
        ));
      }
      true
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
    Err(error) => {
      return Err(format!("cannot inspect {}: {error}", target.display()));
    }
  };

  let staging = Builder::new()
    .prefix(&format!(".dawn-stage-{}-", manifest.id))
    .tempdir_in(root)
    .map_err(|error| {
      format!(
        "cannot create a staging directory in {}: {error}",
        root.display()
      )
    })?;
  set_dir_mode(staging.path())?;

  let mut total = 0;
  copy_tree(bundle, staging.path(), &mut total)?;
  validate_staged(staging.path(), &manifest)?;

  if replaced {
    let backup = Builder::new()
      .prefix(&format!(".dawn-old-{}-", manifest.id))
      .tempdir_in(root)
      .map_err(|error| {
        format!(
          "cannot create a backup directory in {}: {error}",
          root.display()
        )
      })?;
    let backup = backup.keep();
    if let Err(error) = fs::rename(&target, &backup) {
      let _ = fs::remove_dir_all(&backup);
      return Err(format!("cannot move the old theme aside: {error}"));
    }
    let staging = staging.keep();
    match fs::rename(&staging, &target) {
      Ok(()) => {
        if let Err(error) = fs::remove_dir_all(&backup) {
          eprintln!(
            "warning: installed the new theme but could not remove {}: {error}",
            backup.display()
          );
        }
      }
      Err(error) => {
        let _ = fs::rename(&backup, &target);
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("cannot move the new theme into place: {error}"));
      }
    }
  } else {
    let staging = staging.keep();
    if let Err(error) = fs::rename(&staging, &target) {
      let _ = fs::remove_dir_all(&staging);
      return Err(format!("cannot move the theme into place: {error}"));
    }
  }

  let preview = manifest.preview_path(&target);
  Ok(AddedTheme {
    entry: ThemeEntry {
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      description: manifest.description,
      preview,
      path: target,
      origin,
    },
    replaced,
  })
}

fn validate_staged(staging: &Path, expected: &Manifest) -> Result<(), String> {
  let manifest = Manifest::load(staging)?;
  if manifest.id != expected.id {
    return Err(format!(
      "{}: theme.toml changed while copying",
      staging.display()
    ));
  }

  let config_path = staging.join("config.toml");
  let loaded = crate::config::load_theme(&config_path);
  if !loaded.issues.is_empty() {
    let mut message = format!("{}: config validation failed", config_path.display());
    for issue in loaded.issues.iter().take(8) {
      message.push_str(&format!("\n  {issue}"));
    }
    if loaded.issues.len() > 8 {
      message.push_str(&format!("\n  ... and {} more", loaded.issues.len() - 8));
    }
    return Err(message);
  }

  let source = fs::read_to_string(&config_path)
    .map_err(|error| format!("cannot read {}: {error}", config_path.display()))?;
  let value: toml::Value =
    toml::from_str(&source).map_err(|error| format!("{}: {error}", config_path.display()))?;
  validate_config_paths(&value, &[], staging)
}

/// Walks the parsed config and rejects values that would break portability or
/// try to write into a read-only theme store.
fn validate_config_paths(value: &toml::Value, path: &[&str], root: &Path) -> Result<(), String> {
  match value {
    toml::Value::Table(table) => {
      for (key, child) in table {
        let mut next = path.to_vec();
        next.push(key.as_str());
        if key == "package" && path == ["theme"] {
          return Err("config.toml must not select another theme with [theme].package".to_string());
        }
        if key == "save" && path == ["layout"] && child.as_bool() == Some(true) {
          return Err(
            "config.toml must set layout.save = false; theme bundles are read-only".to_string(),
          );
        }
        if key == "image"
          && let Some(value) = child.as_str()
        {
          validate_relative_asset(value)?;
          let asset = root.join(value);
          if !asset.is_file() {
            return Err(format!("image {value:?} does not exist in the bundle"));
          }
        }
        if key == "dirs"
          && path == ["plugins"]
          && let Some(values) = child.as_array()
        {
          for value in values {
            let Some(value) = value.as_str() else {
              continue;
            };
            validate_relative_asset(value)?;
            let dir = root.join(value);
            if !dir.is_dir() {
              return Err(format!("plugin directory {value:?} does not exist"));
            }
          }
        }
        validate_config_paths(child, &next, root)?;
      }
      Ok(())
    }
    toml::Value::Array(values) => {
      for value in values {
        validate_config_paths(value, path, root)?;
      }
      Ok(())
    }
    _ => Ok(()),
  }
}

fn copy_tree(source: &Path, target: &Path, total: &mut u64) -> Result<(), String> {
  for entry in WalkDir::new(source).follow_links(false).sort_by_file_name() {
    let entry = entry.map_err(|error| format!("cannot walk {}: {error}", source.display()))?;
    let from = entry.path();
    let Ok(relative) = from.strip_prefix(source) else {
      return Err(format!("{}: escaped the bundle root", from.display()));
    };
    if relative.as_os_str().is_empty() {
      continue;
    }
    let to = target.join(relative);
    let metadata = fs::symlink_metadata(from)
      .map_err(|error| format!("cannot inspect {}: {error}", from.display()))?;
    if metadata.file_type().is_symlink() {
      return Err(format!(
        "{}: symlinks are not allowed in a theme bundle",
        from.display()
      ));
    }
    if metadata.is_dir() {
      fs::create_dir(&to).map_err(|error| format!("cannot create {}: {error}", to.display()))?;
      set_dir_mode(&to)?;
    } else if metadata.is_file() {
      *total = total.saturating_add(metadata.len());
      if *total > MAX_TOTAL_BYTES {
        return Err(format!(
          "theme bundle is larger than the {} GiB limit",
          MAX_TOTAL_BYTES / 1024 / 1024 / 1024
        ));
      }
      fs::copy(from, &to).map_err(|error| format!("cannot copy {}: {error}", from.display()))?;
      set_file_mode(&to)?;
    } else {
      return Err(format!(
        "{}: only regular files and directories are allowed",
        from.display()
      ));
    }
  }
  Ok(())
}

fn set_dir_mode(path: &Path) -> Result<(), String> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
      .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))?;
  }
  Ok(())
}

fn set_file_mode(path: &Path) -> Result<(), String> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
      .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))?;
  }
  Ok(())
}

pub fn remove(root: &Path, id: &str) -> Result<PathBuf, String> {
  let target = root.join(id);
  if !target.exists() {
    return Err(format!(
      "theme {id:?} is not installed at {}",
      root.display()
    ));
  }
  let metadata = fs::symlink_metadata(&target)
    .map_err(|error| format!("cannot inspect {}: {error}", target.display()))?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(format!(
      "{}: refusing to remove something that is not a theme directory",
      target.display()
    ));
  }
  fs::remove_dir_all(&target)
    .map_err(|error| format!("cannot remove {}: {error}", target.display()))?;
  Ok(target)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{add, remove};
  use crate::theme::Origin;

  fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-theme-{name}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
  }

  fn bundle(root: &std::path::Path, id: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("assets")).expect("create bundle");
    std::fs::write(
      dir.join("theme.toml"),
      format!("schema = 1\nid = \"{id}\"\nname = \"Test\"\n"),
    )
    .expect("manifest");
    std::fs::write(
      dir.join("config.toml"),
      "[theme]\npreset = \"glass-dark\"\n\n[layout]\nsave = false\n",
    )
    .expect("config");
    dir
  }

  #[test]
  fn add_list_find_remove_round_trip() {
    let source = temp_dir("source");
    let store = temp_dir("store");
    let dir = bundle(&source, "sample");
    let added = add(&store, Origin::User, &[dir], false).expect("add");
    assert_eq!(added.len(), 1);
    assert!(store.join("sample").join("theme.toml").is_file());
    let entry = super::super::find_in(&[(store.clone(), Origin::User)], "sample")
      .expect("find")
      .1;
    assert_eq!(entry.name, "Test");
    remove(&store, "sample").expect("remove");
    assert!(!store.join("sample").exists());
  }

  #[test]
  fn batch_add_finds_immediate_children() {
    let source = temp_dir("batch");
    let store = temp_dir("batch-store");
    bundle(&source, "one");
    bundle(&source, "two");
    let added = add(&store, Origin::User, &[source], false).expect("batch add");
    assert_eq!(added.len(), 2);
  }

  #[test]
  fn duplicate_add_needs_force() {
    let source = temp_dir("dupe");
    let store = temp_dir("dupe-store");
    let dir = bundle(&source, "sample");
    add(&store, Origin::User, std::slice::from_ref(&dir), false).expect("first add");
    assert!(add(&store, Origin::User, std::slice::from_ref(&dir), false).is_err());
    assert!(add(&store, Origin::User, &[dir], true).is_ok());
  }

  #[test]
  fn portable_paths_are_enforced() {
    let source = temp_dir("portable");
    let store = temp_dir("portable-store");
    let dir = bundle(&source, "sample");
    std::fs::write(
      dir.join("config.toml"),
      "[background]\nimage = \"/etc/passwd\"\n\n[layout]\nsave = false\n",
    )
    .expect("config");
    let error = add(&store, Origin::User, &[dir], false).unwrap_err();
    assert!(error.contains("relative"), "{error}");
  }

  #[cfg(unix)]
  #[test]
  fn symlinks_are_rejected() {
    let source = temp_dir("symlink");
    let store = temp_dir("symlink-store");
    let dir = bundle(&source, "sample");
    std::os::unix::fs::symlink("/etc/passwd", dir.join("assets").join("passwd")).expect("symlink");
    let error = add(&store, Origin::User, &[dir], false).unwrap_err();
    assert!(error.contains("symlinks"), "{error}");
  }
}
