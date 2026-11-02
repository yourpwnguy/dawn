//! Wallpaper configuration and per-output overrides.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundKind {
  /// Detect from the file: a multi-frame GIF/WebP is animated, anything else
  /// is a still image. No image means the colour or gradient.
  #[default]
  Auto,
  Solid,
  Gradient,
  Image,
  Animated,
}

impl BackgroundKind {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "solid" => Some(Self::Solid),
      "gradient" => Some(Self::Gradient),
      "image" => Some(Self::Image),
      "animated" | "video" => Some(Self::Animated),
      _ => None,
    }
  }
}

#[derive(Debug, Clone)]
pub struct BackgroundConfig {
  pub kind: BackgroundKind,
  pub color: Rgb,
  /// A very subtle vertical gradient derived from `color`. Set to false for a
  /// perfectly flat background.
  pub gradient: bool,
  pub image: Option<PathBuf>,
  /// Play the video wallpaper's audio track as the ambient bed. Host
  /// `[audio].enabled` still decides whether any sound plays at all.
  pub audio: bool,
  pub fit: Fit,
  /// Upper bound on animated playback. The file's own delays are respected
  /// when they are slower than this.
  pub fps: u32,
  /// Hard budget for decoded wallpaper frames.
  pub memory_limit: u64,
  /// Freeze animation after this many seconds without input. Zero disables it.
  pub idle_pause: Duration,
  /// Per-output overrides keyed by output name, for example `DP-1`.
  pub monitors: BTreeMap<String, BackgroundOverride>,
}

impl BackgroundConfig {
  /// The resolved base colour for one output. This is the cheap lookup the
  /// renderer uses every frame; `for_monitor` clones the whole config and is
  /// only needed when starting a load.
  #[must_use]
  pub fn color_for_monitor(&self, monitor: Option<&str>) -> Rgb {
    monitor
      .and_then(|name| self.monitors.get(name))
      .and_then(|over| over.color)
      .unwrap_or(self.color)
  }

  /// Base config with the named output's overrides applied. Unknown outputs
  /// simply keep the base values.
  #[must_use]
  pub fn for_monitor(&self, monitor: Option<&str>) -> Self {
    let mut resolved = self.clone();
    let Some(monitor) = monitor else {
      return resolved;
    };
    let Some(over) = self.monitors.get(monitor) else {
      return resolved;
    };
    if let Some(kind) = over.kind {
      resolved.kind = kind;
    }
    if let Some(color) = over.color {
      resolved.color = color;
    }
    if let Some(image) = &over.image {
      resolved.image = Some(image.clone());
    }
    if let Some(fit) = over.fit {
      resolved.fit = fit;
    }
    resolved
  }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BackgroundOverride {
  pub kind: Option<BackgroundKind>,
  pub color: Option<Rgb>,
  pub image: Option<PathBuf>,
  pub fit: Option<Fit>,
}
