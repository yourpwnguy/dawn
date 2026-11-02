//! Theme audio: ambient beds, interface feedback, and result stings.
//!
//! The design is in `docs/audio.md`. The important properties are:
//!
//! - sound is on by default, and the host config can mute it at any time;
//! - the mixer is one thread behind a bounded queue, so the UI never waits;
//! - a missing device, a missing file, or a bad file only removes sound;
//! - the device is released when the process exits, before the session starts.
//!
//! Theme files are decoded on the audio thread. A slow ambient file delays the
//! first sound, never the login screen.

mod decode;
mod mixer;
mod sink;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::time::{Duration, Instant};

use tracing::{debug, warn};

pub use decode::SAMPLE_RATE;
use mixer::{LoadedAmbient, LoadedSound, Mixer};
use sink::{AlsaSink, Sink};

/// One engine event a theme can attach a sound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Event {
  Boot,
  Key,
  Backspace,
  FieldFocus,
  Submit,
  Success,
  Failure,
  Cancel,
  SessionChange,
  RarityCommon,
  RarityRare,
  RaritySaga,
  Milestone,
  Achievement,
  ErrorConfig,
}

impl Event {
  pub const ALL: [Self; 15] = [
    Self::Boot,
    Self::Key,
    Self::Backspace,
    Self::FieldFocus,
    Self::Submit,
    Self::Success,
    Self::Failure,
    Self::Cancel,
    Self::SessionChange,
    Self::RarityCommon,
    Self::RarityRare,
    Self::RaritySaga,
    Self::Milestone,
    Self::Achievement,
    Self::ErrorConfig,
  ];

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Boot => "boot",
      Self::Key => "key",
      Self::Backspace => "backspace",
      Self::FieldFocus => "field_focus",
      Self::Submit => "submit",
      Self::Success => "success",
      Self::Failure => "failure",
      Self::Cancel => "cancel",
      Self::SessionChange => "session_change",
      Self::RarityCommon => "rarity_common",
      Self::RarityRare => "rarity_rare",
      Self::RaritySaga => "rarity_saga",
      Self::Milestone => "milestone",
      Self::Achievement => "achievement",
      Self::ErrorConfig => "error_config",
    }
  }

  #[must_use]
  pub fn parse(name: &str) -> Option<Self> {
    Self::ALL
      .into_iter()
      .find(|event| event.as_str().eq_ignore_ascii_case(name.trim()))
  }
}

/// What to do when an event fires while its previous voice is still playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Retrigger {
  /// Cut the previous voice and start from the top.
  #[default]
  Restart,
  /// Add another voice, up to the voice cap.
  Overlap,
  /// Ignore the new trigger until the old voice ends.
  Ignore,
}

impl Retrigger {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value.trim().to_ascii_lowercase().as_str() {
      "restart" => Some(Self::Restart),
      "overlap" => Some(Self::Overlap),
      "ignore" => Some(Self::Ignore),
      _ => None,
    }
  }
}

/// One event sound from the theme config.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSound {
  pub file: PathBuf,
  pub volume_db: f32,
  pub retrigger: Retrigger,
  pub cooldown: Duration,
  pub pitch_jitter: f32,
  pub duck_db: f32,
  pub max: Option<Duration>,
}

/// The looping bed from the theme config.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientSpec {
  pub file: PathBuf,
  pub volume_db: f32,
  pub fade_in: Duration,
  pub duck_db: f32,
  pub duck_ms: Duration,
}

impl Default for AmbientSpec {
  fn default() -> Self {
    Self {
      file: PathBuf::new(),
      volume_db: -12.0,
      fade_in: Duration::from_millis(600),
      duck_db: -10.0,
      duck_ms: Duration::from_millis(250),
    }
  }
}

/// The host's audio policy. These fields survive theme selection.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPolicy {
  pub enabled: bool,
  pub volume: u8,
  pub device: String,
  pub max_voices: u8,
  pub stop_on_login: bool,
}

impl Default for AudioPolicy {
  fn default() -> Self {
    Self {
      // Sound is part of the shipped experience now. A theme without audio
      // content still plays nothing because `should_start` also checks that
      // there is an ambient bed or an event to play. The machine can always
      // turn it off with `enabled = false` or `DAWN_AUDIO=0`.
      enabled: true,
      volume: 60,
      // `auto` asks ALSA for the best local output. This is the system-
      // independent default: nobody has to edit a theme for their own card.
      device: "auto".to_string(),
      max_voices: 8,
      stop_on_login: true,
    }
  }
}

/// The complete audio configuration: host policy plus theme content.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioConfig {
  pub enabled: bool,
  pub volume: u8,
  pub device: String,
  pub max_voices: u8,
  pub stop_on_login: bool,
  pub ambient: Option<AmbientSpec>,
  /// Set when `[background].audio` is enabled and the wallpaper is a video.
  /// The track is decoded through ffmpeg and used instead of `ambient.file`.
  pub wallpaper_audio: Option<PathBuf>,
  pub events: BTreeMap<Event, EventSound>,
}

impl AudioConfig {
  #[must_use]
  pub fn policy(&self) -> AudioPolicy {
    AudioPolicy {
      enabled: self.enabled,
      volume: self.volume,
      device: self.device.clone(),
      max_voices: self.max_voices,
      stop_on_login: self.stop_on_login,
    }
  }

  /// Reapplies the host policy after a theme config replaced the file.
  pub fn apply_policy(&mut self, policy: &AudioPolicy) {
    self.enabled = policy.enabled;
    self.volume = policy.volume;
    self.device.clone_from(&policy.device);
    self.max_voices = policy.max_voices;
    self.stop_on_login = policy.stop_on_login;
  }

  /// `DAWN_AUDIO=0` (or `off`/`false`) mutes for this process. `1` (or
  /// `on`/`true`) enables, which is what `dawn dev` uses.
  pub fn apply_env_value(&mut self, value: Option<&str>) {
    let Some(value) = value else {
      return;
    };
    match value.trim().to_ascii_lowercase().as_str() {
      "0" | "off" | "false" | "no" | "mute" => self.enabled = false,
      "1" | "on" | "true" | "yes" => self.enabled = true,
      _ => {}
    }
  }

  pub fn apply_env(&mut self) {
    let value = std::env::var("DAWN_AUDIO").ok();
    self.apply_env_value(value.as_deref());
  }

  /// True when there is something to play and the host allows it.
  #[must_use]
  pub fn should_start(&self) -> bool {
    self.enabled
      && (self.ambient.is_some() || self.wallpaper_audio.is_some() || !self.events.is_empty())
  }

  /// One-line summary for `--check-config`.
  #[must_use]
  pub fn summary(&self) -> String {
    let ambient = self.ambient.as_ref().map_or("none".to_string(), |ambient| {
      ambient.file.display().to_string()
    });
    let wallpaper = self
      .wallpaper_audio
      .as_ref()
      .map_or("none".to_string(), |path| path.display().to_string());
    format!(
      "enabled={} device={} volume={} events={} ambient={} wallpaper_audio={}",
      self.enabled,
      self.device,
      self.volume,
      self.events.len(),
      ambient,
      wallpaper
    )
  }
}

impl Default for AudioConfig {
  fn default() -> Self {
    let policy = AudioPolicy::default();
    Self {
      enabled: policy.enabled,
      volume: policy.volume,
      device: policy.device,
      max_voices: policy.max_voices,
      stop_on_login: policy.stop_on_login,
      ambient: None,
      wallpaper_audio: None,
      events: BTreeMap::new(),
    }
  }
}

/// Maintenance commands for the audio output path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioCommand {
  /// List ALSA playback PCM names and descriptions.
  Devices,
  /// Open one device and play a short test tone.
  Test { device: String },
}

/// Runs `dawn audio ...`. This is a diagnostic path: it never touches the
/// login screen and it prints the device that ALSA actually accepted.
pub fn run_cli(command: AudioCommand) -> std::process::ExitCode {
  use std::process::ExitCode;

  match command {
    AudioCommand::Devices => match list_devices() {
      Ok(devices) => {
        if devices.is_empty() {
          println!("no ALSA playback devices found");
          return ExitCode::FAILURE;
        }
        for (name, description) in devices {
          if description.is_empty() {
            println!("{name}");
          } else {
            println!("{name:<36} {description}");
          }
        }
        println!();
        println!("test one with: dawn audio test --device NAME");
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("dawn: {error}");
        ExitCode::FAILURE
      }
    },
    AudioCommand::Test { device } => match test_device(&device) {
      Ok((name, rate)) => {
        println!("opened {name} at {rate} Hz; played a 0.5 s test tone");
        ExitCode::SUCCESS
      }
      Err(error) => {
        print_device_error(&error);
        ExitCode::FAILURE
      }
    },
  }
}

/// Prints the raw ALSA error plus the explanation people actually need. A busy
/// analog card while a desktop session is running is not a dawn bug.
fn print_device_error(error: &str) {
  eprintln!("dawn: {error}");
  let lower = error.to_ascii_lowercase();
  if lower.contains("busy") {
    eprintln!(
      "hint: PipeWire or PulseAudio currently owns this card. The greeter starts before your session, so test from a TTY or stop the user sound server first."
    );
  } else if lower.contains("host is down") {
    eprintln!(
      "hint: `default` points at a sound server that is not running. Use an explicit ALSA name such as sysdefault:CARD=Generic_1 or hw:2,0."
    );
  }
}

fn list_devices() -> Result<Vec<(String, String)>, String> {
  use alsa::device_name::HintIter;
  use std::collections::BTreeSet;

  let iter =
    HintIter::new_str(None, "pcm").map_err(|error| format!("cannot list ALSA devices: {error}"))?;
  let mut seen = BTreeSet::new();
  let mut devices = Vec::new();
  for hint in iter {
    if matches!(hint.direction, Some(alsa::Direction::Capture)) {
      continue;
    }
    let Some(name) = hint.name else {
      continue;
    };
    if !seen.insert(name.clone()) {
      continue;
    }
    let description = hint
      .desc
      .unwrap_or_default()
      .split_whitespace()
      .collect::<Vec<_>>()
      .join(" ");
    devices.push((name, description));
  }
  Ok(devices)
}

fn test_device(device: &str) -> Result<(String, u32), String> {
  let mut sink = AlsaSink::open(device)?;
  let rate = sink.sample_rate();
  let frames = (rate / 2).max(1) as usize;
  let mut samples = Vec::with_capacity(frames * 2);
  for index in 0..frames {
    let t = index as f32 / rate as f32;
    let fade = (t / 0.02).min(1.0) * ((0.5 - t) / 0.05).clamp(0.0, 1.0);
    let value = ((std::f32::consts::TAU * 440.0 * t).sin() * 0.18 * fade * 32_767.0) as i16;
    samples.push(value);
    samples.push(value);
  }
  // Write in the same 10 ms chunks as the mixer. A single 0.5 s write can
  // overrun a small ALSA buffer and look like a device failure.
  for chunk in samples.chunks(960) {
    sink.write(chunk)?;
  }
  Ok((sink.name().to_string(), rate))
}

enum Command {
  Play(Event),
  Stop,
}

/// The UI-facing handle. Dropping it does not wait for the mixer; the channel
/// closes when the process exits, which closes the device.
pub struct AudioHandle {
  tx: Option<SyncSender<Command>>,
  dropped: Arc<AtomicU64>,
}

impl AudioHandle {
  #[must_use]
  pub fn disabled() -> Self {
    Self {
      tx: None,
      dropped: Arc::new(AtomicU64::new(0)),
    }
  }

  /// Starts the mixer thread when the policy allows it. Asset paths in
  /// `config` are already resolved against the config file's directory by the
  /// validator, so nothing here depends on the process working directory.
  #[must_use]
  pub fn start(config: &AudioConfig) -> Self {
    if !config.should_start() {
      return Self::disabled();
    }
    let (tx, rx) = mpsc::sync_channel(64);
    let dropped = Arc::new(AtomicU64::new(0));
    let dropped_for_thread = Arc::clone(&dropped);
    let config = config.clone();
    let spawned = std::thread::Builder::new()
      .name("dawn-audio".to_string())
      .spawn(move || run(rx, &config, &dropped_for_thread));
    match spawned {
      Ok(_) => Self {
        tx: Some(tx),
        dropped,
      },
      Err(error) => {
        warn!("audio: cannot start the mixer thread: {error}");
        Self::disabled()
      }
    }
  }

  /// Queues one event. Never blocks; a full queue drops the sound.
  pub fn play(&self, event: Event) {
    let Some(tx) = &self.tx else {
      return;
    };
    if tx.try_send(Command::Play(event)).is_err() {
      self.dropped.fetch_add(1, Ordering::Relaxed);
    }
  }

  /// Stops playback and releases the device. Called on the login handoff; it
  /// does not wait for the thread.
  pub fn stop(&self) {
    if let Some(tx) = &self.tx {
      let _ = tx.try_send(Command::Stop);
    }
    let dropped = self.dropped.load(Ordering::Relaxed);
    if dropped > 0 {
      debug!("audio: {dropped} sound(s) dropped because the queue was full");
    }
  }

  #[must_use]
  pub fn enabled(&self) -> bool {
    self.tx.is_some()
  }
}

impl Drop for AudioHandle {
  fn drop(&mut self) {
    if let Some(tx) = &self.tx {
      let _ = tx.try_send(Command::Stop);
    }
  }
}

fn run(rx: mpsc::Receiver<Command>, config: &AudioConfig, dropped: &Arc<AtomicU64>) {
  let sink: Box<dyn Sink> = match AlsaSink::open(&config.device) {
    Ok(sink) => Box::new(sink),
    Err(error) => {
      warn!("audio: {error}; continuing silently");
      return;
    }
  };

  let mut sounds = HashMap::new();
  for (event, spec) in &config.events {
    match decode::decode(&spec.file, decode::Kind::Event) {
      Ok(clip) => {
        debug!(
          "audio: loaded {} for {} ({:.2} s)",
          spec.file.display(),
          event.as_str(),
          clip.seconds()
        );
        sounds.insert(
          *event,
          LoadedSound {
            frames: clip.frames(),
            data: Arc::new(clip.samples),
            spec: spec.clone(),
          },
        );
      }
      Err(error) => warn!("audio: {error}; {} stays silent", event.as_str()),
    }
  }

  let bed_spec = config.ambient.clone().unwrap_or_default();
  let mut ambient = None;
  if let Some(video) = &config.wallpaper_audio {
    match decode::decode_video_audio(video) {
      Ok(clip) => {
        debug!(
          "audio: loaded wallpaper track {} ({:.1} s)",
          video.display(),
          clip.seconds()
        );
        ambient = Some(LoadedAmbient {
          frames: clip.frames(),
          data: Arc::new(clip.samples),
          spec: bed_spec.clone(),
        });
      }
      Err(error) => {
        debug!("audio: wallpaper audio unavailable ({error}); trying the ambient file");
      }
    }
  }
  if ambient.is_none()
    && let Some(spec) = &config.ambient
  {
    match decode::decode(&spec.file, decode::Kind::Ambient) {
      Ok(clip) => {
        debug!(
          "audio: loaded ambient {} ({:.1} s)",
          spec.file.display(),
          clip.seconds()
        );
        ambient = Some(LoadedAmbient {
          frames: clip.frames(),
          data: Arc::new(clip.samples),
          spec: spec.clone(),
        });
      }
      Err(error) => {
        warn!("audio: {error}; ambient stays silent");
      }
    }
  }

  if sounds.is_empty() && ambient.is_none() {
    warn!("audio: no playable assets; continuing silently");
    return;
  }

  let mut mixer = Mixer::new(
    sink,
    f32::from(config.volume) / 100.0,
    usize::from(config.max_voices),
    sounds,
    ambient,
  );
  loop {
    while let Ok(command) = rx.try_recv() {
      match command {
        Command::Play(event) => {
          mixer.play(event, Instant::now());
        }
        Command::Stop => {
          let dropped = dropped.load(Ordering::Relaxed);
          if dropped > 0 {
            debug!("audio: {dropped} sound(s) dropped because the queue was full");
          }
          return;
        }
      }
    }
    if let Err(error) = mixer.write_chunk() {
      warn!("audio: {error}; stopping playback");
      return;
    }
  }
}

/// dBFS to a linear gain factor.
#[must_use]
pub(crate) fn db_to_gain(db: f32) -> f32 {
  10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
  use super::{AudioConfig, AudioPolicy, Event, Retrigger};

  #[test]
  fn events_round_trip_through_their_names() {
    for event in Event::ALL {
      assert_eq!(Event::parse(event.as_str()), Some(event));
    }
    assert_eq!(Event::parse("nope"), None);
  }

  #[test]
  fn an_empty_theme_does_not_start_the_mixer() {
    let mut config = AudioConfig::default();
    assert!(!config.should_start());
    config.enabled = true;
    assert!(!config.should_start());
  }

  #[test]
  fn the_host_policy_survives_a_theme_config() {
    let mut theme = AudioConfig {
      enabled: true,
      volume: 100,
      device: "hw:9".to_string(),
      ..AudioConfig::default()
    };
    theme.apply_policy(&AudioPolicy {
      enabled: false,
      volume: 25,
      device: "default".to_string(),
      max_voices: 4,
      stop_on_login: true,
    });
    assert!(!theme.enabled);
    assert_eq!(theme.volume, 25);
    assert_eq!(theme.device, "default");
    assert_eq!(theme.max_voices, 4);
  }

  #[test]
  fn the_environment_can_mute_and_unmute() {
    let mut config = AudioConfig::default();
    config.apply_env_value(Some("0"));
    assert!(!config.enabled);
    config.apply_env_value(Some("on"));
    assert!(config.enabled);
    config.apply_env_value(Some("garbage"));
    assert!(config.enabled);
  }

  #[test]
  fn retrigger_names_parse() {
    assert_eq!(Retrigger::parse("Restart"), Some(Retrigger::Restart));
    assert_eq!(Retrigger::parse("overlap"), Some(Retrigger::Overlap));
    assert_eq!(Retrigger::parse("ignore"), Some(Retrigger::Ignore));
    assert_eq!(Retrigger::parse("loop"), None);
  }
}
