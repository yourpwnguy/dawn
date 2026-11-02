//! Config discovery, bounded reads, TOML parsing, and theme loading.

use super::*;

/// Loads config, always returning a usable configuration.
#[must_use]
pub fn load(explicit: Option<&Path>) -> LoadedConfig {
  let path = explicit.map(Path::to_path_buf).or_else(discover);
  match path {
    Some(path) => load_path(&path, true),
    None => LoadedConfig {
      config: Config::default(),
      path: None,
      issues: Vec::new(),
      package: None,
    },
  }
}

/// Loads a complete config from a theme bundle.
///
/// A bundle cannot select another bundle. That keeps `dawn add` validation
/// finite and makes every installed theme self-contained.
#[must_use]
pub fn load_theme(path: &Path) -> LoadedConfig {
  // Keep the bundle's own audio table here. `run` still applies the machine
  // policy after selection, so a theme cannot change the host's mute, volume,
  // or device. Keeping the table intact means `dawn check` and the dev studio
  // see the sound design the bundle actually ships.
  load_path(path, false)
}

fn load_path(path: &Path, allow_package: bool) -> LoadedConfig {
  let source = match read_bounded(path) {
    Ok(source) => source,
    Err(message) => {
      return LoadedConfig {
        config: Config::default(),
        path: Some(path.to_path_buf()),
        issues: vec![Issue {
          key: "config".to_string(),
          location: None,
          message: format!("{}: {message}; using built-in defaults", path.display()),
        }],
        package: None,
      };
    }
  };

  let raw: RawConfig = match toml::from_str(&source) {
    Ok(raw) => raw,
    Err(error) => {
      let location = error.span().and_then(|span| {
        location_of(&source, span.start).map(|(line, column)| Location {
          file: path.to_path_buf(),
          line,
          column,
        })
      });
      return LoadedConfig {
        config: Config::default(),
        path: Some(path.to_path_buf()),
        issues: vec![Issue {
          key: "config".to_string(),
          location,
          message: format!("{}; using built-in defaults", error.message()),
        }],
        package: None,
      };
    }
  };

  let mut early = Vec::new();
  let mut package = None;
  if let Some(field) = &raw.theme.package {
    let location = field.span().start;
    let location = location_of(&source, location).map(|(line, column)| Location {
      file: path.to_path_buf(),
      line,
      column,
    });
    match field.get_ref() {
      toml::Value::String(id) if !id.trim().is_empty() => {
        if allow_package {
          package = Some(id.clone());
        } else {
          early.push(Issue {
            key: "theme.package".to_string(),
            location,
            message: "a theme bundle cannot select another theme".to_string(),
          });
        }
      }
      _ => early.push(Issue {
        key: "theme.package".to_string(),
        location,
        message: "expected a theme id string".to_string(),
      }),
    }
  }

  let base = path.parent().map(Path::to_path_buf);
  let mut validator = Validator::new(path, &source);
  let config = validator.build(raw, base.as_deref());
  let mut issues = early;
  issues.extend(validator.into_issues());

  LoadedConfig {
    config,
    path: Some(path.to_path_buf()),
    issues,
    package,
  }
}

fn discover() -> Option<PathBuf> {
  discover_from(std::env::var_os(ENV_CONFIG), Path::new(SYSTEM_CONFIG))
}

/// Two layers only, and no XDG/home search. `XDG_CONFIG_HOME` is deliberately
/// absent: a config placed there is invisible to the greeter user and having
/// two possible machine configs makes "why did it load that file?" impossible
/// to answer. Tests call this directly so they never have to mutate the process
/// environment.
pub(crate) fn discover_from(env: Option<OsString>, system: &Path) -> Option<PathBuf> {
  if let Some(path) = env.filter(|value| !value.is_empty()) {
    let candidate = PathBuf::from(path);
    if candidate.is_file() {
      return Some(candidate);
    }
  }
  system.is_file().then(|| system.to_path_buf())
}

/// Config is read once at boot. A cap keeps a mistaken `--config /dev/zero`
/// from turning into an out-of-memory at the worst possible moment.
fn read_bounded(path: &Path) -> Result<String, String> {
  const MAX_BYTES: u64 = 1024 * 1024;
  let metadata = std::fs::metadata(path).map_err(|error| format!("cannot read config: {error}"))?;
  if !metadata.is_file() {
    return Err("not a regular file".to_string());
  }
  if metadata.len() > MAX_BYTES {
    return Err(format!(
      "file is {} bytes, larger than the {MAX_BYTES} byte limit",
      metadata.len()
    ));
  }
  std::fs::read_to_string(path).map_err(|error| format!("cannot read config: {error}"))
}

/// Converts a byte offset into a 1-based line and column for error messages.
pub(crate) fn location_of(source: &str, offset: usize) -> Option<(usize, usize)> {
  let prefix = source.get(..offset).unwrap_or(source);
  let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
  let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
  let column = prefix
    .get(line_start..)
    .map(|text| text.chars().count() + 1)
    .unwrap_or(1);
  Some((line, column))
}
