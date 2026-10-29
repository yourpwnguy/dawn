//! MPRIS media metadata over D-Bus.
//!
//! This runs on its own thread and never touches the render, auth, or greetd
//! paths. It polls the *greeter's* session bus, which is normally not the
//! user's session bus: greetd starts the greeter before the user logs in, so
//! the user's Spotify is usually unreachable. The widget therefore has a calm
//! idle state and the renderer refuses to show real metadata until the view is
//! marked authenticated. That is a deliberate product constraint, not a bug.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use tiny_skia::Pixmap;
use tracing::debug;

use crate::ui::image::{self, DecodeLimits};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const ART_LIMITS: DecodeLimits = DecodeLimits {
  max_file_bytes: 8 * 1024 * 1024,
  max_dimension: 4096,
  max_pixels: 4 * 1024 * 1024,
  max_alloc: 64 * 1024 * 1024,
};
const ART_SIZE: (u32, u32) = (256, 256);

/// What the media widget shows. It contains no credential and no user input.
#[derive(Clone)]
pub struct MediaSnapshot {
  pub available: bool,
  pub player: String,
  pub title: String,
  pub artist: String,
  pub album: String,
  pub playing: bool,
  pub position: Duration,
  pub length: Duration,
  /// True when this is the last known track and no player is currently
  /// answering. The widget uses it to draw a quiet "Last played" caption.
  pub stale: bool,
  /// Album art, already decoded and downscaled through [`crate::ui::image`].
  pub art: Option<Arc<Pixmap>>,
  /// The URI the art came from, so we only decode it when it changes.
  pub art_key: String,
}

impl Default for MediaSnapshot {
  fn default() -> Self {
    Self {
      available: false,
      player: String::new(),
      title: String::new(),
      artist: String::new(),
      album: String::new(),
      playing: false,
      position: Duration::ZERO,
      length: Duration::ZERO,
      stale: false,
      art: None,
      art_key: String::new(),
    }
  }
}

impl MediaSnapshot {
  /// Compare only what the widget can display. Comparing `Arc<Pixmap>` would
  /// walk every pixel on every poll.
  fn visible_eq(&self, other: &Self) -> bool {
    self.available == other.available
      && self.player == other.player
      && self.title == other.title
      && self.artist == other.artist
      && self.album == other.album
      && self.playing == other.playing
      && self.position == other.position
      && self.length == other.length
      && self.stale == other.stale
      && self.art_key == other.art_key
  }
}

/// Shared, thread-safe widget state.
pub struct MediaState {
  snapshot: Mutex<MediaSnapshot>,
  generation: AtomicU64,
  /// Keep the last visible track when the player pauses, stops, or exits.
  keep_last: bool,
}

impl MediaState {
  #[must_use]
  pub fn new() -> Self {
    Self::with_keep_last(true)
  }

  #[must_use]
  pub fn with_keep_last(keep_last: bool) -> Self {
    Self {
      snapshot: Mutex::new(MediaSnapshot::default()),
      generation: AtomicU64::new(0),
      keep_last,
    }
  }

  #[must_use]
  pub fn keep_last(&self) -> bool {
    self.keep_last
  }

  #[must_use]
  pub fn snapshot(&self) -> MediaSnapshot {
    match self.snapshot.lock() {
      Ok(snapshot) => snapshot.clone(),
      Err(poisoned) => poisoned.into_inner().clone(),
    }
  }

  #[must_use]
  pub fn generation(&self) -> u64 {
    self.generation.load(Ordering::Relaxed)
  }

  fn update(&self, next: MediaSnapshot) -> bool {
    let mut snapshot = match self.snapshot.lock() {
      Ok(snapshot) => snapshot,
      Err(poisoned) => poisoned.into_inner(),
    };
    if snapshot.visible_eq(&next) {
      return false;
    }
    *snapshot = next;
    self.generation.fetch_add(1, Ordering::Relaxed);
    true
  }
}

impl Default for MediaState {
  fn default() -> Self {
    Self::new()
  }
}

/// Controls the widget can send to the player. The renderer only exposes these
/// after authentication, and the worker ignores them when no player exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaControl {
  PlayPause,
  Next,
  Previous,
}

/// Stops the media thread when the window closes. Dropping it is enough; the
/// worker checks the flag between polls and while sleeping.
pub struct MediaHandle {
  stop: Arc<AtomicBool>,
  controls: mpsc::Sender<MediaControl>,
}

impl MediaHandle {
  #[must_use]
  pub fn control(&self, control: MediaControl) -> bool {
    self.controls.send(control).is_ok()
  }
}

impl Drop for MediaHandle {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
  }
}

/// Starts the MPRIS poller. `on_change` is called only when visible state
/// changed, so an idle desktop does not wake the UI every two seconds.
#[must_use]
pub fn spawn(state: Arc<MediaState>, on_change: impl Fn() + Send + 'static) -> MediaHandle {
  let stop = Arc::new(AtomicBool::new(false));
  let worker_stop = Arc::clone(&stop);
  let (controls, control_rx) = mpsc::channel();
  let spawned = std::thread::Builder::new()
    .name("dawn-media".to_string())
    .spawn(move || run(state, worker_stop, control_rx, on_change));
  if let Err(error) = spawned {
    // Losing the media widget is not a reason to lose the login screen.
    debug!(%error, "cannot start the media worker; the widget stays idle");
  }
  MediaHandle { stop, controls }
}

/// The snapshot to show when no player is answering.
///
/// A player can disappear when it is closed or when it briefly re-registers on
/// the bus. Wiping the widget to "No media" loses the only useful information
/// we had. Keep the last track, mark it stale, and let the next real snapshot
/// replace it. `keep_last = false` restores the old privacy-first behaviour.
fn when_no_player(current: MediaSnapshot, keep_last: bool) -> MediaSnapshot {
  if keep_last && current.available {
    MediaSnapshot {
      playing: false,
      stale: true,
      ..current
    }
  } else {
    MediaSnapshot::default()
  }
}

fn run(
  state: Arc<MediaState>,
  stop: Arc<AtomicBool>,
  controls: mpsc::Receiver<MediaControl>,
  on_change: impl Fn(),
) {
  while !stop.load(Ordering::Relaxed) {
    let connection = match zbus::block_on(zbus::Connection::session()) {
      Ok(connection) => connection,
      Err(error) => {
        debug!(%error, "no session bus; media widget stays idle");
        sleep_unless_stopped(&stop, Duration::from_secs(30));
        continue;
      }
    };

    loop {
      if stop.load(Ordering::Relaxed) {
        return;
      }
      let previous = state.snapshot();
      match zbus::block_on(poll(&connection, &previous)) {
        Ok(Some(snapshot)) => {
          if state.update(snapshot) {
            on_change();
          }
        }
        Ok(None) => {
          let next = when_no_player(state.snapshot(), state.keep_last());
          if state.update(next) {
            on_change();
          }
        }
        Err(error) => {
          // The bus or the player disappeared. Reconnect instead of polling a
          // dead connection every two seconds for the rest of the session.
          debug!(%error, "MPRIS poll failed; reconnecting");
          break;
        }
      }
      while let Ok(control) = controls.try_recv() {
        let player = state.snapshot().player;
        if player.is_empty() {
          continue;
        }
        if let Err(error) = zbus::block_on(send_control(&connection, &player, control)) {
          debug!(%error, "cannot send media control");
        }
      }
      sleep_unless_stopped(&stop, POLL_INTERVAL);
    }
  }
}

fn sleep_unless_stopped(stop: &AtomicBool, duration: Duration) {
  let step = Duration::from_millis(100);
  let mut remaining = duration;
  while !remaining.is_zero() {
    if stop.load(Ordering::Relaxed) {
      return;
    }
    let sleep = remaining.min(step);
    std::thread::sleep(sleep);
    remaining = remaining.saturating_sub(sleep);
  }
}

/// Reads the selected player. `previous` is used only to reuse decoded album
/// art: the art URL usually stays the same for minutes, and re-reading and
/// re-scaling a 4K cover every two seconds would be pure waste.
async fn poll(
  connection: &zbus::Connection,
  previous: &MediaSnapshot,
) -> Result<Option<MediaSnapshot>, zbus::Error> {
  let dbus = zbus::fdo::DBusProxy::new(connection).await?;
  let names = dbus.list_names().await?;
  let mut players: Vec<String> = names
    .into_iter()
    .map(|name| name.to_string())
    .filter(|name| name.starts_with("org.mpris.MediaPlayer2."))
    .collect();
  players.sort();

  let mut selected = None;
  for player in &players {
    let Some(status) = playback_status(connection, player).await? else {
      continue;
    };
    if status == "Playing" {
      selected = Some((player.clone(), status));
      break;
    }
    if selected.is_none() {
      selected = Some((player.clone(), status));
    }
  }

  let Some((player, status)) = selected else {
    return Ok(None);
  };

  let metadata = metadata(connection, &player).await.unwrap_or_default();
  let position = position(connection, &player)
    .await
    .unwrap_or(Duration::ZERO);
  let title = string_value(metadata.get("xesam:title")).map(clean_text);
  let artist = artist_value(metadata.get("xesam:artist")).map(clean_text);
  let album = string_value(metadata.get("xesam:album")).map(clean_text);
  let art_url = string_value(metadata.get("mpris:artUrl"))
    .filter(|url| url.chars().count() <= 1024)
    .map(clean_text);
  let length = metadata
    .get("mpris:length")
    .and_then(|value| i64::try_from(value.try_clone().ok()?).ok())
    .and_then(|micros| u64::try_from(micros).ok())
    .map(Duration::from_micros)
    .unwrap_or(Duration::ZERO);

  let art_key = art_url.clone().unwrap_or_default();
  let art = if !art_key.is_empty() && art_key == previous.art_key && previous.art.is_some() {
    previous.art.clone()
  } else {
    art_url.as_deref().and_then(load_art).map(Arc::new)
  };

  Ok(Some(MediaSnapshot {
    available: title.as_deref().is_some_and(|value| !value.is_empty())
      || artist.as_deref().is_some_and(|value| !value.is_empty()),
    player,
    title: title.unwrap_or_default(),
    artist: artist.unwrap_or_default(),
    album: album.unwrap_or_default(),
    playing: status == "Playing",
    position,
    length,
    stale: false,
    art_key,
    art,
  }))
}

async fn playback_status(
  connection: &zbus::Connection,
  player: &str,
) -> Result<Option<String>, zbus::Error> {
  let proxy = player_proxy(connection, player).await?;
  match proxy.get_property::<String>("PlaybackStatus").await {
    Ok(status) => Ok(Some(status)),
    Err(error) => {
      debug!(%error, player, "cannot read PlaybackStatus");
      Ok(None)
    }
  }
}

/// Builds a player proxy without a property cache.
///
/// We read one or two properties right after creating the proxy and then throw
/// it away, so a cache has nothing to save. More importantly the default lazy
/// cache fills itself with a `GetAll`, and playerctld answers that with
/// `NoActivePlayer` whenever the user has no player open. zbus logs that as a
/// warning on every poll, which would spam the greeter journal every two
/// seconds for a totally normal state.
async fn player_proxy<'a>(
  connection: &zbus::Connection,
  player: &'a str,
) -> Result<zbus::Proxy<'a>, zbus::Error> {
  zbus::proxy::Builder::new(connection)
    .destination(player)
    .path("/org/mpris/MediaPlayer2")
    .interface("org.mpris.MediaPlayer2.Player")
    .cache_properties(zbus::proxy::CacheProperties::No)
    .build()
    .await
}

async fn metadata(
  connection: &zbus::Connection,
  player: &str,
) -> Result<HashMap<String, zbus::OwnedVariant>, zbus::Error> {
  let proxy = player_proxy(connection, player).await?;
  proxy.get_property("Metadata").await
}

async fn position(connection: &zbus::Connection, player: &str) -> Result<Duration, zbus::Error> {
  let proxy = player_proxy(connection, player).await?;
  let micros = proxy.get_property::<i64>("Position").await?;
  Ok(
    u64::try_from(micros)
      .map(Duration::from_micros)
      .unwrap_or(Duration::ZERO),
  )
}

async fn send_control(
  connection: &zbus::Connection,
  player: &str,
  control: MediaControl,
) -> Result<(), zbus::Error> {
  let proxy = player_proxy(connection, player).await?;
  let method = match control {
    MediaControl::PlayPause => "PlayPause",
    MediaControl::Next => "Next",
    MediaControl::Previous => "Previous",
  };
  proxy.call_method(method, &()).await?;
  Ok(())
}

fn string_value(value: Option<&zbus::OwnedVariant>) -> Option<String> {
  let value = value?.try_clone().ok()?;
  String::try_from(value).ok()
}

fn artist_value(value: Option<&zbus::OwnedVariant>) -> Option<String> {
  let value = value?.try_clone().ok()?;
  if let Ok(artists) = Vec::<String>::try_from(value.try_clone().ok()?) {
    return Some(artists.join(", "));
  }
  String::try_from(value).ok()
}

fn load_art(url: &str) -> Option<Pixmap> {
  let path = art_path(url)?;
  // The URI comes from an untrusted D-Bus property. Check the file type and
  // size BEFORE reading: `fs::read` on /dev/zero or a 4 GiB file would happily
  // eat the greeter's memory before the decoder ever sees a limit.
  let metadata = std::fs::metadata(&path).ok()?;
  if !metadata.is_file() {
    debug!(path = %path.display(), "album art is not a regular file");
    return None;
  }
  if metadata.len() > ART_LIMITS.max_file_bytes {
    debug!(path = %path.display(), "album art is too large");
    return None;
  }
  let file = std::fs::File::open(&path).ok()?;
  let mut bytes = Vec::with_capacity(metadata.len() as usize);
  if file
    .take(ART_LIMITS.max_file_bytes + 1)
    .read_to_end(&mut bytes)
    .is_err()
    || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > ART_LIMITS.max_file_bytes
  {
    debug!(path = %path.display(), "cannot read album art within the limit");
    return None;
  }
  let image = match image::decode_bytes(&bytes, ART_LIMITS) {
    Ok(image) => image,
    Err(error) => {
      debug!(path = %path.display(), %error, "cannot decode album art");
      return None;
    }
  };
  let fitted = image::fit_image(
    image,
    ART_SIZE,
    crate::layout::Fit::Cover,
    crate::color::Rgb::solid(0, 0, 0),
  );
  image::to_pixmap(fitted)
}

fn art_path(url: &str) -> Option<PathBuf> {
  let rest = url.strip_prefix("file://")?;
  let rest = rest.strip_prefix("localhost").unwrap_or(rest);
  if !rest.starts_with('/') {
    return None;
  }
  let decoded = percent_decode(rest);
  if decoded.is_empty() || decoded.chars().any(char::is_control) {
    return None;
  }
  Some(PathBuf::from(decoded))
}

/// Album metadata is untrusted text from another process. Strip control
/// characters, squeeze whitespace, and bound the length so a hostile player
/// cannot distort the layout or smuggle terminal escapes into the log.
fn clean_text(value: String) -> String {
  value
    .chars()
    .filter(|ch| !ch.is_control())
    .take(200)
    .collect::<String>()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

fn percent_decode(input: &str) -> String {
  let bytes = input.as_bytes();
  let mut output = Vec::with_capacity(bytes.len());
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'%' && index + 2 < bytes.len() {
      let hi = char::from(bytes[index + 1]).to_digit(16);
      let lo = char::from(bytes[index + 2]).to_digit(16);
      if let (Some(hi), Some(lo)) = (hi, lo) {
        output.push((hi * 16 + lo) as u8);
        index += 3;
        continue;
      }
    }
    output.push(bytes[index]);
    index += 1;
  }
  String::from_utf8_lossy(&output).into_owned()
}

#[cfg(test)]
mod tests {
  use super::{
    MediaControl, MediaSnapshot, MediaState, art_path, clean_text, percent_decode, when_no_player,
  };

  #[test]
  fn visible_equality_ignores_art_pixels() {
    let left = MediaSnapshot::default();
    let mut right = MediaSnapshot::default();
    assert!(left.visible_eq(&right));
    right.title = "Song".to_string();
    assert!(!left.visible_eq(&right));
  }

  #[test]
  fn state_generation_only_moves_on_visible_change() {
    let state = MediaState::new();
    assert_eq!(state.generation(), 0);
    assert!(!state.update(MediaSnapshot::default()));
    let next = MediaSnapshot {
      title: "Song".to_string(),
      ..MediaSnapshot::default()
    };
    assert!(state.update(next));
    assert_eq!(state.generation(), 1);
  }

  #[test]
  fn file_uris_are_decoded_and_http_is_ignored() {
    assert_eq!(
      art_path("file:///tmp/a%20b.png"),
      Some(std::path::PathBuf::from("/tmp/a b.png"))
    );
    assert_eq!(art_path("https://example.invalid/a.png"), None);
    assert_eq!(art_path("file://localhostrelative.png"), None);
    assert_eq!(percent_decode("a%20b"), "a b");
    assert_eq!(
      clean_text(" a\u{1b}[31m  title \n".to_string()),
      "a[31m title"
    );
  }

  #[test]
  fn the_last_track_survives_a_disappearing_player() {
    let current = MediaSnapshot {
      available: true,
      title: "Song".to_string(),
      artist: "Artist".to_string(),
      playing: true,
      ..MediaSnapshot::default()
    };
    let next = when_no_player(current.clone(), true);
    assert!(next.available);
    assert_eq!(next.title, "Song");
    assert!(!next.playing);
    assert!(next.stale);

    let next = when_no_player(current, false);
    assert!(!next.available);
    assert!(!next.stale);
  }

  #[test]
  fn controls_are_sendable() {
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(MediaControl::PlayPause).unwrap();
    assert_eq!(receiver.recv().unwrap(), MediaControl::PlayPause);
  }
}
