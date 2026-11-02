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
