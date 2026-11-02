//! Loader for the wallpaper engine.

use super::*;

impl LoadedBackground {
  /// Decodes the configured wallpaper for one output. The returned frames are
  /// already the size of the output, so painting is a single blit.
  pub fn load(
    config: &BackgroundConfig,
    size: (u32, u32),
    monitor: Option<&str>,
  ) -> Result<Self, String> {
    let config = config.for_monitor(monitor);
    match config.kind {
      BackgroundKind::Solid => Ok(Self {
        content: Content::Solid(config.color),
        warning: None,
      }),
      BackgroundKind::Gradient => Ok(Self {
        content: Content::Gradient(config.color),
        warning: None,
      }),
      BackgroundKind::Auto if config.image.is_none() => Ok(Self {
        content: if config.gradient {
          Content::Gradient(config.color)
        } else {
          Content::Solid(config.color)
        },
        warning: None,
      }),
      BackgroundKind::Auto | BackgroundKind::Image | BackgroundKind::Animated => {
        let Some(path) = config.image.as_deref() else {
          return Err(
            "background.image is empty; set a PNG/JPEG/GIF/WebP/MP4 path or remove background.kind"
              .to_string(),
          );
        };
        let video = video::is_video_path(path);
        let format = if video {
          None
        } else {
          image::animation_format(path)?
        };
        let wants_animation = config.kind == BackgroundKind::Animated
          || (config.kind == BackgroundKind::Auto && (format.is_some() || video));

        if wants_animation {
          if video {
            let stream = VideoStream::load(
              path,
              size,
              config.fit,
              config.color,
              config.fps,
              config.memory_limit,
              LOAD_LIMITS,
            )?;
            return Ok(Self {
              content: Content::Stream(Arc::new(stream)),
              warning: None,
            });
          }
          if let Some(format) = format {
            let stream = FrameStream::load(
              path,
              format,
              size,
              config.fit,
              config.color,
              config.fps,
              config.memory_limit,
              LOAD_LIMITS,
            )?;
            return Ok(Self {
              content: Content::Stream(Arc::new(stream)),
              warning: None,
            });
          }
          // kind = "animated" on a still file is a config mistake, not a
          // reason to fail. Show the still and say what happened.
          let frame = image::load_static(path, LOAD_LIMITS, size, config.fit, config.color)?;
          return Ok(Self {
            content: Content::Frames {
              frames: vec![Arc::new(frame)],
              delays: Vec::new(),
              cumulative: Vec::new(),
            },
            warning: Some(
              "background.kind = \"animated\" but the file has one frame; showing it still"
                .to_string(),
            ),
          });
        }

        if video {
          let frame = VideoStream::first_frame(
            path,
            size,
            config.fit,
            config.color,
            config.fps,
            config.memory_limit,
          )?;
          return Ok(Self {
            content: Content::Frames {
              frames: vec![Arc::new(frame)],
              delays: Vec::new(),
              cumulative: Vec::new(),
            },
            warning: Some(
              "background.kind = \"image\" shows only the first frame of the video".to_string(),
            ),
          });
        }

        let frame = image::load_static(path, LOAD_LIMITS, size, config.fit, config.color)?;
        let warning = (config.kind == BackgroundKind::Image && format.is_some())
          .then(|| "background.kind = \"image\" shows only the first frame".to_string());
        Ok(Self {
          content: Content::Frames {
            frames: vec![Arc::new(frame)],
            delays: Vec::new(),
            cumulative: Vec::new(),
          },
          warning,
        })
      }
    }
  }

  #[must_use]
  pub fn is_animated(&self) -> bool {
    match &self.content {
      Content::Frames { frames, .. } => frames.len() > 1,
      Content::Stream(_) => true,
      _ => false,
    }
  }

  #[must_use]
  pub fn frame_count(&self) -> usize {
    match &self.content {
      Content::Frames { frames, .. } => frames.len(),
      Content::Stream(_) => 1,
      _ => 1,
    }
  }

  pub(super) fn frame_at(&self, elapsed: Duration) -> (Option<&Arc<Pixmap>>, usize) {
    match &self.content {
      Content::Frames {
        frames, cumulative, ..
      } => {
        if frames.len() <= 1 {
          return (frames.first(), 0);
        }
        let total = cumulative.last().copied().unwrap_or_default();
        if total.is_zero() {
          return (frames.first(), 0);
        }
        let position = wrap(elapsed, total);
        let index = cumulative
          .partition_point(|end| *end <= position)
          .min(frames.len() - 1);
        (frames.get(index), index)
      }
      _ => (None, 0),
    }
  }
}

/// A request owns everything the loader thread needs. It is `Send`, which is
/// the point: no `Pixmap`, config, or window handle crosses the wire mid-load.
#[derive(Debug)]
pub struct BackgroundRequest {
  pub generation: u64,
  pub(super) config: BackgroundConfig,
  pub(super) size: (u32, u32),
  pub(super) monitor: Option<String>,
}

impl BackgroundRequest {
  /// Runs on the loader thread.
  pub fn load(self) -> (u64, Result<LoadedBackground, String>) {
    let result = LoadedBackground::load(&self.config, self.size, self.monitor.as_deref());
    (self.generation, result)
  }
}
