//! Resources validation.

use super::*;

impl<'a> Validator<'a> {
  pub(super) fn gpu(&mut self, raw: &RawGpu) -> GpuConfig {
    let defaults = GpuConfig::default();
    GpuConfig {
      enabled: self.boolean(raw.enabled.as_ref(), "gpu.enabled", defaults.enabled),
      backend: self.choice(
        raw.backend.as_ref(),
        "gpu.backend",
        defaults.backend,
        GpuBackend::parse,
        "auto, vulkan, gl, or software",
      ),
      adapter: self.gpu_adapter(raw.adapter.as_ref(), &defaults.adapter),
      device: self.choice(
        raw.device.as_ref(),
        "gpu.device",
        defaults.device,
        GpuDevice::parse,
        "auto, discrete, integrated, virtual, or cpu",
      ),
      power: self.choice(
        raw.power.as_ref(),
        "gpu.power",
        defaults.power,
        GpuPower::parse,
        "low, high, or default",
      ),
      present: self.choice(
        raw.present.as_ref(),
        "gpu.present",
        defaults.present,
        GpuPresent::parse,
        "fifo, mailbox, or immediate",
      ),
      fallback: self.choice(
        raw.fallback.as_ref(),
        "gpu.fallback",
        defaults.fallback,
        GpuFallback::parse,
        "chain, software, cpu, or none",
      ),
    }
  }

  pub(super) fn audio(
    &mut self,
    raw: &RawAudio,
    wallpaper_audio: bool,
    base: Option<&Path>,
  ) -> AudioConfig {
    let defaults = AudioConfig::default();
    let mut audio = AudioConfig {
      enabled: self.boolean(raw.enabled.as_ref(), "audio.enabled", defaults.enabled),
      volume: self.integer_range(
        raw.volume.as_ref(),
        "audio.volume",
        u64::from(defaults.volume),
        0,
        100,
      ) as u8,
      device: self.text(
        raw.device.as_ref(),
        "audio.device",
        &defaults.device,
        "expected an ALSA device name",
        128,
      ),
      max_voices: self.integer_range(
        raw.max_voices.as_ref(),
        "audio.max_voices",
        u64::from(defaults.max_voices),
        1,
        16,
      ) as u8,
      stop_on_login: self.boolean(
        raw.stop_on_login.as_ref(),
        "audio.stop_on_login",
        defaults.stop_on_login,
      ),
      ambient: None,
      wallpaper_audio: None,
      events: BTreeMap::new(),
    };

    if let Some(raw_ambient) = &raw.ambient
      && let Some(spec) = self.ambient(raw_ambient, wallpaper_audio, base)
    {
      audio.ambient = Some(spec);
    }

    if let Some(events) = &raw.events {
      for (name, raw_event) in events {
        let key = format!("audio.events.{name}");
        let Some(event) = Event::parse(name) else {
          self.issue(
            key,
            None,
            format!(
              "unknown event; use one of: {}",
              Event::ALL
                .iter()
                .map(|event| event.as_str())
                .collect::<Vec<_>>()
                .join(", ")
            ),
          );
          continue;
        };
        let Some(sound) = self.audio_event(raw_event, &key, base) else {
          continue;
        };
        audio.events.insert(event, sound);
      }
    }
    audio
  }

  pub(super) fn ambient(
    &mut self,
    raw: &RawAmbient,
    allow_missing_file: bool,
    base: Option<&Path>,
  ) -> Option<AmbientSpec> {
    let file = if raw.file.is_some() {
      self.audio_file(raw.file.as_ref(), "audio.ambient.file", base)?
    } else if allow_missing_file {
      // The wallpaper supplies the audio; this table only tunes the bed.
      PathBuf::new()
    } else {
      self.issue(
        "audio.ambient.file",
        raw.file.as_ref(),
        "an audio entry needs a file",
      );
      return None;
    };
    Some(AmbientSpec {
      file,
      volume_db: self.range(
        raw.volume_db.as_ref(),
        "audio.ambient.volume_db",
        -12.0,
        -60.0,
        0.0,
      ) as f32,
      fade_in: Duration::from_millis(self.integer_range(
        raw.fade_in_ms.as_ref(),
        "audio.ambient.fade_in_ms",
        600,
        0,
        10_000,
      )),
      duck_db: self.range(
        raw.duck_db.as_ref(),
        "audio.ambient.duck_db",
        -10.0,
        -40.0,
        0.0,
      ) as f32,
      duck_ms: Duration::from_millis(self.integer_range(
        raw.duck_ms.as_ref(),
        "audio.ambient.duck_ms",
        250,
        0,
        2000,
      )),
    })
  }

  pub(super) fn audio_event(
    &mut self,
    raw: &RawAudioEvent,
    key: &str,
    base: Option<&Path>,
  ) -> Option<EventSound> {
    let file = self.audio_file(raw.file.as_ref(), &format!("{key}.file"), base)?;
    let max_ms = self.integer_range(raw.max_ms.as_ref(), &format!("{key}.max_ms"), 0, 0, 5000);
    Some(EventSound {
      file,
      volume_db: self.range(
        raw.volume_db.as_ref(),
        &format!("{key}.volume_db"),
        -10.0,
        -60.0,
        0.0,
      ) as f32,
      retrigger: self.choice(
        raw.retrigger.as_ref(),
        &format!("{key}.retrigger"),
        Retrigger::Restart,
        Retrigger::parse,
        "restart, overlap, or ignore",
      ),
      cooldown: Duration::from_millis(self.integer_range(
        raw.cooldown_ms.as_ref(),
        &format!("{key}.cooldown_ms"),
        0,
        0,
        60_000,
      )),
      pitch_jitter: self.range(
        raw.pitch_jitter.as_ref(),
        &format!("{key}.pitch_jitter"),
        0.0,
        0.0,
        12.0,
      ) as f32,
      duck_db: self.range(
        raw.duck_db.as_ref(),
        &format!("{key}.duck_db"),
        -10.0,
        -40.0,
        0.0,
      ) as f32,
      max: (max_ms > 0).then(|| Duration::from_millis(max_ms)),
    })
  }

  /// Resolves an audio path and rejects formats the decoders cannot open, so
  /// the mistake shows up in `--check-config` instead of at boot.
  pub(super) fn audio_file(
    &mut self,
    field: Option<&Field>,
    key: &str,
    base: Option<&Path>,
  ) -> Option<PathBuf> {
    let value = self.text(
      field,
      key,
      "",
      "expected a path to an .ogg or .wav file",
      MAX_PATH_CHARS,
    );
    if value.is_empty() {
      self.issue(key, field, "an audio entry needs a file");
      return None;
    }
    let lower = value.to_ascii_lowercase();
    if !(lower.ends_with(".ogg") || lower.ends_with(".wav")) {
      self.issue(key, field, "expected an .ogg or .wav file");
      return None;
    }
    let path = match expand_path(&value, base) {
      Ok(path) => path,
      Err(message) => {
        self.issue(key, field, message);
        return None;
      }
    };
    match std::fs::metadata(&path) {
      Ok(metadata) if metadata.is_file() => Some(path),
      Ok(_) => {
        self.issue(
          key,
          field,
          format!("{} is not a regular file", path.display()),
        );
        None
      }
      Err(error) => {
        self.issue(
          key,
          field,
          format!("cannot read {}: {error}", path.display()),
        );
        None
      }
    }
  }

  pub(super) fn rituals(&mut self, raw: &RawRituals) -> RitualsConfig {
    let defaults = RitualsConfig::default();
    let weights = raw.weights.as_ref();
    let weights = DropWeights {
      common: self.integer_range(
        weights.and_then(|weights| weights.common.as_ref()),
        "rituals.weights.common",
        u64::from(defaults.weights.common),
        0,
        100,
      ) as u8,
      rare: self.integer_range(
        weights.and_then(|weights| weights.rare.as_ref()),
        "rituals.weights.rare",
        u64::from(defaults.weights.rare),
        0,
        100,
      ) as u8,
      saga: self.integer_range(
        weights.and_then(|weights| weights.saga.as_ref()),
        "rituals.weights.saga",
        u64::from(defaults.weights.saga),
        0,
        100,
      ) as u8,
    };
    if weights.is_empty() {
      self.issue(
        "rituals.weights",
        None,
        "at least one rarity weight must be above zero; using the defaults",
      );
    }
    RitualsConfig {
      enabled: self.boolean(raw.enabled.as_ref(), "rituals.enabled", defaults.enabled),
      state_path: self
        .optional_text(
          raw.state_path.as_ref(),
          "rituals.state_path",
          "expected a path",
          MAX_PATH_CHARS,
        )
        .map(PathBuf::from),
      achievements: self.boolean(
        raw.achievements.as_ref(),
        "rituals.achievements",
        defaults.achievements,
      ),
      drops: self.boolean(raw.drops.as_ref(), "rituals.drops", defaults.drops),
      seed: self.choice(
        raw.seed.as_ref(),
        "rituals.seed",
        defaults.seed,
        DropSeed::parse,
        "date or boot",
      ),
      weights: if weights.is_empty() {
        defaults.weights
      } else {
        weights
      },
    }
  }

  /// The shared shape of every enumerated choice key, not just `[gpu]`.  /// Keeping the issue key caller-provided is what makes a typo report
  /// `gpu.present` or `media.layout` rather than a generic table error.
  pub(super) fn choice<T: Copy>(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: T,
    parse: fn(&str) -> Option<T>,
    expected: &str,
  ) -> T {
    let Some(field) = field else {
      return default;
    };
    let Some(value) = field.get_ref().as_str() else {
      self.issue(key, Some(field), format!("expected {expected}"));
      return default;
    };
    match parse(value) {
      Some(choice) => choice,
      None => {
        self.issue(
          key,
          Some(field),
          format!("unknown value {value:?}; use {expected}"),
        );
        default
      }
    }
  }

  /// Adapter selection is intentionally looser than the other keys: a name may
  /// only exist on the machine running the greeter, so an unknown name is not a
  /// config error. Only the `index:N` syntax can be checked here.
  pub(super) fn gpu_adapter(&mut self, field: Option<&Field>, default: &str) -> String {
    let Some(field) = field else {
      return default.to_string();
    };
    let Some(value) = self.string_value(
      Some(field),
      "gpu.adapter",
      "expected auto, index:N, or an adapter name",
      64,
    ) else {
      return default.to_string();
    };
    if value.is_empty() || value.eq_ignore_ascii_case("auto") {
      return "auto".to_string();
    }
    if let Some(rest) = value.strip_prefix("index:") {
      let rest = rest.trim();
      return match rest.parse::<u32>() {
        Ok(index) => format!("index:{index}"),
        Err(_) => {
          self.issue(
            "gpu.adapter",
            Some(field),
            format!("expected index:N with a whole number, got {value:?}"),
          );
          "auto".to_string()
        }
      };
    }
    value
  }

  pub(super) fn rotation(&mut self, field: Option<&Field>, key: &str, default: f32) -> f32 {
    self.range(field, key, f64::from(default), -360.0, 360.0) as f32
  }

  pub(super) fn fade_pair(
    &mut self,
    field: Option<&Field>,
    key: &str,
    default: (f32, f32),
  ) -> (f32, f32) {
    let Some(field) = field else {
      return default;
    };
    let Some(values) = field.get_ref().as_array() else {
      self.issue(key, Some(field), "expected [start, end] fractions");
      return default;
    };
    if values.len() != 2 {
      self.issue(
        key,
        Some(field),
        "expected exactly two numbers, [start, end]",
      );
      return default;
    }
    let mut pair = [0.0f32; 2];
    for (index, value) in values.iter().enumerate() {
      let Some(number) = value
        .as_float()
        .or_else(|| value.as_integer().map(|number| number as f64))
      else {
        self.issue(key, Some(field), "fade fractions must be numbers");
        return default;
      };
      if !(0.0..=1.0).contains(&number) {
        self.issue(key, Some(field), "fade fractions must be between 0 and 1");
        return default;
      }
      pair[index] = number as f32;
    }
    if pair[0] > pair[1] {
      self.issue(key, Some(field), "fade start must not be after fade end");
      return default;
    }
    (pair[0], pair[1])
  }
}
