//! Resources section of the raw config schema.

use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawGpu {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub backend: Option<Field>,
  #[serde(default)]
  pub adapter: Option<Field>,
  #[serde(default)]
  pub device: Option<Field>,
  #[serde(default)]
  pub power: Option<Field>,
  #[serde(default)]
  pub present: Option<Field>,
  #[serde(default)]
  pub fallback: Option<Field>,
}

/// The `[audio]` table: host policy and theme sound content share it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAudio {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub volume: Option<Field>,
  #[serde(default)]
  pub device: Option<Field>,
  #[serde(default)]
  pub max_voices: Option<Field>,
  #[serde(default)]
  pub stop_on_login: Option<Field>,
  #[serde(default)]
  pub ambient: Option<RawAmbient>,
  #[serde(default)]
  pub events: Option<BTreeMap<String, RawAudioEvent>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAmbient {
  #[serde(default)]
  pub file: Option<Field>,
  #[serde(default)]
  pub volume_db: Option<Field>,
  #[serde(default)]
  pub fade_in_ms: Option<Field>,
  #[serde(default)]
  pub duck_db: Option<Field>,
  #[serde(default)]
  pub duck_ms: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAudioEvent {
  #[serde(default)]
  pub file: Option<Field>,
  #[serde(default)]
  pub volume_db: Option<Field>,
  #[serde(default)]
  pub retrigger: Option<Field>,
  #[serde(default)]
  pub cooldown_ms: Option<Field>,
  #[serde(default)]
  pub pitch_jitter: Option<Field>,
  #[serde(default)]
  pub duck_db: Option<Field>,
  #[serde(default)]
  pub max_ms: Option<Field>,
}

/// The opt-in rituals layer.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRituals {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub state_path: Option<Field>,
  #[serde(default)]
  pub achievements: Option<Field>,
  #[serde(default)]
  pub drops: Option<Field>,
  #[serde(default)]
  pub seed: Option<Field>,
  #[serde(default)]
  pub weights: Option<RawRitualWeights>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRitualWeights {
  #[serde(default)]
  pub common: Option<Field>,
  #[serde(default)]
  pub rare: Option<Field>,
  #[serde(default)]
  pub saga: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSessions {
  #[serde(default)]
  pub dirs: Option<Field>,
  #[serde(default)]
  pub default: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPlugins {
  #[serde(default)]
  pub enabled: Option<Field>,
  #[serde(default)]
  pub dirs: Option<Field>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAuth {
  #[serde(default)]
  pub socket: Option<Field>,
  #[serde(default)]
  pub timeout_secs: Option<Field>,
}
