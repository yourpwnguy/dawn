//! `dawn new`: create a minimal, valid DSB-2 bundle.
//!
//! The command exists so nobody has to reverse-engineer the spec from an
//! example. It writes the four required artifacts into a sibling staging
//! directory and renames the directory into place, so an interrupted run never
//! leaves a half-bundle under the id the user asked for.
//!
//! The generated preview is a real WebP, drawn as a gradient and encoded with
//! the `image` crate's lossless encoder. That crate is already a dependency for
//! wallpapers, so this adds no new codec stack. The previous wave's tests
//! consume the file with the same decoder the validator uses, which keeps the
//! generator honest.

use std::path::{Path, PathBuf};

use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};
use tempfile::Builder;

use super::manifest::valid_spec_id;
use super::spec::PREVIEW_FILE;

const PREVIEW_WIDTH: u32 = 640;
const PREVIEW_HEIGHT: u32 = 400;

/// Turns a display name into a bundle id: lowercase ASCII letters and digits,
/// with every other run of characters collapsed to a single dash.
///
/// Returns `None` when nothing usable is left, so `dawn new "..."` can tell the
/// user to pick a name with at least one letter or digit.
#[must_use]
pub fn slug(name: &str) -> Option<String> {
  let mut id = String::new();
  for character in name.chars() {
    if character.is_ascii_alphanumeric() {
      id.push(character.to_ascii_lowercase());
    } else if !id.is_empty() && !id.ends_with('-') {
      id.push('-');
    }
  }
  let id = id.trim_end_matches('-').to_string();
  if valid_spec_id(&id) { Some(id) } else { None }
}

/// Creates `<parent>/<id>/` with all four required artifacts.
///
/// The parent directory is created when missing. The target must not already
/// exist: silently merging into an existing bundle would make `dawn new` a
/// destructive command. `dawn add --migrate` is the way to upgrade a tree.
pub fn create(parent: &Path, id: &str, name: &str) -> Result<PathBuf, String> {
  if !valid_spec_id(id) {
    return Err(format!("invalid bundle id {id:?}: use [a-z0-9][a-z0-9_-]*"));
  }
  let name = name.trim();
  if name.is_empty() {
    return Err("give the theme a name with at least one printable character".to_string());
  }
  if name.chars().count() > 80 {
    return Err("the theme name is longer than 80 characters".to_string());
  }
  if name.chars().any(char::is_control) {
    return Err("the theme name contains control characters".to_string());
  }
  std::fs::create_dir_all(parent)
    .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
  let target = parent.join(id);
  if std::fs::symlink_metadata(&target).is_ok() {
    return Err(format!("{} already exists", target.display()));
  }

  let staging = Builder::new()
    .prefix(&format!(".dawn-new-{id}-"))
    .tempdir_in(parent)
    .map_err(|error| {
      format!(
        "cannot create a staging directory in {}: {error}",
        parent.display()
      )
    })?;
  let root = staging.path();
  std::fs::create_dir(root.join("assets"))
    .map_err(|error| format!("cannot create {}: {error}", root.join("assets").display()))?;
  std::fs::write(root.join("theme.toml"), manifest(id, name))
    .map_err(|error| format!("cannot write theme.toml: {error}"))?;
  std::fs::write(root.join("config.toml"), CONFIG)
    .map_err(|error| format!("cannot write config.toml: {error}"))?;
  let preview = preview_webp()?;
  std::fs::write(root.join(PREVIEW_FILE), preview)
    .map_err(|error| format!("cannot write {PREVIEW_FILE}: {error}"))?;
  // An empty directory does not survive some copy and packaging workflows, so
  // the scaffold ships a placeholder. Rule 2 ignores files under assets/ that
  // the config does not reference, which makes this legal.
  std::fs::write(root.join("assets/.gitkeep"), "")
    .map_err(|error| format!("cannot write assets/.gitkeep: {error}"))?;

  let staging = staging.keep();
  match std::fs::rename(&staging, &target) {
    Ok(()) => Ok(target),
    Err(error) => {
      let _ = std::fs::remove_dir_all(&staging);
      Err(format!("cannot move the bundle into place: {error}"))
    }
  }
}

/// The three lines `dawn new` prints. Kept as a pure function so the wording is
/// testable without capturing stdout.
#[must_use]
pub fn next_steps(path: &Path, id: &str) -> [String; 3] {
  [
    format!("next: dawn check {}", super::shell_quote(path)),
    format!("next: dawn add {} --force", super::shell_quote(path)),
    format!("next: dawn use {id}"),
  ]
}

fn manifest(id: &str, name: &str) -> String {
  // `toml` serialises the name as a proper basic string, so quotes and
  // backslashes in a display name cannot break the manifest.
  let name = toml::Value::String(name.to_string()).to_string();
  format!(
    r#"# DSB-2 bundle manifest. The normative text is docs/bundle-spec.md.
# Fill in your name and role below before publishing. An empty author list is
# rejected by rule 7.
[spec]
revision = 2

[bundle]
id = "{id}"
name = {name}
version = "0.1.0"
description = "A new dawn theme."
preview = "preview.webp"

[bundle.engine]
min = "0.5.0"

[[author]]
name = "Your Name" # TODO: replace with your name
role = "author" # TODO: what you made, for example layout, palette, artwork
"#
  )
}

/// A minimal bundle config. It is deliberately built around a gradient so a
/// freshly scaffolded bundle has no asset references and `dawn check` can pass
/// before the author adds a single file.
const CONFIG: &str = r##"# dawn bundle config (DSB-2). This is a complete run config: when the bundle is
# selected it replaces the machine config, apart from the GPU selection.
# Every key is optional; these are the values a new theme starts from.
[ui]
font = "Inter"
font_size = 17

[background]
# A gradient needs no files. Switch to kind = "image" and point image at a file
# under assets/ when you add a wallpaper.
kind = "gradient"
color = "#0b1020"
gradient = true
fit = "cover"

[theme]
preset = "glass-dark"

[layout]
# Theme bundles are read-only, so the greeter must never write here.
save = false

[plugins]
enabled = false
"##;

fn preview_webp() -> Result<Vec<u8>, String> {
  // A diagonal blend from the template's deep navy to a cool highlight. It is
  // intentionally simple: the preview only has to communicate the palette.
  let start = [11u8, 16, 32];
  let end = [122u8, 162, 247];
  let span = f32::from(u16::try_from(PREVIEW_WIDTH + PREVIEW_HEIGHT).unwrap_or(u16::MAX));
  let mut pixels = Vec::with_capacity((PREVIEW_WIDTH * PREVIEW_HEIGHT * 3) as usize);
  for y in 0..PREVIEW_HEIGHT {
    for x in 0..PREVIEW_WIDTH {
      let t = (x + y) as f32 / span;
      for channel in 0..3 {
        let a = f32::from(start[channel]);
        let b = f32::from(end[channel]);
        pixels.push((a + (b - a) * t).round() as u8);
      }
    }
  }
  encode_webp(&pixels, PREVIEW_WIDTH, PREVIEW_HEIGHT)
}

/// The gradient used by `dawn new` and by migration when a legacy tree had no
/// preview. Exposed at crate scope so both paths produce the same asset.
pub(crate) fn generated_preview() -> Result<Vec<u8>, String> {
  preview_webp()
}

/// Encodes RGB8 pixels as lossless WebP. Shared with the migration path, which
/// has to invent a preview when a legacy tree never had one.
pub(crate) fn encode_webp(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
  let mut output = Vec::new();
  WebPEncoder::new_lossless(&mut output)
    .write_image(pixels, width, height, ExtendedColorType::Rgb8)
    .map_err(|error| format!("cannot encode the preview as WebP: {error}"))?;
  Ok(output)
}

#[cfg(test)]
mod tests {
  use super::{CONFIG, create, next_steps, slug};
  use crate::theme::manifest::valid_semver;

  fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-new-{name}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
  }

  #[test]
  fn scaffold_has_all_four_artifacts() {
    let parent = temp_dir("layout");
    let dir = create(&parent, "probe", "Probe").expect("create");
    assert!(dir.join("theme.toml").is_file());
    assert!(dir.join("config.toml").is_file());
    assert!(dir.join("preview.webp").is_file());
    assert!(dir.join("assets").is_dir());
    assert_eq!(
      dir.file_name().and_then(|name| name.to_str()),
      Some("probe")
    );
    std::fs::remove_dir_all(&parent).expect("cleanup");
  }

  #[test]
  fn config_is_minimal_and_valid_toml() {
    let value: toml::Value = toml::from_str(CONFIG).expect("config parses");
    assert_eq!(
      value
        .get("background")
        .and_then(|background| background.get("kind"))
        .and_then(toml::Value::as_str),
      Some("gradient")
    );
  }

  #[test]
  fn manifest_uses_a_semver_engine_minimum() {
    assert!(valid_semver(env!("CARGO_PKG_VERSION")));
  }

  #[test]
  fn three_next_steps_are_check_add_use() {
    let steps = next_steps(std::path::Path::new("/tmp/probe"), "probe");
    assert_eq!(steps.len(), 3);
    assert!(steps[0].contains("dawn check"), "{steps:?}");
    assert!(steps[1].contains("dawn add"), "{steps:?}");
    assert!(!steps[1].contains("sudo"), "{steps:?}");
    assert!(steps[2].contains("dawn use probe"), "{steps:?}");
  }

  #[test]
  fn the_display_name_goes_into_the_manifest() {
    let parent = temp_dir("name");
    let dir = create(&parent, "crimson-stage", "Crimson Stage").expect("create");
    let manifest = std::fs::read_to_string(dir.join("theme.toml")).expect("read manifest");
    let value: toml::Value = toml::from_str(&manifest).expect("manifest parses");
    let bundle = value.get("bundle").expect("bundle table");
    assert_eq!(
      bundle.get("name").and_then(toml::Value::as_str),
      Some("Crimson Stage")
    );
    assert_eq!(
      bundle.get("id").and_then(toml::Value::as_str),
      Some("crimson-stage")
    );
    std::fs::remove_dir_all(&parent).expect("cleanup");
  }

  #[test]
  fn slug_collapses_punctuation_and_case() {
    assert_eq!(slug("Crimson Stage").as_deref(), Some("crimson-stage"));
    assert_eq!(slug("  Hu Tao!! ").as_deref(), Some("hu-tao"));
    assert_eq!(slug("Midnight_2").as_deref(), Some("midnight-2"));
    assert_eq!(slug("!!!"), None);
    assert_eq!(slug(""), None);
  }

  #[test]
  fn leading_dot_and_uppercase_ids_are_rejected() {
    let parent = temp_dir("ids");
    assert!(create(&parent, "../escape", "Escape").is_err());
    assert!(create(&parent, "Upper", "Upper").is_err());
    std::fs::remove_dir_all(&parent).expect("cleanup");
  }
}
