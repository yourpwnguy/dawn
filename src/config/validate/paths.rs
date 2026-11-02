//! Paths validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn path(
    &mut self,
    field: Option<&Field>,
    key: &str,
    base: Option<&Path>,
  ) -> Option<PathBuf> {
    let raw = self.optional_text(field, key, "expected a file path", MAX_PATH_CHARS)?;
    match expand_path(&raw, base) {
      Ok(path) => Some(path),
      Err(message) => {
        self.issue(key, field, message);
        None
      }
    }
  }

  pub(super) fn timezone(&mut self, field: Option<&Field>) -> Option<String> {
    let name = self.optional_text(
      field,
      "clock.timezone",
      "expected an IANA time zone such as \"Europe/Berlin\"",
      64,
    )?;
    match jiff::tz::TimeZone::get(&name) {
      Ok(_) => Some(name),
      Err(error) => {
        self.issue(
          "clock.timezone",
          field,
          format!("cannot load time zone {name:?}: {error}"),
        );
        None
      }
    }
  }

  pub(super) fn strftime(
    &mut self,
    field: Option<&Field>,
    key: &'static str,
    default: &str,
    allow_empty: bool,
  ) -> String {
    let Some(value) = self.string_value(field, key, "expected an strftime format string", 64)
    else {
      return default.to_string();
    };
    if value.is_empty() {
      if allow_empty {
        return value;
      }
      self.issue(key, field, "must not be empty");
      return default.to_string();
    }
    if let Err(error) = jiff::fmt::strtime::format(&value, &jiff::Zoned::now()) {
      self.issue(key, field, format!("not a valid strftime format: {error}"));
      return default.to_string();
    }
    value
  }

  pub(super) fn image(
    &mut self,
    field: Option<&Field>,
    key: &str,
    base: Option<&Path>,
  ) -> Option<PathBuf> {
    let raw = self.optional_text(
      field,
      key,
      "expected a path to a PNG, JPEG, GIF, WebP, or MP4 file",
      MAX_PATH_CHARS,
    )?;
    let path = match expand_path(&raw, base) {
      Ok(path) => path,
      Err(message) => {
        self.issue(key, field, message);
        return None;
      }
    };
    // Video files are allowed to be much larger than still images. Keep the
    // still limit strict, because a 1 GiB PNG is almost always a mistake, but
    // a few minutes of 1080p MP4 is normal.
    let video = has_video_extension(&path);
    let limit = if video {
      8 * 1024 * 1024 * 1024
    } else {
      MAX_IMAGE_BYTES
    };
    match std::fs::metadata(&path) {
      Ok(metadata) if !metadata.is_file() => {
        self.issue(
          key,
          field,
          format!(
            "{} is not a regular file (fix: point {key} at an image or video file, or remove it)",
            path.display()
          ),
        );
        None
      }
      Ok(metadata) if metadata.len() > limit => {
        let what = if video { "video" } else { "image" };
        self.issue(
          key,
          field,
          format!(
            "{} is larger than the {} MiB {what} limit (fix: downscale or shorten it)",
            path.display(),
            limit / 1024 / 1024
          ),
        );
        None
      }
      Ok(_) => Some(path),
      Err(error) => {
        self.issue(
          key,
          field,
          format!(
            "cannot read {}: {error} (fix: correct {key} or make the file readable)",
            path.display()
          ),
        );
        None
      }
    }
  }

  pub(super) fn background_monitors(
    &mut self,
    raw: &BTreeMap<String, RawBackgroundOverride>,
    base: Option<&Path>,
  ) -> BTreeMap<String, BackgroundOverride> {
    if raw.len() > 16 {
      self.issue(
        "background.monitors",
        None,
        "at most 16 per-monitor overrides are supported",
      );
    }
    let mut monitors = BTreeMap::new();
    for (name, over) in raw.iter().take(16) {
      if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        self.issue(
          "background.monitors",
          None,
          format!("ignoring output {name:?}: names must be 1-64 printable characters"),
        );
        continue;
      }
      let key = format!("background.monitors.{name}");
      let kind = over.kind.as_ref().map(|field| {
        self.background_kind(Some(field), BackgroundKind::Auto, &format!("{key}.kind"))
      });
      let color = self.optional_color(over.color.as_ref(), &format!("{key}.color"));
      let image = self.image(over.image.as_ref(), &format!("{key}.image"), base);
      let fit = over
        .fit
        .as_ref()
        .map(|field| self.fit(Some(field), Fit::Cover, &format!("{key}.fit")));
      monitors.insert(
        name.clone(),
        BackgroundOverride {
          kind,
          color,
          image,
          fit,
        },
      );
    }
    monitors
  }

  pub(super) fn dirs(
    &mut self,
    field: Option<&Field>,
    key: &'static str,
    defaults: &[PathBuf],
    base: Option<&Path>,
    require_one: bool,
  ) -> Vec<PathBuf> {
    let Some(field) = field else {
      return defaults.to_vec();
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(key, Some(field), "expected a list of directory paths");
      return defaults.to_vec();
    };
    if values.len() > 8 {
      self.issue(key, Some(field), "at most 8 directories are supported");
      return defaults.to_vec();
    }

    let mut resolved = Vec::with_capacity(values.len());
    for value in values {
      let Some(value) = value.as_str() else {
        self.issue(key, Some(field), "every entry must be a string");
        return defaults.to_vec();
      };
      match expand_path(value, base) {
        Ok(path) => resolved.push(path),
        Err(message) => {
          self.issue(key, Some(field), message);
          return defaults.to_vec();
        }
      }
    }

    if resolved.is_empty() && require_one {
      self.issue(key, Some(field), "must contain at least one directory");
      return defaults.to_vec();
    }
    resolved
  }
}

/// The issue key for a placement field. Keeping these in one place means a
/// typo in one section does not get reported under another section's name.
pub(super) fn placement_key(scope: &str, index: usize) -> &'static str {
  match (scope, index) {
    ("clock", 0) => "clock.anchor",
    ("clock", 1) => "clock.offset",
    ("clock", 2) => "clock.monitor",
    ("clock", _) => "clock.rotation",
    ("media", 0) => "media.anchor",
    ("media", 1) => "media.offset",
    ("media", 2) => "media.monitor",
    ("media", _) => "media.rotation",
    ("login", 0) => "login.anchor",
    ("login", 1) => "login.offset",
    ("login", 2) => "login.monitor",
    _ => "login.rotation",
  }
}

/// Video container extensions. Kept here instead of calling into the UI so the
/// config layer stays independent of the renderer.
pub(super) fn has_video_extension(path: &Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "mpg" | "mpeg"
      )
    })
}

/// Expands `~` and resolves relative paths against the config file's directory.
/// The path is not required to exist: desktop and plugin directories may be
/// created later, and that should be a quiet no-op rather than a config error.
pub(super) fn expand_path(raw: &str, base: Option<&Path>) -> Result<PathBuf, String> {
  let raw = raw.trim();
  if raw.is_empty() {
    return Err("path must not be empty".to_string());
  }
  if raw.chars().count() > MAX_PATH_CHARS {
    return Err(format!("path must be at most {MAX_PATH_CHARS} characters"));
  }
  if raw.chars().any(char::is_control) {
    return Err("path must not contain control characters".to_string());
  }
  if let Some(rest) = raw.strip_prefix("~/") {
    let home = std::env::var_os("HOME")
      .filter(|value| !value.is_empty())
      .ok_or_else(|| "cannot expand '~': HOME is not set".to_string())?;
    return Ok(PathBuf::from(home).join(rest));
  }
  if raw == "~" {
    return std::env::var_os("HOME")
      .filter(|value| !value.is_empty())
      .map(PathBuf::from)
      .ok_or_else(|| "cannot expand '~': HOME is not set".to_string());
  }
  let path = PathBuf::from(raw);
  if path.is_relative()
    && let Some(base) = base
  {
    return Ok(base.join(path));
  }
  Ok(path)
}
