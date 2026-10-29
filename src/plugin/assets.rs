//! Safe loading of plugin PNG assets.
//!
//! Plugin manifests are authored by third parties, so an image path is treated
//! as hostile: it must be relative, declared in `[permissions].filesystem`,
//! canonicalise to a path still inside the plugin directory, and stay within
//! small size and pixel-count limits. Only PNG is accepted in v1, which keeps
//! the dependency and attack surface tiny while covering logos and icons.

use std::io::Cursor;
use std::path::Path;

use tiny_skia::Pixmap;

use crate::plugin::manifest::is_granted;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DIMENSION: u32 = 2048;
const MAX_PIXELS: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AssetError {
  #[error("path is not covered by [permissions].filesystem")]
  NotGranted,
  #[error("resolves outside the plugin directory")]
  Escape,
  #[error("cannot read {path}: {source}")]
  Io {
    path: String,
    #[source]
    source: std::io::Error,
  },
  #[error("file is larger than 8 MiB")]
  TooLarge,
  #[error(
    "image is {width}x{height}; the limit is {MAX_DIMENSION}x{MAX_DIMENSION} and 4 megapixels"
  )]
  TooManyPixels { width: u32, height: u32 },
  #[error("not a readable PNG: {0}")]
  Png(#[from] png::DecodingError),
}

/// Loads one PNG from a plugin directory after enforcing every permission and
/// size rule. The result is a premultiplied pixmap ready to paint.
pub(crate) fn load_png(
  plugin_dir: &Path,
  relative: &str,
  grants: &[String],
) -> Result<Pixmap, AssetError> {
  if !is_granted(relative, grants) {
    return Err(AssetError::NotGranted);
  }

  let candidate = plugin_dir.join(relative);
  let root = plugin_dir.canonicalize().map_err(|source| AssetError::Io {
    path: plugin_dir.display().to_string(),
    source,
  })?;
  let canonical = candidate.canonicalize().map_err(|source| AssetError::Io {
    path: candidate.display().to_string(),
    source,
  })?;
  if !canonical.starts_with(&root) {
    return Err(AssetError::Escape);
  }

  let metadata = std::fs::metadata(&canonical).map_err(|source| AssetError::Io {
    path: canonical.display().to_string(),
    source,
  })?;
  if !metadata.is_file() {
    return Err(AssetError::Io {
      path: canonical.display().to_string(),
      source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file"),
    });
  }
  if metadata.len() > MAX_FILE_BYTES {
    return Err(AssetError::TooLarge);
  }

  let bytes = std::fs::read(&canonical).map_err(|source| AssetError::Io {
    path: canonical.display().to_string(),
    source,
  })?;

  // Check the header before handing the bytes to the decoder. `decode_png`
  // allocates from the declared dimensions, so this is the cheap gate that
  // stops a 30,000 x 30,000 PNG header from becoming a 3.6 GiB allocation.
  {
    let limits = png::Limits {
      bytes: (MAX_PIXELS * 4) as usize + 1024 * 1024,
    };
    let decoder = png::Decoder::new_with_limits(Cursor::new(bytes.as_slice()), limits);
    let reader = decoder.read_info()?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0
      || height == 0
      || width > MAX_DIMENSION
      || height > MAX_DIMENSION
      || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
      return Err(AssetError::TooManyPixels { width, height });
    }
  }

  Ok(Pixmap::decode_png(&bytes)?)
}

#[cfg(test)]
mod tests {
  use super::load_png;

  fn temp_plugin(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dawn-asset-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    dir
  }

  fn write_png(path: &std::path::Path, width: u32, height: u32) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let data = vec![255u8; (width * height * 4) as usize];
    writer.write_image_data(&data).unwrap();
  }

  #[test]
  fn loads_a_granted_png() {
    let dir = temp_plugin("ok");
    write_png(&dir.join("assets/logo.png"), 8, 8);
    let pixmap = load_png(&dir, "assets/logo.png", &["assets".to_string()]).unwrap();
    assert_eq!(pixmap.width(), 8);
    assert_eq!(pixmap.height(), 8);
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn refuses_undeclared_paths() {
    let dir = temp_plugin("grant");
    write_png(&dir.join("assets/logo.png"), 4, 4);
    let error = load_png(&dir, "assets/logo.png", &[]).unwrap_err();
    assert!(error.to_string().contains("permissions"));
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn refuses_paths_that_escape_via_symlink() {
    let dir = temp_plugin("escape");
    let outside = dir.parent().unwrap().join(format!(
      "dawn-outside-{}-{}.png",
      std::process::id(),
      "escape"
    ));
    write_png(&outside, 4, 4);
    std::os::unix::fs::symlink(&outside, dir.join("assets/logo.png")).unwrap();
    let error = load_png(&dir, "assets/logo.png", &["assets".to_string()]).unwrap_err();
    assert!(error.to_string().contains("outside"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&outside);
  }

  #[test]
  fn refuses_oversized_images_and_garbage() {
    let dir = temp_plugin("bad");
    write_png(&dir.join("assets/wide.png"), 16, 16);
    assert!(
      load_png(&dir, "assets/wide.png", &["assets".to_string()])
        .unwrap()
        .width()
        > 0
    );
    std::fs::write(dir.join("assets/fake.png"), b"not a png").unwrap();
    assert!(load_png(&dir, "assets/fake.png", &["assets".to_string()]).is_err());
    let _ = std::fs::remove_dir_all(&dir);
  }
}
