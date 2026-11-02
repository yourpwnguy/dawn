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
