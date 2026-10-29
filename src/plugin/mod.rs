//! The plugin host for v1 declarative plugins.
//!
//! A plugin is a directory under one of `plugins.dirs` with a `plugin.toml`
//! and optional image assets. The host reads and validates all of it at
//! startup, then hands the UI a list of inert widgets. There is no code
//! execution, no event hook, and no shared buffer, so the credential-isolation
//! rule holds by construction rather than by convention.
//!
//! The clock and media widgets are first-party plugins built from config. They
//! use the same [`Widget`] enum, the same renderer, and the same placement
//! rules as third-party widgets. Media is intentionally not exposed as a
//! manifest kind yet: it needs a live D-Bus state and exposing a dead copy
//! would be a worse API than waiting for a real v2 manifest contract.
//!
//! Failure policy: a broken plugin is logged and left out. It must never stop
//! the greeter, and it must never sit between the user and the login form, so
//! widgets are painted *under* all core UI as well.

mod assets;
mod manifest;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tiny_skia::Pixmap;
use tracing::{debug, info, warn};

use crate::color::Rgb;
use crate::config::{Config, PluginsConfig, ThemeConfig};
use crate::layout::Placement;
use crate::ui::clock::ClockSpec;
use crate::ui::media::MediaState;
use manifest::{Manifest, ParsedWidget};

pub use crate::layout::Anchor;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// One drawable widget supplied by a plugin or by the built-in set.
#[derive(Clone)]
pub enum Widget {
  Text {
    text: String,
    size: f32,
    color: Rgb,
    placement: Placement,
    max_width: Option<f32>,
  },
  Clock {
    spec: ClockSpec,
    placement: Placement,
    draggable: bool,
  },
  Image {
    image: Pixmap,
    width: f32,
    placement: Placement,
  },
  Media {
    state: Arc<MediaState>,
    placement: Placement,
    size: f32,
    show_art: bool,
    color: Option<Rgb>,
    show_before_login: bool,
    draggable: bool,
  },
}

impl Widget {
  /// Stable kind name for `--check-config`. Keep these in sync with the
  /// manifest `kind` values.
  #[must_use]
  pub fn kind(&self) -> &'static str {
    match self {
      Self::Text { .. } => "text",
      Self::Clock { .. } => "clock",
      Self::Image { .. } => "image",
      Self::Media { .. } => "media",
    }
  }

  #[must_use]
  pub fn placement(&self) -> &Placement {
    match self {
      Self::Text { placement, .. }
      | Self::Clock { placement, .. }
      | Self::Image { placement, .. }
      | Self::Media { placement, .. } => placement,
    }
  }

  /// A string that changes when the widget's visible content changes. The app
  /// uses it to avoid repainting a minute clock every second.
  #[must_use]
  pub fn signature(&self, now: &jiff::Zoned) -> String {
    match self {
      Self::Clock { spec, .. } => spec.signature(now),
      Self::Media { state, .. } => state.generation().to_string(),
      Self::Text { .. } | Self::Image { .. } => String::new(),
    }
  }

  #[must_use]
  pub fn media_state(&self) -> Option<&Arc<MediaState>> {
    match self {
      Self::Media { state, .. } => Some(state),
      _ => None,
    }
  }

  /// Whether the pointer may move this widget. Plugin text and images are not
  /// draggable yet; the built-in clock and media widget are.
  #[must_use]
  pub fn draggable(&self) -> bool {
    match self {
      Self::Clock { draggable, .. } | Self::Media { draggable, .. } => *draggable,
      Self::Text { .. } | Self::Image { .. } => false,
    }
  }
}

/// A loaded plugin, ready for the renderer.
#[derive(Clone)]
pub struct Plugin {
  pub id: String,
  pub name: String,
  pub version: String,
  pub description: String,
  pub widgets: Vec<Widget>,
  /// True for the clock/media widgets built from the main config.
  pub builtin: bool,
}

impl Plugin {
  #[must_use]
  pub fn media_states(&self) -> Vec<Arc<MediaState>> {
    self
      .widgets
      .iter()
      .filter_map(Widget::media_state)
      .cloned()
      .collect()
  }
}

/// The first-party widgets. They are appended to the third-party plugin list
/// by [`crate::app::App::new`], so every frontend renders the same set.
pub struct Builtins {
  pub plugins: Vec<Plugin>,
  pub media: Vec<Arc<MediaState>>,
}

/// Builds the clock and media widgets from config. Both are optional and
/// either can be disabled without affecting the other.
#[must_use]
pub fn builtins(config: &Config) -> Builtins {
  let mut widgets = Vec::new();
  let mut media = Vec::new();
  if config.clock.enabled {
    widgets.push(Widget::Clock {
      spec: ClockSpec::from_config(&config.clock),
      placement: config.clock.placement.clone(),
      draggable: config.clock.draggable,
    });
  }
  if config.media.enabled {
    let state = Arc::new(MediaState::with_keep_last(config.media.keep_last));
    media.push(Arc::clone(&state));
    widgets.push(Widget::Media {
      state,
      placement: config.media.placement.clone(),
      size: config.media.size,
      show_art: config.media.show_art,
      color: config.media.color,
      show_before_login: config.media.show_before_login,
      draggable: config.media.draggable,
    });
  }
  if widgets.is_empty() {
    return Builtins {
      plugins: Vec::new(),
      media,
    };
  }
  Builtins {
    plugins: vec![Plugin {
      id: "dawn".to_string(),
      name: "Dawn built-in widgets".to_string(),
      version: env!("CARGO_PKG_VERSION").to_string(),
      description: "Clock and media widgets configured in the main config.".to_string(),
      widgets,
      builtin: true,
    }],
    media,
  }
}

/// What startup found. Errors are also logged; the extra copy exists so
/// `dawn --check-config` can print a clean report without parsing the log.
pub struct Report {
  pub plugins: Vec<Plugin>,
  pub errors: Vec<String>,
}

/// Loads every plugin directory. Never returns an error: plugins are optional.
#[must_use]
pub fn load_all(config: &PluginsConfig, theme: &ThemeConfig) -> Report {
  let mut report = Report {
    plugins: Vec::new(),
    errors: Vec::new(),
  };
  if !config.enabled {
    debug!("plugins are disabled in config");
    return report;
  }

  let mut seen = std::collections::HashSet::new();
  for dir in &config.dirs {
    let entries = match std::fs::read_dir(dir) {
      Ok(entries) => entries,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
        debug!(path = %dir.display(), "plugin directory does not exist");
        continue;
      }
      Err(error) => {
        let message = format!("{}: cannot read plugin directory: {error}", dir.display());
        warn!("{message}");
        report.errors.push(message);
        continue;
      }
    };

    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    // read_dir order is filesystem order; sorting keeps layering deterministic.
    paths.sort();

    for path in paths {
      if !path.is_dir() {
        continue;
      }
      let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
        continue;
      };
      if id.starts_with('.') || !valid_id(id) {
        debug!(path = %path.display(), "ignoring plugin directory with an invalid id");
        continue;
      }
      if !seen.insert(id.to_string()) {
        let message = format!("{id}: duplicated plugin id, the earlier directory wins");
        warn!("{message}");
        report.errors.push(message);
        continue;
      }

      match load_one(id, &path, theme) {
        Ok(Some(plugin)) => {
          debug!(id, name = %plugin.name, widgets = plugin.widgets.len(), "loaded plugin");
          report.plugins.push(plugin);
        }
        Ok(None) => {}
        Err(message) => {
          let message = format!("{id}: {message}");
          warn!("{message}");
          report.errors.push(message);
        }
      }
    }
  }

  report.plugins.sort_by(|left, right| left.id.cmp(&right.id));
  info!(
    count = report.plugins.len(),
    errors = report.errors.len(),
    "loaded plugins"
  );
  report
}

/// `Ok(None)` means "not a plugin", which is quieter than one error per random
/// directory an administrator happened to place next to their plugins.
fn load_one(id: &str, dir: &Path, theme: &ThemeConfig) -> Result<Option<Plugin>, String> {
  let manifest_path = dir.join("plugin.toml");
  let metadata = match std::fs::metadata(&manifest_path) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      debug!(path = %dir.display(), "directory has no plugin.toml");
      return Ok(None);
    }
    Err(error) => return Err(format!("cannot read plugin.toml: {error}")),
  };
  if !metadata.is_file() {
    return Err("plugin.toml is not a regular file".to_string());
  }
  if metadata.len() > MAX_MANIFEST_BYTES {
    return Err(format!(
      "plugin.toml is larger than the {} KiB limit",
      MAX_MANIFEST_BYTES / 1024
    ));
  }
  let text = std::fs::read_to_string(&manifest_path)
    .map_err(|error| format!("cannot read plugin.toml: {error}"))?;

  let manifest: Manifest = manifest::parse(&text, theme.muted)?;
  let mut widgets = Vec::with_capacity(manifest.widgets.len());
  for widget in manifest.widgets {
    match resolve_widget(dir, widget, &manifest.grants) {
      Ok(widget) => widgets.push(widget),
      Err(message) => warn!(plugin = id, %message, "skipping plugin widget"),
    }
  }
  if widgets.is_empty() {
    debug!(id, "plugin has no usable widgets");
    return Ok(None);
  }

  Ok(Some(Plugin {
    id: id.to_string(),
    name: manifest.name,
    version: manifest.version,
    description: manifest.description,
    widgets,
    builtin: false,
  }))
}

fn resolve_widget(dir: &Path, widget: ParsedWidget, grants: &[String]) -> Result<Widget, String> {
  match widget {
    ParsedWidget::Text {
      text,
      size,
      color,
      anchor,
      offset,
      monitor,
      rotation,
      max_width,
    } => Ok(Widget::Text {
      text,
      size,
      color,
      placement: Placement {
        anchor,
        offset,
        monitor,
        rotation,
      },
      max_width,
    }),
    ParsedWidget::Clock {
      spec,
      anchor,
      offset,
      monitor,
      rotation,
    } => Ok(Widget::Clock {
      spec,
      placement: Placement {
        anchor,
        offset,
        monitor,
        rotation,
      },
      draggable: false,
    }),
    ParsedWidget::Media {
      size,
      show_art,
      show_before_login,
      color,
      anchor,
      offset,
      monitor,
      rotation,
    } => Ok(Widget::Media {
      state: Arc::new(MediaState::new()),
      placement: Placement {
        anchor,
        offset,
        monitor,
        rotation,
      },
      size,
      show_art,
      color: Some(color),
      show_before_login,
      draggable: false,
    }),
    ParsedWidget::Image {
      path,
      width,
      anchor,
      offset,
      monitor,
      rotation,
    } => {
      let image =
        assets::load_png(dir, &path, grants).map_err(|error| format!("{path}: {error}"))?;
      Ok(Widget::Image {
        image,
        width,
        placement: Placement {
          anchor,
          offset,
          monitor,
          rotation,
        },
      })
    }
  }
}

fn valid_id(id: &str) -> bool {
  !id.is_empty()
    && id.len() <= 64
    && id
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

#[cfg(test)]
mod tests {
  use super::load_all;
  use crate::config::{Config, PluginsConfig};
  use std::path::PathBuf;

  fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dawn-plugin-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn theme() -> crate::config::ThemeConfig {
    Config::default().theme
  }

  #[test]
  fn loads_a_text_plugin_and_ignores_non_plugins() {
    let root = temp_root("text");
    let plugin = root.join("hello");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
      plugin.join("plugin.toml"),
      "api_version = 1\nname = \"Hello\"\n[[widget]]\nkind = \"text\"\ntext = \"Hi\"\nrotation = 15\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("not-a-plugin")).unwrap();

    let report = load_all(
      &PluginsConfig {
        enabled: true,
        dirs: vec![root.clone()],
      },
      &theme(),
    );
    assert_eq!(report.plugins.len(), 1);
    assert_eq!(report.plugins[0].name, "Hello");
    assert_eq!(report.plugins[0].widgets[0].placement().rotation, 15.0);
    assert!(!report.plugins[0].builtin);
    assert!(report.errors.is_empty());
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn a_broken_plugin_is_reported_but_does_not_stop_others() {
    let root = temp_root("broken");
    let bad = root.join("bad");
    let good = root.join("good");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::create_dir_all(&good).unwrap();
    std::fs::write(
      bad.join("plugin.toml"),
      "api_version = 99\nname = \"Bad\"\n",
    )
    .unwrap();
    std::fs::write(
      good.join("plugin.toml"),
      "api_version = 1\nname = \"Good\"\n[[widget]]\nkind = \"clock\"\n",
    )
    .unwrap();

    let report = load_all(
      &PluginsConfig {
        enabled: true,
        dirs: vec![root.clone()],
      },
      &theme(),
    );
    assert_eq!(report.plugins.len(), 1);
    assert_eq!(report.plugins[0].id, "good");
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].contains("bad"));
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn disabled_plugins_are_not_read() {
    let report = load_all(
      &PluginsConfig {
        enabled: false,
        dirs: vec![PathBuf::from("/nonexistent")],
      },
      &theme(),
    );
    assert!(report.plugins.is_empty());
    assert!(report.errors.is_empty());
  }

  #[test]
  fn builtins_include_clock_and_media() {
    let built = super::builtins(&Config::default());
    assert_eq!(built.plugins.len(), 1);
    assert!(built.plugins[0].builtin);
    assert_eq!(built.plugins[0].widgets.len(), 2);
    assert_eq!(built.media.len(), 1);
  }
}
