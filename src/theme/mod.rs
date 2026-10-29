//! Theme bundles and the on-disk theme store.
//!
//! A bundle is a directory with `theme.toml`, a complete `config.toml`, and
//! every asset both files reference. `dawn add` copies it into a store, and
//! `dawn --theme ID` (or `[theme].package = "ID"`) loads the stored config.
//! Relative paths in that config resolve against the stored directory, so the
//! config and its assets can never drift apart.
//!
//! There are two stores, checked in this order:
//!
//! 1. `$XDG_DATA_HOME/dawn/themes` (or `~/.local/share/dawn/themes`)
//! 2. `/usr/share/dawn/themes`
//!
//! The user store wins when both contain the same id. `dawn add` writes to the
//! user store unless `--system` is passed. A real greeter normally runs as the
//! `greeter` user with a tiny home, so system themes belong in the system
//! store.

pub mod cli;
mod install;
mod manifest;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub use install::AddedTheme;
use manifest::Manifest;
pub use manifest::valid_id;

/// Where a theme came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
  User,
  System,
}

impl fmt::Display for Origin {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::User => f.write_str("user"),
      Self::System => f.write_str("system"),
    }
  }
}

/// One installed theme, ready for `dawn list`.
#[derive(Debug, Clone)]
pub struct ThemeEntry {
  pub id: String,
  pub name: String,
  pub version: Option<String>,
  pub description: Option<String>,
  pub preview: Option<PathBuf>,
  pub path: PathBuf,
  pub origin: Origin,
}

/// The user theme store, when HOME or XDG_DATA_HOME gives us one.
#[must_use]
pub fn user_root() -> Option<PathBuf> {
  if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
    return Some(PathBuf::from(xdg).join("dawn/themes"));
  }
  let home = std::env::var_os("HOME").filter(|value| !value.is_empty())?;
  Some(PathBuf::from(home).join(".local/share/dawn/themes"))
}

/// The system theme store. Package managers and admins own this one.
#[must_use]
pub fn system_root() -> PathBuf {
  PathBuf::from("/usr/share/dawn/themes")
}

fn roots() -> Vec<(PathBuf, Origin)> {
  let mut roots = Vec::new();
  if let Some(user) = user_root() {
    roots.push((user, Origin::User));
  }
  roots.push((system_root(), Origin::System));
  // Extra read-only roots are useful for immutable systems such as NixOS,
  // where a theme lives in the Nix store. `dawn add` never writes here.
  if let Some(extra) = std::env::var_os("DAWN_THEME_DIRS") {
    for dir in std::env::split_paths(&extra) {
      if !dir.as_os_str().is_empty() {
        roots.push((dir, Origin::System));
      }
    }
  }
  roots
}

/// Finds an installed theme by id, user store first.
#[must_use]
pub fn find(id: &str) -> Option<PathBuf> {
  find_in(&roots(), id).map(|(path, _)| path)
}

/// Looks up an id in an explicit root list. Kept separate so tests never have
/// to mutate process-wide environment variables.
pub(crate) fn find_in(roots: &[(PathBuf, Origin)], id: &str) -> Option<(PathBuf, ThemeEntry)> {
  if !valid_id(id) {
    return None;
  }
  for (root, origin) in roots {
    let dir = root.join(id);
    if std::fs::symlink_metadata(&dir).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
      continue;
    }
    if !dir.join("theme.toml").is_file() {
      continue;
    }
    let Ok(manifest) = Manifest::load(&dir) else {
      continue;
    };
    if manifest.id == id {
      let preview = manifest.preview_path(&dir);
      return Some((
        dir.clone(),
        ThemeEntry {
          id: manifest.id,
          name: manifest.name,
          version: manifest.version,
          description: manifest.description,
          preview,
          path: dir,
          origin: *origin,
        },
      ));
    }
  }
  None
}

/// Lists installed themes. A broken bundle produces a warning, not a failed
/// command, so one bad folder cannot make the rest of the store invisible.
#[must_use]
pub fn list() -> (Vec<ThemeEntry>, Vec<String>) {
  list_in(&roots())
}

pub(crate) fn list_in(roots: &[(PathBuf, Origin)]) -> (Vec<ThemeEntry>, Vec<String>) {
  let mut entries: BTreeMap<String, ThemeEntry> = BTreeMap::new();
  let mut warnings = Vec::new();
  for (root, origin) in roots {
    let read = match std::fs::read_dir(root) {
      Ok(read) => read,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
      Err(error) => {
        warnings.push(format!("cannot read {}: {error}", root.display()));
        continue;
      }
    };
    for entry in read {
      let entry = match entry {
        Ok(entry) => entry,
        Err(error) => {
          warnings.push(format!("cannot read {}: {error}", root.display()));
          continue;
        }
      };
      let dir = entry.path();
      let metadata = match std::fs::symlink_metadata(&dir) {
        Ok(metadata) => metadata,
        Err(error) => {
          warnings.push(format!("cannot inspect {}: {error}", dir.display()));
          continue;
        }
      };
      if metadata.file_type().is_symlink() || !metadata.is_dir() {
        continue;
      }
      if !dir.join("theme.toml").is_file() {
        continue;
      }
      let manifest = match Manifest::load(&dir) {
        Ok(manifest) => manifest,
        Err(error) => {
          warnings.push(format!("skipping {}: {error}", dir.display()));
          continue;
        }
      };
      if manifest.id != entry.file_name().to_string_lossy() {
        warnings.push(format!(
          "skipping {}: manifest id {:?} does not match the directory name",
          dir.display(),
          manifest.id
        ));
        continue;
      }
      // User entries are inserted first and win over a system entry with the
      // same id. This matches the lookup order in `find`.
      let preview = manifest.preview_path(&dir);
      entries.entry(manifest.id.clone()).or_insert(ThemeEntry {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        preview,
        path: dir,
        origin: *origin,
      });
    }
  }
  (entries.into_values().collect(), warnings)
}

/// Installs bundles into the user store, or the system store with `system`.
pub fn add(paths: &[PathBuf], force: bool, system: bool) -> Result<Vec<AddedTheme>, String> {
  let (root, origin) = if system {
    (system_root(), Origin::System)
  } else {
    (
      user_root().ok_or_else(|| {
        "cannot determine the user data directory; set XDG_DATA_HOME or HOME".to_string()
      })?,
      Origin::User,
    )
  };
  install::add(&root, origin, paths, force).map_err(|error| {
    if system && !root.exists() {
      format!(
        "{error}\nfix: create {} with the right permissions first",
        root.display()
      )
    } else {
      error
    }
  })
}

/// Removes a theme from the user store, or from the system store with `system`.
pub fn remove(id: &str, system: bool) -> Result<PathBuf, String> {
  if !valid_id(id) {
    return Err(format!("invalid theme id: {id:?}"));
  }
  let root = if system {
    system_root()
  } else {
    user_root().ok_or_else(|| {
      "cannot determine the user data directory; set XDG_DATA_HOME or HOME".to_string()
    })?
  };
  if !root.join(id).exists() {
    if !system && system_root().join(id).exists() {
      return Err(format!(
        "theme {id:?} is a system theme; rerun with --system if you really mean it"
      ));
    }
    return Err(format!(
      "theme {id:?} is not installed in {}",
      root.display()
    ));
  }
  install::remove(&root, id)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{Origin, list_in};

  fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-store-{name}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
  }

  fn install(root: &std::path::Path, id: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("create theme");
    std::fs::write(
      dir.join("theme.toml"),
      format!("schema = 1\nid = \"{id}\"\nname = \"{id}\"\n"),
    )
    .expect("manifest");
    std::fs::write(
      dir.join("config.toml"),
      "[theme]\npreset = \"glass-dark\"\n",
    )
    .expect("config");
  }

  #[test]
  fn user_themes_shadow_system_themes() {
    let user = temp_dir("user");
    let system = temp_dir("system");
    install(&user, "same");
    install(&system, "same");
    install(&system, "only-system");
    let (entries, warnings) = list_in(&[(user, Origin::User), (system, Origin::System)]);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(entries.len(), 2);
    let same = entries
      .iter()
      .find(|entry| entry.id == "same")
      .expect("same");
    assert_eq!(same.origin, Origin::User);
  }

  #[test]
  fn broken_theme_is_a_warning_not_a_failure() {
    let root = temp_dir("broken");
    let bad = root.join("bad");
    std::fs::create_dir_all(&bad).expect("create");
    std::fs::write(bad.join("theme.toml"), "schema = 1\nid = \"bad\"\n").expect("manifest");
    let (entries, warnings) = list_in(&[(root, Origin::User)]);
    assert!(entries.is_empty());
    assert_eq!(warnings.len(), 1);
  }
}
