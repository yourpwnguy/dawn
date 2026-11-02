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
