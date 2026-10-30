//! Atomic writes for the small text files the CLI owns.
//!
//! Both `dawn init` and `dawn use` replace a config file that a greeter may be
//! reading at the same moment. We write a staging file in the destination
//! directory, flush it, set its mode, then rename it over the target. A reader
//! therefore sees either the old file or the new one, never a half-written
//! file.
//!
//! The mode argument exists because `tempfile` creates files as 0600. A config
//! read by the unprivileged `greeter` account has to be world-readable, so the
//! caller passes 0o644 here instead of fixing it up afterwards.

use std::fs;
use std::io::Write;
use std::path::Path;

use tempfile::Builder;

/// Replaces `path` with `contents`, applying `mode` before the rename.
pub fn write_atomic(path: &Path, contents: &str, mode: u32) -> Result<(), String> {
  let parent = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));
  fs::create_dir_all(parent)
    .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;

  let mut staging = Builder::new()
    .prefix(".dawn-write-")
    .tempfile_in(parent)
    .map_err(|error| {
      format!(
        "cannot create a staging file in {}: {error}",
        parent.display()
      )
    })?;

  staging
    .write_all(contents.as_bytes())
    .map_err(|error| format!("cannot write {}: {error}", staging.path().display()))?;
  // Flush and sync before the rename so a crash cannot leave an empty file
  // under the final name while the bytes are still in the page cache.
  staging
    .as_file()
    .sync_all()
    .map_err(|error| format!("cannot flush {}: {error}", staging.path().display()))?;
  set_mode(staging.path(), mode)?;

  staging.persist(path).map_err(|error| {
    format!(
      "cannot move the file into place at {}: {}",
      path.display(),
      error.error
    )
  })?;
  Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
      .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))?;
  }
  #[cfg(not(unix))]
  {
    let _ = (path, mode);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::write_atomic;

  fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
      "dawn-fsutil-{name}-{}-{}",
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
  fn replaces_the_file_and_keeps_the_requested_mode() {
    let dir = temp_dir("mode");
    let path = dir.join("config.toml");
    std::fs::write(&path, "old\n").expect("seed");

    write_atomic(&path, "new\n", 0o644).expect("write");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "new\n");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
      assert_eq!(mode, 0o644);
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
  }

  #[test]
  fn a_failed_rename_leaves_the_destination_alone() {
    let dir = temp_dir("atomic");
    let path = dir.join("config.toml");
    std::fs::create_dir(&path).expect("destination is a directory");

    assert!(write_atomic(&path, "replacement\n", 0o644).is_err());
    assert!(path.is_dir());
    assert!(
      std::fs::read_dir(&dir)
        .expect("read dir")
        .all(|entry| !entry
          .expect("entry")
          .file_name()
          .to_string_lossy()
          .starts_with(".dawn-write-")),
      "staging file was left behind"
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
  }
}
