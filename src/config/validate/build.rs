//! Build validation.

use super::*;

impl<'a> Validator<'a> {
  pub(crate) fn build(&mut self, raw: RawConfig, base: Option<&Path>) -> Config {
    let preset = self.preset(raw.theme.preset.as_ref());
    let tokens = preset.tokens();
    let defaults = Config::default();

    let ui = self.ui(&raw, &defaults);
    let login = self.login(&raw, &ui, &tokens, &defaults, base);
    let particles = self.particles(&raw.particles);
    let ticker = self.ticker(&raw.ticker);

    let power = PowerConfig {
      low_power: self.boolean(
        raw.power.low_power.as_ref(),
        "power.low_power",
        defaults.power.low_power,
      ),
    };

    let sessions = SessionsConfig {
      dirs: self.dirs(
        raw.sessions.dirs.as_ref(),
        "sessions.dirs",
        &defaults.sessions.dirs,
        base,
        true,
      ),
      default: self.optional_text(
        raw.sessions.default.as_ref(),
        "sessions.default",
        "expected a .desktop id",
        128,
      ),
    };

    let plugins = PluginsConfig {
      enabled: self.boolean(
        raw.plugins.enabled.as_ref(),
        "plugins.enabled",
        defaults.plugins.enabled,
      ),
      dirs: self.dirs(
        raw.plugins.dirs.as_ref(),
        "plugins.dirs",
        &defaults.plugins.dirs,
        base,
        false,
      ),
    };

    let auth = AuthConfig {
      socket: self
        .optional_text(
          raw.auth.socket.as_ref(),
          "auth.socket",
          "expected a socket path",
          MAX_PATH_CHARS,
        )
        .map(PathBuf::from),
      timeout: Duration::from_secs(self.integer_range(
        raw.auth.timeout_secs.as_ref(),
        "auth.timeout_secs",
        defaults.auth.timeout.as_secs(),
        5,
        600,
      )),
    };

    let background = self.background(&raw, preset, &defaults, base);
    let gpu = self.gpu(&raw.gpu);
    let mut audio = self.audio(&raw.audio, background.audio, base);
    if background.audio {
      match (&background.image, background.kind) {
        (Some(image), BackgroundKind::Auto | BackgroundKind::Animated) => {
          audio.wallpaper_audio = Some(image.clone());
        }
        _ => self.issue(
          "background.audio",
          raw.background.audio.as_ref(),
          "background.audio needs a video file in background.image",
        ),
      }
    }
    let rituals = self.rituals(&raw.rituals);

    let layout = self.layout(&raw, &defaults, base);
    let clock = self.clock(&raw, &defaults);
    let media = self.media(&raw, &defaults);
    let calendar = self.calendar(&raw, &defaults);
    let battery = self.battery(&raw, &defaults);
    let system = self.system(&raw, &defaults);
    let theme = self.theme(&raw, &tokens);
    let motion = self.motion(&raw, &defaults);

    Config {
      ui,
      login,
      clock,
      media,
      calendar,
      battery,
      system,
      particles,
      ticker,
      background,
      theme,
      motion,
      power,
      gpu,
      audio,
      rituals,
      layout,
      sessions,
      plugins,
      auth,
    }
  }
}
