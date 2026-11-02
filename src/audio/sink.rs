//! ALSA output.
//!
//! A greeter starts before the user's sound server, so this talks to ALSA
//! directly. The sink owns the PCM handle and is only ever touched from the
//! `dawn-audio` thread.
//!
//! Two details matter for a login screen. The device is asked for a small
//! period and buffer so the first key click is not half a second late, and we
//! read the rate the device actually accepted instead of assuming 48 kHz. The
//! mixer resamples its 48 kHz source to that rate, so a 44.1 kHz-only DAC still
//! plays at the right pitch.
//!
//! Missing hardware, a busy device, or a bad PCM name is a normal silent run.
//! Nothing here can delay the login form.

use crate::audio::decode::SAMPLE_RATE;

/// One blocking stereo PCM sink. `write` returns once the device accepted the
/// whole buffer, which is what paces the mixer thread.
pub trait Sink {
  /// The rate the device accepted. The mixer uses this to resample.
  fn sample_rate(&self) -> u32;

  /// Writes interleaved stereo i16 frames at [`Sink::sample_rate`].
  fn write(&mut self, samples: &[i16]) -> Result<(), String>;
}

pub struct AlsaSink {
  pcm: alsa::pcm::PCM,
  name: String,
  rate: u32,
}

impl AlsaSink {
  /// Opens the configured device, or the best usable ALSA output for `auto`.
  pub fn open(device: &str) -> Result<Self, String> {
    // alsa-lib writes some probe failures straight to stderr. Keep that noise
    // out of the journal and surface our own error string instead.
    let _quiet = alsa::Output::local_error_handler().ok();
    let explicit = !(device.eq_ignore_ascii_case("auto") || device.is_empty());
    if explicit {
      return open_named(device);
    }
    let mut last = String::new();
    for name in auto_candidates() {
      match open_named(&name) {
        Ok(sink) => {
          tracing::debug!("audio: auto selected ALSA device {}", sink.name());
          return Ok(sink);
        }
        Err(error) => last = error,
      }
    }
    Err(format!("no usable ALSA device: {last}"))
  }
}

/// Scores one ALSA PCM hint. `None` means the name is a helper or a loopback
/// card and should never be chosen automatically.
fn hint_rank(name: &str, description: &str) -> Option<i32> {
  let lower = name.to_ascii_lowercase();
  if [
    "null",
    "lavrate",
    "samplerate",
    "speex",
    "upmix",
    "vdownmix",
    "jack",
    "oss",
  ]
  .iter()
  .any(|skip| lower.starts_with(skip))
  {
    return None;
  }
  let desc = description.to_ascii_lowercase();
  if lower.contains("loopback") || desc.contains("loopback") {
    return None;
  }
  let mut score = 0;
  if desc.contains("analog") {
    score += 50;
  }
  if lower.starts_with("sysdefault") {
    score += 30;
  } else if lower.starts_with("front") {
    score += 25;
  } else if lower.starts_with("plughw") {
    score += 15;
  } else if lower.starts_with("hw:") {
    score += 10;
  } else if lower == "default" {
    score += 1;
  }
  if lower.starts_with("hdmi") || desc.contains("hdmi") {
    score -= 40;
  }
  Some(score)
}

/// Scores ALSA playback hints for a boot-time greeter.
///
/// The old fixed order (`default`, then `hw:0` to `hw:3`) is wrong on any
/// machine with a loopback card or HDMI first: it opens a device that exists
/// but makes no sound. Hints let us prefer analog outputs, then a plug layer,
/// then direct `hw`, and keep `default` as the last resort because it may be a
/// PipeWire or PulseAudio plugin that is not running for the greeter user.
fn auto_candidates() -> Vec<String> {
  use alsa::device_name::HintIter;
  use std::collections::BTreeSet;

  let mut scored = Vec::new();
  if let Ok(hints) = HintIter::new_str(None, "pcm") {
    for hint in hints {
      if matches!(hint.direction, Some(alsa::Direction::Capture)) {
        continue;
      }
      let Some(name) = hint.name else {
        continue;
      };
      let desc = hint.desc.unwrap_or_default();
      if let Some(score) = hint_rank(&name, &desc) {
        scored.push((score, name));
      }
    }
  }
  scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
  let mut seen = BTreeSet::new();
  let mut names: Vec<String> = scored
    .into_iter()
    .map(|(_, name)| name)
    .filter(|name| seen.insert(name.clone()))
    .collect();
  if !seen.contains("default") {
    names.push("default".to_string());
  }
  if names.is_empty() {
    names.extend(["default", "hw:0", "hw:1", "hw:2", "hw:3"].map(str::to_string));
  }
  names
}

fn open_named(name: &str) -> Result<AlsaSink, String> {
  use alsa::pcm::{Access, Format, HwParams, PCM};

  let pcm =
    PCM::new(name, alsa::Direction::Playback, false).map_err(|error| format!("{name}: {error}"))?;
  let rate = {
    let params = HwParams::any(&pcm).map_err(|error| format!("{name}: {error}"))?;
    params
      .set_channels(2)
      .map_err(|error| format!("{name}: {error}"))?;
    // Ask for 48 kHz and let the device pick the nearest rate it supports.
    // We do not force an exact rate: a 44.1 kHz-only interface is still a
    // perfectly good output, and the mixer can resample.
    params
      .set_rate_near(SAMPLE_RATE, alsa::ValueOr::Nearest)
      .map_err(|error| format!("{name}: {error}"))?;
    let rate = params
      .get_rate()
      .map_err(|error| format!("{name}: cannot read the sample rate: {error}"))?;
    if !(8_000..=192_000).contains(&rate) {
      return Err(format!(
        "{name}: device selected {rate} Hz, outside 8000 to 192000"
      ));
    }
    params
      .set_format(Format::s16())
      .map_err(|error| format!("{name}: {error}"))?;
    params
      .set_access(Access::RWInterleaved)
      .map_err(|error| format!("{name}: {error}"))?;
    // 10 ms periods and a 40 ms buffer. That is short enough for feedback but
    // long enough that an ordinary scheduler hiccup does not underrun.
    let period = i64::from(rate) / 100;
    let _ = params.set_period_size_near(period, alsa::ValueOr::Nearest);
    let _ = params.set_buffer_size_near((period * 4).max(period));
    pcm
      .hw_params(&params)
      .map_err(|error| format!("{name}: cannot apply hardware parameters: {error}"))?;
    rate
  };
  pcm
    .prepare()
    .map_err(|error| format!("{name}: cannot prepare: {error}"))?;
  Ok(AlsaSink {
    pcm,
    name: name.to_string(),
    rate,
  })
}

impl AlsaSink {
  /// The device name ALSA actually opened. With `auto` this may be a fallback.
  #[must_use]
  pub fn name(&self) -> &str {
    &self.name
  }
}

impl Sink for AlsaSink {
  fn sample_rate(&self) -> u32 {
    self.rate
  }

  fn write(&mut self, samples: &[i16]) -> Result<(), String> {
    if samples.is_empty() {
      return Ok(());
    }
    let io = self
      .pcm
      .io_i16()
      .map_err(|error| format!("cannot open the PCM stream: {error}"))?;
    let mut offset = 0;
    let mut recoveries = 0;
    // `writei` is allowed to accept fewer frames than offered. The old code
    // ignored that count, which silently dropped the rest of the chunk and
    // produced clicks. Loop until everything is queued.
    while offset < samples.len() {
      match io.writei(&samples[offset..]) {
        Ok(0) => return Err("the audio device accepted zero frames".to_string()),
        Ok(frames) => offset += frames * 2,
        Err(_) if self.pcm.state() == alsa::pcm::State::XRun && recoveries < 2 => {
          // An underrun happens when the greeter was busy elsewhere. Recover
          // and retry, but do not spin forever on a device that keeps failing.
          recoveries += 1;
          self
            .pcm
            .prepare()
            .map_err(|error| format!("cannot recover from an underrun: {error}"))?;
        }
        Err(error) => return Err(format!("cannot write audio: {error}")),
      }
    }
    Ok(())
  }
}

/// A sink that accepts everything. Used by tests that exercise the mixer
/// without touching a sound card.
#[cfg(test)]
pub struct NullSink;

#[cfg(test)]
impl Sink for NullSink {
  fn sample_rate(&self) -> u32 {
    SAMPLE_RATE
  }

  fn write(&mut self, _samples: &[i16]) -> Result<(), String> {
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::hint_rank;

  #[test]
  fn analog_beats_hdmi_and_loopback_is_never_auto() {
    assert!(
      hint_rank("sysdefault:CARD=Generic_1", "ALC256 Analog")
        > hint_rank("hdmi:CARD=NVidia,DEV=0", "HDMI 0")
    );
    assert_eq!(hint_rank("hw:CARD=Loopback,DEV=0", "Loopback PCM"), None);
    assert!(
      hint_rank("default", "Default") < hint_rank("front:CARD=Generic_1,DEV=0", "Analog front")
    );
  }
}
