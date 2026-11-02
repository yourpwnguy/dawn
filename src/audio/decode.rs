//! Decoding theme audio into interleaved stereo f32 at 48 kHz.
//!
//! Two formats are supported on purpose: OGG Vorbis for music and ambience,
//! and WAV for short clicks. Both decoders are pure Rust, so a theme cannot
//! pull a C codec stack into the greeter. Every limit here is enforced again
//! before playback even if validation already checked the file.
//!
//! Decoding happens on the audio thread, never on the UI thread. A slow
//! ambient file delays the first sound, not the login screen.

use std::fs::File;
use std::path::Path;

use hound::SampleFormat;
use lewton::inside_ogg::OggStreamReader;

/// The internal mix rate. Assets are resampled to this once at load.
pub const SAMPLE_RATE: u32 = 48_000;

/// One decoded sound, interleaved stereo at [`SAMPLE_RATE`].
pub struct Clip {
  pub samples: Vec<f32>,
}

impl Clip {
  #[must_use]
  pub fn frames(&self) -> usize {
    self.samples.len() / 2
  }

  #[must_use]
  pub fn seconds(&self) -> f32 {
    self.frames() as f32 / SAMPLE_RATE as f32
  }
}

/// Which cap applies. Events are short interface sounds; ambient is a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Event,
  Ambient,
}

const MAX_EVENT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_AMBIENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EVENT_SECONDS: f64 = 5.0;
const MAX_AMBIENT_SECONDS: f64 = 180.0;
const MAX_AMBIENT_DECODED_BYTES: usize = 80 * 1024 * 1024;
const MAX_SOURCE_SECONDS: f64 = 600.0;
const MAX_VIDEO_AUDIO_BYTES: u64 = 40 * 1024 * 1024;

/// Decodes a video wallpaper's audio track into 48 kHz stereo.
///
/// The video pipeline already depends on ffmpeg, so this reuses it instead of
/// linking a second set of container decoders. A video with no audio track
/// returns an error, which the caller treats as "fall back to the ambient
/// file" rather than a failure.
pub fn decode_video_audio(path: &Path) -> Result<Clip, String> {
  use std::io::Read;
  use std::process::{Command, Stdio};

  let mut child = Command::new("ffmpeg")
    .arg("-v")
    .arg("error")
    .arg("-i")
    .arg(path)
    .args(["-vn", "-f", "s16le", "-ac", "2", "-ar", "48000", "pipe:1"])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|error| format!("cannot run ffmpeg for {}: {error}", path.display()))?;
  let mut stdout = child
    .stdout
    .take()
    .ok_or_else(|| format!("{}: ffmpeg did not expose an audio pipe", path.display()))?;
  let mut bytes = Vec::new();
  let mut buffer = [0u8; 64 * 1024];
  loop {
    match stdout.read(&mut buffer) {
      Ok(0) => break,
      Ok(read) => {
        if bytes.len() as u64 + read as u64 > MAX_VIDEO_AUDIO_BYTES {
          let _ = child.kill();
          let _ = child.wait();
          return Err(format!(
            "{}: audio is longer than the supported ambient length",
            path.display()
          ));
        }
        bytes.extend_from_slice(&buffer[..read]);
      }
      Err(error) => {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{}: cannot read audio: {error}", path.display()));
      }
    }
  }
  let status = child
    .wait()
    .map_err(|error| format!("{}: ffmpeg did not finish: {error}", path.display()))?;
  if !status.success() {
    return Err(format!("{} has no decodable audio track", path.display()));
  }
  if bytes.len() < 2 {
    return Err(format!("{} has no audio samples", path.display()));
  }
  let samples: Vec<f32> = bytes
    .chunks_exact(2)
    .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32_768.0)
    .collect();
  let frames = samples.len() / 2;
  let seconds = frames as f64 / f64::from(SAMPLE_RATE);
  if seconds > MAX_AMBIENT_SECONDS {
    return Err(format!(
      "{}: {seconds:.1} s of audio is longer than the ambient limit",
      path.display()
    ));
  }
  Ok(Clip { samples })
}

/// Decodes one asset, applies the caps for its kind, and returns 48 kHz stereo.
pub fn decode(path: &Path, kind: Kind) -> Result<Clip, String> {
  let metadata =
    std::fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
  if !metadata.is_file() {
    return Err(format!("{} is not a regular file", path.display()));
  }
  let max_bytes = match kind {
    Kind::Event => MAX_EVENT_BYTES,
    Kind::Ambient => MAX_AMBIENT_BYTES,
  };
  if metadata.len() > max_bytes {
    return Err(format!(
      "{} is {} bytes; the limit for this sound is {}",
      path.display(),
      metadata.len(),
      max_bytes
    ));
  }

  let extension = path
    .extension()
    .and_then(|ext| ext.to_str())
    .map(|ext| ext.to_ascii_lowercase())
    .unwrap_or_default();
  let raw = match extension.as_str() {
    "ogg" => decode_ogg(path)?,
    "wav" => decode_wav(path)?,
    other => {
      return Err(format!(
        "{}: unsupported audio format {:?}; use .ogg or .wav",
        path.display(),
        other
      ));
    }
  };

  let max_seconds = match kind {
    Kind::Event => MAX_EVENT_SECONDS,
    Kind::Ambient => MAX_AMBIENT_SECONDS,
  };
  if raw.seconds > max_seconds {
    return Err(format!(
      "{} is {:.1} s; the limit is {max_seconds:.0} s",
      path.display(),
      raw.seconds
    ));
  }
  let clip = to_stereo_48k(&raw)?;
  if matches!(kind, Kind::Ambient)
    && clip.samples.len() * std::mem::size_of::<f32>() > MAX_AMBIENT_DECODED_BYTES
  {
    return Err(format!(
      "{} decodes to {} MiB; the ambient cap is {} MiB",
      path.display(),
      clip.samples.len() * std::mem::size_of::<f32>() / (1024 * 1024),
      MAX_AMBIENT_DECODED_BYTES / (1024 * 1024)
    ));
  }
  Ok(clip)
}

/// Interleaved samples at the source rate, normalised to -1.0 to 1.0.
struct Raw {
  samples: Vec<f32>,
  channels: usize,
  rate: u32,
  seconds: f64,
}

fn decode_wav(path: &Path) -> Result<Raw, String> {
  let mut reader =
    hound::WavReader::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
  let spec = reader.spec();
  if spec.channels == 0 || spec.channels > 2 {
    return Err(format!(
      "{}: {} channels; mono or stereo is supported",
      path.display(),
      spec.channels
    ));
  }
  if !(8_000..=192_000).contains(&spec.sample_rate) {
    return Err(format!(
      "{}: sample rate {} is outside 8000 to 192000",
      path.display(),
      spec.sample_rate
    ));
  }
  let samples = match (spec.sample_format, spec.bits_per_sample) {
    (SampleFormat::Float, 32) => reader
      .samples::<f32>()
      .collect::<Result<Vec<_>, _>>()
      .map_err(|error| format!("{}: {error}", path.display()))?,
    (SampleFormat::Int, 16) => reader
      .samples::<i16>()
      .map(|sample| sample.map(|value| f32::from(value) / 32_768.0))
      .collect::<Result<Vec<_>, _>>()
      .map_err(|error| format!("{}: {error}", path.display()))?,
    (SampleFormat::Int, 24) => reader
      .samples::<i32>()
      .map(|sample| sample.map(|value| value as f32 / 8_388_608.0))
      .collect::<Result<Vec<_>, _>>()
      .map_err(|error| format!("{}: {error}", path.display()))?,
    (SampleFormat::Int, 32) => reader
      .samples::<i32>()
      .map(|sample| sample.map(|value| value as f32 / 2_147_483_648.0))
      .collect::<Result<Vec<_>, _>>()
      .map_err(|error| format!("{}: {error}", path.display()))?,
    (format, bits) => {
      return Err(format!(
        "{}: unsupported WAV sample format {format:?} at {bits} bits; use PCM 16/24 or float 32",
        path.display()
      ));
    }
  };
  let channels = usize::from(spec.channels);
  let frames = samples.len() / channels;
  let seconds = frames as f64 / f64::from(spec.sample_rate);
  if seconds > MAX_SOURCE_SECONDS || seconds > MAX_AMBIENT_SECONDS {
    return Err(format!(
      "{}: {seconds:.1} s is longer than the supported audio length",
      path.display()
    ));
  }
  Ok(Raw {
    samples,
    channels,
    rate: spec.sample_rate,
    seconds,
  })
}

fn decode_ogg(path: &Path) -> Result<Raw, String> {
  let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
  let mut reader = OggStreamReader::new(file)
    .map_err(|error| format!("{}: cannot read OGG Vorbis: {error}", path.display()))?;
  let channels = usize::from(reader.ident_hdr.audio_channels);
  let rate = reader.ident_hdr.audio_sample_rate;
  if channels == 0 || channels > 2 {
    return Err(format!(
      "{}: {channels} channels; mono or stereo is supported",
      path.display()
    ));
  }
  if !(8_000..=192_000).contains(&rate) {
    return Err(format!(
      "{}: sample rate {rate} is outside 8000 to 192000",
      path.display()
    ));
  }

  let mut samples = Vec::new();
  let max_frames = (f64::from(rate) * MAX_AMBIENT_SECONDS) as usize + 1;
  loop {
    let packet = reader
      .read_dec_packet_itl()
      .map_err(|error| format!("{}: {error}", path.display()))?;
    let Some(packet) = packet else {
      break;
    };
    samples.extend(packet.into_iter().map(|value| f32::from(value) / 32_768.0));
    if samples.len() / channels > max_frames {
      return Err(format!(
        "{}: longer than the supported audio length",
        path.display()
      ));
    }
  }
  let frames = samples.len() / channels;
  let seconds = frames as f64 / f64::from(rate);
  if seconds > MAX_AMBIENT_SECONDS {
    return Err(format!(
      "{}: {seconds:.1} s is longer than the supported audio length",
      path.display()
    ));
  }
  Ok(Raw {
    samples,
    channels,
    rate,
    seconds,
  })
}

fn to_stereo_48k(raw: &Raw) -> Result<Clip, String> {
  let frames = raw.samples.len() / raw.channels;
  if frames == 0 {
    return Err("the file decodes to no audio frames".to_string());
  }
  let ratio = f64::from(SAMPLE_RATE) / f64::from(raw.rate);
  let out_frames = (frames as f64 * ratio).round() as usize;
  if out_frames == 0 || out_frames > MAX_AMBIENT_DECODED_BYTES / (2 * std::mem::size_of::<f32>()) {
    return Err("the decoded audio is larger than the supported length".to_string());
  }
  let mut out = Vec::with_capacity(out_frames * 2);
  for index in 0..out_frames {
    let source = index as f64 / ratio;
    let first = source.floor() as usize;
    let second = (first + 1).min(frames.saturating_sub(1));
    let fraction = (source - first as f64) as f32;
    let left = mix(raw, first, second, 0, fraction);
    let right = if raw.channels == 2 {
      mix(raw, first, second, 1, fraction)
    } else {
      left
    };
    out.push(left);
    out.push(right);
  }
  Ok(Clip { samples: out })
}

fn mix(raw: &Raw, first: usize, second: usize, channel: usize, fraction: f32) -> f32 {
  let a = raw.samples[first * raw.channels + channel];
  let b = raw.samples[second * raw.channels + channel];
  a + (b - a) * fraction
}

#[cfg(test)]
mod tests {
  use super::{Kind, SAMPLE_RATE, decode};
  use hound::{SampleFormat, WavSpec, WavWriter};
  use std::path::PathBuf;

  fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
      "dawn-audio-{name}-{}-{}.wav",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
    ))
  }

  fn write_wav(path: &PathBuf, rate: u32, channels: u16, samples: &[i16]) {
    let spec = WavSpec {
      channels,
      sample_rate: rate,
      bits_per_sample: 16,
      sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for sample in samples {
      writer.write_sample(*sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
  }

  #[test]
  fn mono_wav_is_duplicated_to_stereo_at_48k() {
    let path = temp_path("mono");
    write_wav(&path, 24_000, 1, &vec![1000; 24_000]);
    let clip = decode(&path, Kind::Event).expect("decode");
    assert_eq!(clip.frames(), SAMPLE_RATE as usize);
    assert!(clip.seconds() > 0.99 && clip.seconds() < 1.01);
    let left = clip.samples[0];
    let right = clip.samples[1];
    assert!((left - right).abs() < f32::EPSILON);
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn stereo_wav_keeps_both_channels() {
    let path = temp_path("stereo");
    let mut samples = Vec::new();
    for _ in 0..480 {
      samples.push(10_000);
      samples.push(-10_000);
    }
    write_wav(&path, 48_000, 2, &samples);
    let clip = decode(&path, Kind::Event).expect("decode");
    assert!((clip.samples[0] - 10_000.0 / 32_768.0).abs() < 0.001);
    assert!((clip.samples[1] + 10_000.0 / 32_768.0).abs() < 0.001);
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn a_long_event_is_rejected() {
    let path = temp_path("long");
    write_wav(&path, 48_000, 1, &vec![0; 48_000 * 6]);
    assert!(decode(&path, Kind::Event).is_err());
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn an_unknown_extension_is_rejected() {
    let path = temp_path("ext").with_extension("mp3");
    std::fs::write(&path, b"not audio").ok();
    assert!(decode(&path, Kind::Event).is_err());
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn a_video_track_decodes_through_ffmpeg() {
    use std::process::Command;

    let path = temp_path("clip").with_extension("mp4");
    let status = Command::new("ffmpeg")
      .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=64x64:d=0.4",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=0.4",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-shortest",
        "-y",
      ])
      .arg(&path)
      .status();
    // CI and normal developer machines have ffmpeg. Skip when it is absent
    // instead of turning an environment gap into a failed unit test.
    if !matches!(status, Ok(status) if status.success()) {
      std::fs::remove_file(&path).ok();
      return;
    }
    let clip = super::decode_video_audio(&path).expect("decode video audio");
    assert!(clip.frames() > 2_000, "{} frames", clip.frames());
    std::fs::remove_file(&path).ok();
  }
}
