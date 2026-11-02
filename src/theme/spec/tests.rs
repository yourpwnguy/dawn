use super::{PREVIEW_FILE, check_tree};
use crate::theme::scaffold;

fn scaffold(name: &str) -> std::path::PathBuf {
  let parent = std::env::temp_dir().join(format!(
    "dawn-spec-{name}-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|duration| duration.as_nanos())
      .unwrap_or_default()
  ));
  std::fs::create_dir_all(&parent).expect("create parent");
  scaffold::create(&parent, "probe", "Probe").expect("scaffold")
}

fn rules(check: &super::TreeCheck) -> Vec<u8> {
  check.issues.iter().map(|issue| issue.rule).collect()
}

#[test]
fn scaffold_passes_with_zero_issues() {
  let dir = scaffold("scaffold");
  let check = check_tree(&dir);
  assert!(check.ok(), "{:?}", check.issues);
  assert_eq!(check.revision, Some(2));
  assert!(check.warnings.is_empty(), "{:?}", check.warnings);
  let preview = std::fs::metadata(dir.join(PREVIEW_FILE)).expect("preview");
  assert_ne!(preview.len(), 0, "preview must not be empty");
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn generated_preview_is_a_real_webp() {
  let dir = scaffold("preview");
  let path = dir.join(PREVIEW_FILE);
  let dimensions = image::image_dimensions(&path).expect("decode preview header");
  assert_eq!(dimensions, (640, 400));
  assert!(super::decode_webp(&path).is_ok());
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn directory_id_mismatch_is_rule_one() {
  let dir = scaffold("mismatch");
  let renamed = dir.with_file_name("other");
  std::fs::rename(&dir, &renamed).expect("rename");
  let check = check_tree(&renamed);
  assert_eq!(rules(&check), vec![1]);
  assert!(check.issues[0].file == "theme.toml");
  std::fs::remove_dir_all(renamed.parent().expect("parent")).expect("cleanup");
}

#[test]
fn missing_assets_directory_is_rule_one() {
  let dir = scaffold("no-assets");
  std::fs::remove_dir_all(dir.join("assets")).expect("remove assets");
  let check = check_tree(&dir);
  assert_eq!(rules(&check), vec![1]);
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn oversized_preview_is_rule_one() {
  let dir = scaffold("big-preview");
  let pixels = vec![0u8; 2000 * 1200 * 3];
  let bytes = crate::theme::scaffold::encode_webp(&pixels, 2000, 1200).expect("encode");
  std::fs::write(dir.join(PREVIEW_FILE), bytes).expect("write preview");
  let check = check_tree(&dir);
  assert_eq!(rules(&check), vec![1]);
  assert!(
    check.issues[0].message.contains("exceeds"),
    "{:?}",
    check.issues
  );
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn escaping_reference_is_rule_three_and_root_reference_is_rule_two() {
  let dir = scaffold("escape");
  std::fs::write(
    dir.join("config.toml"),
    "[background]\nkind = \"image\"\nimage = \"../outside.png\"\n",
  )
  .expect("config");
  let check = check_tree(&dir);
  assert!(rules(&check).contains(&3), "{:?}", check.issues);

  std::fs::write(
    dir.join("config.toml"),
    "[background]\nkind = \"image\"\nimage = \"wallpaper.mp4\"\n",
  )
  .expect("config");
  let check = check_tree(&dir);
  // Rule 4 also fires because the file does not exist. The layout complaint
  // is the one that tells the author what to do.
  assert!(rules(&check).contains(&2), "{:?}", check.issues);
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rule_three() {
  let dir = scaffold("symlink");
  std::os::unix::fs::symlink("/etc/passwd", dir.join("assets/evil")).expect("symlink");
  let check = check_tree(&dir);
  assert!(rules(&check).contains(&3), "{:?}", check.issues);
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn unknown_revision_is_rule_five() {
  let dir = scaffold("revision");
  std::fs::write(
    dir.join("theme.toml"),
    "[spec]\nrevision = 9\n[bundle]\nid = \"probe\"\nname = \"Probe\"\n",
  )
  .expect("manifest");
  let check = check_tree(&dir);
  assert_eq!(rules(&check), vec![5]);
  assert!(
    check.issues[0]
      .message
      .contains("unsupported spec revision 9")
  );
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn zero_authors_is_rule_seven() {
  let dir = scaffold("authors");
  let manifest = std::fs::read_to_string(dir.join("theme.toml")).expect("manifest");
  let manifest = manifest
    .split("[[author]]")
    .next()
    .expect("author block")
    .to_string();
  std::fs::write(dir.join("theme.toml"), manifest).expect("manifest");
  let check = check_tree(&dir);
  assert_eq!(rules(&check), vec![7]);
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}

#[test]
fn revision_one_passes_with_one_warning() {
  let dir = scaffold("legacy");
  std::fs::write(
    dir.join("theme.toml"),
    "schema = 1\nid = \"probe\"\nname = \"Probe\"\n",
  )
  .expect("manifest");
  // The scaffold preview is still valid, but DSB-1 did not require assets/.
  std::fs::remove_dir_all(dir.join("assets")).expect("remove assets");
  let check = check_tree(&dir);
  assert!(check.ok(), "{:?}", check.issues);
  assert_eq!(check.revision, Some(1));
  assert_eq!(check.warnings.len(), 1);
  assert!(
    check.warnings[0].contains("--migrate"),
    "{:?}",
    check.warnings
  );
  std::fs::remove_dir_all(dir.parent().expect("parent")).expect("cleanup");
}
