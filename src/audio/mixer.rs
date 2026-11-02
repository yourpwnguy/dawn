//! The mixer.
//!
//! One thread owns this struct and the output sink. It mixes a fixed 10 ms
//! chunk of the ambient bed plus the active one-shot voices, applies gain,
//! ducking, and the master volume, and blocks on the device write. Nothing
//! here is shared with the UI thread, and nothing here can wait on auth.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::decode::SAMPLE_RATE;
use super::sink::Sink;
use super::{AmbientSpec, Event, EventSound, Retrigger, db_to_gain};

/// Frames per device write at 48 kHz. Tests use this as the known chunk size;
/// the mixer itself derives its chunk from the sink's actual rate.
#[cfg(test)]
pub const CHUNK_FRAMES: usize = 480;

/// Key clicks accepted in any one second window.
const MAX_KEY_CLICKS_PER_SECOND: usize = 12;

pub struct LoadedSound {
  pub data: Arc<Vec<f32>>,
  pub frames: usize,
  pub spec: EventSound,
}

pub struct LoadedAmbient {
  pub data: Arc<Vec<f32>>,
  pub frames: usize,
  pub spec: AmbientSpec,
}

struct Voice {
  event: Event,
  data: Arc<Vec<f32>>,
  frames: usize,
  max_frames: usize,
  position: f64,
  step: f64,
  gain: f32,
  duck_gain: f32,
  duck_until: Instant,
  finished: bool,
}

struct Ambient {
  data: Arc<Vec<f32>>,
  frames: usize,
  spec: AmbientSpec,
  position: f64,
  gain: f32,
  duck_gain: f32,
  started: Instant,
}

pub struct Mixer {
  sink: Box<dyn Sink>,
  master: f32,
  max_voices: usize,
  sounds: HashMap<Event, LoadedSound>,
  ambient: Option<Ambient>,
  voices: Vec<Voice>,
  last_play: HashMap<Event, Instant>,
  key_plays: Vec<Instant>,
  rng: u64,
  /// Frames per device write at the device rate. Ten milliseconds is short
  /// enough for responsive feedback and long enough to avoid per-sample calls.
  chunk_frames: usize,
  /// How many 48 kHz source frames one output frame advances. The decoder
  /// always produces 48 kHz; a 44.1 kHz DAC needs a slower step, not a pitch
  /// shift.
  source_step: f64,
  buffer: Vec<f32>,
  output: Vec<i16>,
}

impl Mixer {
  pub fn new(
    sink: Box<dyn Sink>,
    master: f32,
    max_voices: usize,
    sounds: HashMap<Event, LoadedSound>,
    ambient: Option<LoadedAmbient>,
  ) -> Self {
    let now = Instant::now();
    let output_rate = sink.sample_rate().clamp(8_000, 192_000);
    let chunk_frames = (u64::from(output_rate) / 100).max(1) as usize;
    Self {
      sink,
      master,
      max_voices: max_voices.clamp(1, 16),
      sounds,
      ambient: ambient.map(|ambient| Ambient {
        data: ambient.data,
        frames: ambient.frames.max(1),
        spec: ambient.spec,
        position: 0.0,
        gain: 0.0,
        duck_gain: 1.0,
        started: now,
      }),
      voices: Vec::new(),
      last_play: HashMap::new(),
      key_plays: Vec::new(),
      rng: seed(),
      chunk_frames,
      source_step: f64::from(SAMPLE_RATE) / f64::from(output_rate),
      buffer: vec![0.0; chunk_frames * 2],
      output: Vec::with_capacity(chunk_frames * 2),
    }
  }

  /// Queues one event. Returns false when the event is unknown, cooling down,
  /// over the key-click budget, or the voice budget is full.
  pub fn play(&mut self, event: Event, now: Instant) -> bool {
    let Some(sound) = self.sounds.get(&event) else {
      return false;
    };

    if let Some(last) = self.last_play.get(&event)
      && now.saturating_duration_since(*last) < sound.spec.cooldown
    {
      return false;
    }

    if event == Event::Key {
      self
        .key_plays
        .retain(|played| now.saturating_duration_since(*played) < Duration::from_secs(1));
      if self.key_plays.len() >= MAX_KEY_CLICKS_PER_SECOND {
        return false;
      }
    }

    match sound.spec.retrigger {
      Retrigger::Ignore if self.voices.iter().any(|voice| voice.event == event) => return false,
      Retrigger::Restart => self.voices.retain(|voice| voice.event != event),
      Retrigger::Overlap | Retrigger::Ignore => {}
    }

    if self.voices.len() >= self.max_voices {
      return false;
    }

    let jitter = if sound.spec.pitch_jitter > 0.0 {
      let span = f64::from(sound.spec.pitch_jitter) / 2.0;
      (next_random(&mut self.rng) * 2.0 - 1.0) * span
    } else {
      0.0
    };
    let max_frames = match sound.spec.max {
      Some(limit) if !limit.is_zero() => {
        ((limit.as_secs_f64() * f64::from(SAMPLE_RATE)) as usize).min(sound.frames)
      }
      _ => sound.frames,
    };

    self.voices.push(Voice {
      event,
      data: Arc::clone(&sound.data),
      frames: sound.frames,
      max_frames,
      position: 0.0,
      step: 2f64.powf(jitter / 12.0) * self.source_step,
      gain: db_to_gain(sound.spec.volume_db),
      duck_gain: db_to_gain(sound.spec.duck_db),
      duck_until: now + Duration::from_millis(u64::from(self.duck_ms())),
      finished: false,
    });
    self.last_play.insert(event, now);
    if event == Event::Key {
      self.key_plays.push(now);
    }
    true
  }

  fn duck_ms(&self) -> u32 {
    self
      .ambient
      .as_ref()
      .map_or(0, |ambient| ambient.spec.duck_ms.as_millis() as u32)
  }

  /// Mixes one chunk and writes it to the sink.
  pub fn write_chunk(&mut self) -> Result<(), String> {
    let now = Instant::now();
    self.buffer.fill(0.0);
    let duck = self.active_duck(now);
    let frames = self.chunk_frames;

    if let Some(ambient) = &mut self.ambient {
      // The fade curve is already smooth, so apply it directly. Only the duck
      // factor is eased, because a stinger starts and ends on a hard edge and
      // a jump there would click.
      let fade = if ambient.spec.fade_in.is_zero() {
        1.0
      } else {
        (now.saturating_duration_since(ambient.started).as_secs_f32()
          / ambient.spec.fade_in.as_secs_f32())
        .min(1.0)
      };
      ambient.duck_gain += (duck - ambient.duck_gain) * 0.25;
      ambient.gain = db_to_gain(ambient.spec.volume_db) * fade * ambient.duck_gain;
      let mut position = ambient.position;
      for frame in 0..frames {
        let first = position.floor() as usize % ambient.frames;
        let second = (first + 1) % ambient.frames;
        let fraction = (position - position.floor()) as f32;
        self.buffer[frame * 2] += (ambient.data[first * 2] * (1.0 - fraction)
          + ambient.data[second * 2] * fraction)
          * ambient.gain;
        self.buffer[frame * 2 + 1] += (ambient.data[first * 2 + 1] * (1.0 - fraction)
          + ambient.data[second * 2 + 1] * fraction)
          * ambient.gain;
        position += self.source_step;
      }
      ambient.position = position % ambient.frames as f64;
    }

    for voice in &mut self.voices {
      for frame in 0..frames {
        let position = voice.position;
        if position as usize >= voice.max_frames || position as usize >= voice.frames {
          voice.finished = true;
          break;
        }
        let first = position.floor() as usize;
        let second = (first + 1).min(voice.frames - 1);
        let fraction = (position - first as f64) as f32;
        let left = voice.data[first * 2] * (1.0 - fraction) + voice.data[second * 2] * fraction;
        let right =
          voice.data[first * 2 + 1] * (1.0 - fraction) + voice.data[second * 2 + 1] * fraction;
        self.buffer[frame * 2] += left * voice.gain;
        self.buffer[frame * 2 + 1] += right * voice.gain;
        voice.position += voice.step;
      }
    }
    self.voices.retain(|voice| !voice.finished);

    self.output.clear();
    self.output.extend(
      self
        .buffer
        .iter()
        .map(|sample| ((sample * self.master).clamp(-1.0, 1.0) * 32_767.0) as i16),
    );
    self.sink.write(&self.output)
  }

  /// The deepest duck currently held by a stinger, or 1.0 when none is active.
  fn active_duck(&self, now: Instant) -> f32 {
    self
      .voices
      .iter()
      .filter(|voice| voice.duck_gain < 1.0 && now < voice.duck_until)
      .map(|voice| voice.duck_gain)
      .fold(1.0, f32::min)
  }

  /// Testing hook: how many voices are live.
  #[cfg(test)]
  pub fn voice_count(&self) -> usize {
    self.voices.len()
  }

  /// Testing hook: the current ambient gain target is not exposed, so tests
  /// check the duck factor directly.
  #[cfg(test)]
  pub fn duck_factor(&self) -> f32 {
    self.active_duck(Instant::now())
  }
}

fn seed() -> u64 {
  let nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|duration| duration.as_nanos() as u64)
    .unwrap_or(0x9e37_79b9_7f4a_7c15);
  nanos | 1
}

/// Xorshift64*, good enough for pitch jitter and drop weights. Returns 0.0 to
/// 1.0.
fn next_random(state: &mut u64) -> f64 {
  let mut value = *state;
  value ^= value >> 12;
  value ^= value << 25;
  value ^= value >> 27;
  *state = value;
  let result = value.wrapping_mul(0x2545_F491_4F6C_DD1D);
  (result >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
  use super::{CHUNK_FRAMES, LoadedAmbient, LoadedSound, Mixer};
  use crate::audio::sink::{NullSink, Sink};
  use crate::audio::{AmbientSpec, Event, EventSound, Retrigger};
  use std::collections::HashMap;
  use std::sync::{Arc, Mutex};
  use std::time::{Duration, Instant};

  struct CaptureSink {
    rate: u32,
    samples: Arc<Mutex<Vec<i16>>>,
  }

  impl Sink for CaptureSink {
    fn sample_rate(&self) -> u32 {
      self.rate
    }

    fn write(&mut self, samples: &[i16]) -> Result<(), String> {
      self
        .samples
        .lock()
        .map_err(|_| "capture lock".to_string())?
        .extend_from_slice(samples);
      Ok(())
    }
  }

  fn sound(frames: usize) -> LoadedSound {
    LoadedSound {
      data: Arc::new(vec![0.1; frames * 2]),
      frames,
      spec: EventSound {
        file: "unused.ogg".into(),
        volume_db: 0.0,
        retrigger: Retrigger::Overlap,
        cooldown: Duration::ZERO,
        pitch_jitter: 0.0,
        duck_db: 0.0,
        max: None,
      },
    }
  }

  fn mixer(sounds: Vec<(Event, LoadedSound)>) -> Mixer {
    Mixer::new(
      Box::new(NullSink),
      1.0,
      16,
      sounds.into_iter().collect::<HashMap<_, _>>(),
      None,
    )
  }

  #[test]
  fn key_clicks_are_capped_at_twelve_per_second() {
    let mut mixer = mixer(vec![(Event::Key, sound(48_000))]);
    let now = Instant::now();
    let accepted = (0..20)
      .filter(|index| mixer.play(Event::Key, now + Duration::from_millis(*index * 10)))
      .count();
    assert_eq!(accepted, 12);
    assert_eq!(mixer.voice_count(), 12);
  }

  #[test]
  fn cooldown_blocks_a_second_click_too_soon() {
    let mut loaded = sound(480);
    loaded.spec.cooldown = Duration::from_millis(80);
    let mut mixer = mixer(vec![(Event::Key, loaded)]);
    let now = Instant::now();
    assert!(mixer.play(Event::Key, now));
    assert!(!mixer.play(Event::Key, now + Duration::from_millis(20)));
    assert!(mixer.play(Event::Key, now + Duration::from_millis(90)));
  }

  #[test]
  fn restart_replaces_the_previous_voice() {
    let mut loaded = sound(48_000);
    loaded.spec.retrigger = Retrigger::Restart;
    let mut mixer = mixer(vec![(Event::Boot, loaded)]);
    let now = Instant::now();
    assert!(mixer.play(Event::Boot, now));
    assert!(mixer.play(Event::Boot, now + Duration::from_millis(10)));
    assert_eq!(mixer.voice_count(), 1);
  }

  #[test]
  fn a_stinger_ducks_the_bed() {
    let mut loaded = sound(48_000);
    loaded.spec.duck_db = -6.0;
    let ambient = LoadedAmbient {
      data: Arc::new(vec![0.0; CHUNK_FRAMES * 2]),
      frames: CHUNK_FRAMES,
      spec: AmbientSpec {
        file: "ambient.ogg".into(),
        volume_db: -12.0,
        fade_in: Duration::ZERO,
        duck_db: -6.0,
        duck_ms: Duration::from_millis(200),
      },
    };
    let mut mixer = Mixer::new(
      Box::new(NullSink),
      1.0,
      16,
      HashMap::from([(Event::Success, loaded)]),
      Some(ambient),
    );
    assert!(mixer.play(Event::Success, Instant::now()));
    assert!(mixer.duck_factor() < 0.9);
  }

  #[test]
  fn an_unknown_event_is_ignored() {
    let mut mixer = mixer(Vec::new());
    assert!(!mixer.play(Event::Success, Instant::now()));
    assert_eq!(mixer.voice_count(), 0);
  }

  #[test]
  fn a_chunk_mixes_without_panicking() {
    let mut mixer = mixer(vec![(Event::Boot, sound(CHUNK_FRAMES * 2))]);
    assert!(mixer.play(Event::Boot, Instant::now()));
    mixer.write_chunk().expect("mix");
    assert_eq!(mixer.voice_count(), 1);
  }

  #[test]
  fn a_full_scale_ambient_reaches_the_output() {
    let samples = Arc::new(Mutex::new(Vec::new()));
    let ambient = LoadedAmbient {
      data: Arc::new(vec![0.5; CHUNK_FRAMES * 2]),
      frames: CHUNK_FRAMES,
      spec: AmbientSpec {
        file: "ambient.ogg".into(),
        volume_db: 0.0,
        fade_in: Duration::ZERO,
        duck_db: 0.0,
        duck_ms: Duration::ZERO,
      },
    };
    let mut mixer = Mixer::new(
      Box::new(CaptureSink {
        rate: 48_000,
        samples: Arc::clone(&samples),
      }),
      1.0,
      16,
      HashMap::new(),
      Some(ambient),
    );
    mixer.write_chunk().expect("mix");
    let captured = samples.lock().expect("capture");
    assert_eq!(captured.len(), CHUNK_FRAMES * 2);
    assert!(captured.iter().any(|sample| sample.abs() > 16_000));
  }

  #[test]
  fn a_44100_device_gets_441_frame_chunks() {
    let samples = Arc::new(Mutex::new(Vec::new()));
    let mut mixer = Mixer::new(
      Box::new(CaptureSink {
        rate: 44_100,
        samples: Arc::clone(&samples),
      }),
      1.0,
      16,
      HashMap::new(),
      None,
    );
    mixer.write_chunk().expect("mix");
    assert_eq!(samples.lock().expect("capture").len(), 441 * 2);
  }
}
