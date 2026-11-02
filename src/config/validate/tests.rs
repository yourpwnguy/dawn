use super::expand_path;

#[test]
fn relative_paths_use_the_config_directory() {
  let base = std::path::Path::new("/etc/dawn");
  assert_eq!(
    expand_path("wallpapers/a.png", Some(base)).unwrap(),
    std::path::PathBuf::from("/etc/dawn/wallpapers/a.png")
  );
}

#[test]
fn tilde_uses_home_when_present() {
  // HOME is set in the test environment; skip gracefully if it is not.
  if std::env::var_os("HOME").is_some() {
    let path = expand_path("~/x", None).unwrap();
    assert!(path.is_absolute());
    assert!(path.ends_with("x"));
  }
}

#[test]
fn empty_paths_are_rejected() {
  assert!(expand_path("   ", None).is_err());
}
