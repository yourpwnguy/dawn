//! Theme bundles and the on-disk theme store.
//!
//! A DSB-2 bundle is a directory with `theme.toml`, a complete `config.toml`,
//! `preview.webp`, and an `assets/` root. `spec` is the single validator behind
//! both `dawn check` and `dawn add`; the normative text is
//! `docs/bundle-spec.md`. `dawn add` copies a validated bundle into a store,
//! and `dawn --theme ID` (or `[theme].package = "ID"`) loads the stored config.
//! Relative paths in that config resolve against the stored directory, so the
//! config and its assets can never drift apart.
//!
//! Stores are checked in this order:
//!
//! 1. `/usr/share/dawn/themes` (the canonical system store)
//! 2. every `DAWN_THEME_DIRS` entry, left to right
//! 3. `$XDG_DATA_HOME/dawn/themes` (or `~/.local/share/dawn/themes`)
//!
//! The system stores win when more than one contains the same id. The user
//! store is a compatibility fallback: a theme that exists only there still
//! loads, but dawn warns that it should be reinstalled into the system store.
//! `dawn add` writes to the system store by default; `--user` opts back into
//! the XDG store for packaging tests and sandboxes. A real greeter normally
//! runs as the `greeter` user with a tiny home, so boot-time themes belong in
//! `/usr/share/dawn/themes`.

pub mod cli;
mod install;
mod manifest;
mod migrate;
mod scaffold;
pub mod select;
mod spec;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

pub use install::AddedTheme;
use manifest::Manifest;
pub use manifest::valid_id;
pub use spec::{RuleIssue, TreeCheck};

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
  roots_from(user_root(), std::env::var_os("DAWN_THEME_DIRS"))
}

/// Builds the lookup order from explicit inputs so tests never have to mutate
/// process-wide environment variables.
///
/// The system store is canonical and always first. `DAWN_THEME_DIRS` entries
/// are extra system stores (NixOS and other immutable systems put themes in a
/// read-only store). The user store is last because it is a deprecated
/// fallback, not a competing preference.
fn roots_from(user: Option<PathBuf>, extra: Option<OsString>) -> Vec<(PathBuf, Origin)> {
  let mut roots = vec![(system_root(), Origin::System)];
  if let Some(extra) = extra {
    for dir in std::env::split_paths(&extra) {
      if !dir.as_os_str().is_empty() {
        roots.push((dir, Origin::System));
      }
    }
  }
  if let Some(user) = user {
    roots.push((user, Origin::User));
  }
  roots
}

/// Finds an installed theme by id. System stores win; a theme that only exists
/// in the deprecated user store still loads, but the fallback is logged.
#[must_use]
pub fn find(id: &str) -> Option<PathBuf> {
  let (path, _entry, warning) = find_in_with_warning(&roots(), id)?;
  if let Some(warning) = warning {
    tracing::warn!("{warning}");
  }
  Some(path)
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

/// Looks up an id and reports whether the result came from the deprecated user
/// store. The warning is kept separate from `find_in` so installation tests can
/// keep using the plain tuple and callers with a logging channel decide how to
/// surface it.
pub(crate) fn find_in_with_warning(
  roots: &[(PathBuf, Origin)],
  id: &str,
) -> Option<(PathBuf, ThemeEntry, Option<String>)> {
  let (path, entry) = find_in(roots, id)?;
  let warning = (entry.origin == Origin::User).then(|| {
    format!(
      "theme {id:?} was loaded from the deprecated user store; reinstall it into the system store: sudo dawn add {} --force",
      shell_quote(&path)
    )
  });
  Some((path, entry, warning))
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
      // The first root that supplies an id wins. `roots` puts the system
      // stores first, so a stale user copy can never shadow the canonical
      // one. This matches the lookup order in `find`.
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

/// Chooses the add target. The system store is the default; the user store is
/// the `--user` opt-in and is kept for packaging tests and sandboxes.
fn store_for_add(system: bool, user: Option<PathBuf>) -> Result<(PathBuf, Origin), String> {
  if system {
    return Ok((system_root(), Origin::System));
  }
  user.map(|root| (root, Origin::User)).ok_or_else(|| {
    "cannot determine the user data directory; set XDG_DATA_HOME or HOME".to_string()
  })
}

/// Installs bundles into the system store by default, or the user store with
/// `system == false` (the CLI's `--user`).
pub fn add(paths: &[PathBuf], force: bool, system: bool) -> Result<Vec<AddedTheme>, String> {
  let (root, origin) = store_for_add(system, user_root())?;
  add_at(&root, origin, paths, force, system)
}

/// Installs bundles, optionally upgrading revision 1 trees on the way.
///
/// With `migrate`, each bundle is copied to a sibling staging tree, upgraded,
/// and checked before anything is installed. `force` has its usual store
/// meaning and additionally rewrites the source tree; without it the user's
/// original directory is left byte for byte intact.
pub fn add_with_migration(
  paths: &[PathBuf],
  force: bool,
  system: bool,
  migrate: bool,
) -> Result<Vec<AddedTheme>, String> {
  if !migrate {
    return add(paths, force, system);
  }
  let (root, origin) = store_for_add(system, user_root())?;
  let mut upgrades = Vec::new();
  for path in paths {
    for bundle in install::discover(path)? {
      let upgrade = migrate::upgrade(&bundle, force)?;
      for (from, to) in &upgrade.moved {
        println!("moved {from} -> {to}");
      }
      if upgrade.generated_preview {
        println!("generated {}", spec::PREVIEW_FILE);
      }
      if upgrade.replaced_source {
        println!("rewrote {} as revision 2", bundle.display());
      }
      upgrades.push(upgrade);
    }
  }
  if upgrades.is_empty() {
    return Err("no theme bundles found".to_string());
  }
  let trees: Vec<PathBuf> = upgrades
    .iter()
    .map(|upgrade| upgrade.tree.clone())
    .collect();
  add_at_hinted(&root, origin, &trees, paths, force, system)
}

/// Validates an uninstalled tree exactly the way `dawn add` does.
#[must_use]
pub fn check_tree(path: &Path) -> TreeCheck {
  spec::check_tree(path)
}

/// Creates a minimal DSB-2 bundle with `dawn new`.
pub fn new_bundle(parent: &Path, id: &str, name: &str) -> Result<PathBuf, String> {
  scaffold::create(parent, id, name)
}

/// The three lines `dawn new` prints after a successful scaffold.
#[must_use]
pub fn new_next_steps(path: &Path, id: &str) -> [String; 3] {
  scaffold::next_steps(path, id)
}

/// Shared by `add` and the tests: installs into an explicit root and turns a
/// permission failure in the system store into an actionable sudo command.
/// Nothing here tries to escalate privileges.
pub(crate) fn add_at(
  root: &Path,
  origin: Origin,
  paths: &[PathBuf],
  force: bool,
  system: bool,
) -> Result<Vec<AddedTheme>, String> {
  add_at_hinted(root, origin, paths, paths, force, system)
}

/// Same as [`add_at`], but the sudo rerun line quotes `hint_paths`.
///
/// Migration installs from a temporary staging tree. Telling an admin to rerun
/// `sudo dawn add /tmp/.dawn-migrate-...` would be useless, so the hint keeps
/// pointing at the source paths they typed.
fn add_at_hinted(
  root: &Path,
  origin: Origin,
  paths: &[PathBuf],
  hint_paths: &[PathBuf],
  force: bool,
  system: bool,
) -> Result<Vec<AddedTheme>, String> {
  install::add(root, origin, paths, force).map_err(|error| {
    if system && is_permission_denied(&error) {
      format!(
        "{error}\nrerun with sudo: sudo dawn add {} --force",
        quote_paths(hint_paths)
      )
    } else if system && !root.exists() {
      format!(
        "{error}\nfix: create {} with the right permissions first",
        root.display()
      )
    } else {
      error
    }
  })
}

/// Removes a theme from the system store by default, or the user store with
/// `system == false` (the CLI's `--user`).
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
    if system {
      if user_root().is_some_and(|user| user.join(id).exists()) {
        return Err(format!(
          "theme {id:?} is in the user store; rerun with --user"
        ));
      }
    } else if system_root().join(id).exists() {
      return Err(format!(
        "theme {id:?} is in the system store; rerun without --user"
      ));
    }
    return Err(format!(
      "theme {id:?} is not installed in {}",
      root.display()
    ));
  }
  install::remove(&root, id).map_err(|error| {
    if system && is_permission_denied(&error) {
      format!("{error}\nrerun with sudo: sudo dawn remove {id}")
    } else {
      error
    }
  })
}

/// `install` reports user-facing strings instead of typed errors, so this is
/// the one place that recognises the OS permission failure. std spells it both
/// as a message and as a raw errno, depending on how the error was built.
pub(crate) fn is_permission_denied(error: &str) -> bool {
  error.contains("Permission denied")
    || error.contains("Operation not permitted")
    || error.contains("(os error 13)")
    || error.contains("(os error 1)")
}

fn quote_paths(paths: &[PathBuf]) -> String {
  paths
    .iter()
    .map(|path| shell_quote(path))
    .collect::<Vec<_>>()
    .join(" ")
}

/// Quotes a path for the one-line rerun hint. Plain theme paths stay readable;
/// anything with spaces or shell metacharacters gets single quotes.
pub(crate) fn shell_quote(path: &Path) -> String {
  let text = path.to_string_lossy();
  let safe = text
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'));
  if safe {
    text.into_owned()
  } else {
    format!("'{}'", text.replace('\'', "'\\''"))
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{
    Origin, add_at, find_in_with_warning, list_in, roots_from, store_for_add, system_root,
  };

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
  fn system_themes_shadow_user_themes() {
    let user = temp_dir("user");
    let system = temp_dir("system");
    install(&user, "same");
    install(&system, "same");
    install(&system, "only-system");
    let (entries, warnings) = list_in(&[(system, Origin::System), (user, Origin::User)]);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(entries.len(), 2);
    let same = entries
      .iter()
      .find(|entry| entry.id == "same")
      .expect("same");
    assert_eq!(same.origin, Origin::System);
  }

  #[test]
  fn find_prefers_the_system_store_and_does_not_warn() {
    let user = temp_dir("find-user");
    let system = temp_dir("find-system");
    install(&user, "same");
    install(&system, "same");
    let (path, entry, warning) = find_in_with_warning(
      &[(system.clone(), Origin::System), (user, Origin::User)],
      "same",
    )
    .expect("find");
    assert_eq!(path, system.join("same"));
    assert_eq!(entry.origin, Origin::System);
    assert!(warning.is_none());
  }

  #[test]
  fn user_store_fallback_warns_and_still_loads() {
    let user = temp_dir("fallback-user");
    let system = temp_dir("fallback-system");
    install(&user, "legacy");
    let (path, entry, warning) = find_in_with_warning(
      &[(system, Origin::System), (user.clone(), Origin::User)],
      "legacy",
    )
    .expect("find");
    assert_eq!(path, user.join("legacy"));
    assert_eq!(entry.origin, Origin::User);
    let warning = warning.expect("user-store fallback must warn");
    assert!(warning.contains("deprecated user store"), "{warning}");
    assert!(warning.contains("sudo dawn add"), "{warning}");
    assert_eq!(warning.matches("sudo dawn add").count(), 1, "{warning}");
  }

  #[test]
  fn dawn_theme_dirs_are_checked_before_the_user_store() {
    let extra_a = temp_dir("extra-a");
    let extra_b = temp_dir("extra-b");
    let user = temp_dir("extra-user");
    let extra = std::env::join_paths([&extra_a, &extra_b]).expect("join extra dirs");
    let roots = roots_from(Some(user.clone()), Some(extra));
    assert_eq!(roots[0], (system_root(), Origin::System));
    assert_eq!(roots[1], (extra_a.clone(), Origin::System));
    assert_eq!(roots[2], (extra_b, Origin::System));
    assert_eq!(roots[3], (user, Origin::User));

    install(&extra_a, "from-extra");
    let (path, entry, warning) =
      find_in_with_warning(&roots, "from-extra").expect("find in DAWN_THEME_DIRS");
    assert_eq!(path, extra_a.join("from-extra"));
    assert_eq!(entry.origin, Origin::System);
    assert!(warning.is_none());
  }

  #[test]
  fn system_is_the_default_add_target() {
    let user = temp_dir("add-target-user");
    let (root, origin) = store_for_add(true, Some(user.clone())).expect("system target");
    assert_eq!(root, system_root());
    assert_eq!(origin, Origin::System);

    let (root, origin) = store_for_add(false, Some(user.clone())).expect("user target");
    assert_eq!(root, user);
    assert_eq!(origin, Origin::User);
    assert!(store_for_add(false, None).is_err());
  }

  #[test]
  fn add_user_writes_the_given_user_root() {
    let source = temp_dir("add-user-source");
    install(&source, "sample");
    let store = temp_dir("add-user-store");
    let added =
      add_at(&store, Origin::User, &[source.join("sample")], false, false).expect("user add");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].entry.origin, Origin::User);
    assert!(store.join("sample").join("theme.toml").is_file());
  }

  #[test]
  fn permission_errors_are_recognized_in_both_std_spellings() {
    assert!(super::is_permission_denied(
      "cannot create theme store /usr/share/dawn/themes: Permission denied (os error 13)"
    ));
    assert!(super::is_permission_denied(
      "cannot set permissions on /usr/share/dawn/themes: Operation not permitted (os error 1)"
    ));
    assert!(!super::is_permission_denied(
      "cannot read /tmp/x: No such file or directory (os error 2)"
    ));
  }

  #[cfg(unix)]
  #[test]
  fn system_add_permission_error_prints_one_sudo_hint() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let parent = temp_dir("locked-parent");
    let root = parent.join("themes");
    let source = temp_dir("locked-source");
    install(&source, "sample");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).expect("lock parent");

    // A root test runner ignores the mode bits, so there is no EACCES to
    // reproduce. Skip instead of asserting a behaviour the OS will not show.
    if fs::create_dir(&root).is_ok() {
      fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).expect("unlock");
      fs::remove_dir_all(&root).expect("clean probe");
      return;
    }

    let error = add_at(&root, Origin::System, &[source.join("sample")], false, true)
      .expect_err("system add must fail");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).expect("unlock");
    assert!(error.contains("sudo dawn add"), "{error}");
    assert_eq!(error.matches("sudo dawn add").count(), 1, "{error}");
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
