//! Read-only local hardware and kernel readings for the HUD widgets.
//!
//! Everything here comes from a small set of `/sys` and `/proc` files. That is
//! deliberate: a greeter runs before the user's session and must not depend on
//! D-Bus services, a daemon, or a network call to show a battery level or a
//! load average. The reads are bounded and every failure degrades to an empty
//! string or a zero, so the login screen can never be stopped by a missing file
//! on an unusual kernel.
//!
//! The root path is a parameter rather than a constant. That keeps the parser
//! testable with a fake tree and makes the production calls read from `/`
//! without a second code path.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A single read never consumes more than this many bytes. Battery and proc
/// files are tiny; the cap is there so a malicious or broken file cannot make
/// the UI tick allocate without bound.
const MAX_READ_BYTES: u64 = 8 * 1024;

/// Longest string we keep from a proc file. Hostnames and kernel release
/// strings are far shorter than this, and the renderer has a fixed width.
const MAX_STRING_CHARS: usize = 128;

/// The last battery reading we could make.
///
/// `present = false` is the honest answer on a desktop or in a container, and
/// the renderer turns it into a calm "NO BATTERY" label instead of a fake 0%.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatterySnapshot {
  pub present: bool,
  /// Charge percentage, already clamped to 0..=100.
  pub percent: u8,
  pub charging: bool,
  pub full: bool,
}

impl BatterySnapshot {
  /// Reads the first usable battery under `root/sys/class/power_supply`.
  ///
  /// A directory counts as a battery when its `type` file says `Battery` and
  /// its `present` file is either missing or `1`. Missing files inside an
  /// otherwise usable battery are treated as zero/false, which is what a
  /// partially populated sysfs tree looks like on some laptops.
  #[must_use]
  pub fn read(root: &Path) -> Self {
    let directory = root.join("sys/class/power_supply");
    let Ok(entries) = std::fs::read_dir(directory) else {
      return Self::default();
    };
    let mut paths: Vec<PathBuf> = entries
      .filter_map(Result::ok)
      .map(|entry| entry.path())
      .collect();
    // read_dir order is filesystem order. Sorting makes the choice stable when
    // a machine exposes more than one pack.
    paths.sort();
    for path in paths {
      if read_text(&path.join("type")) != "Battery" {
        continue;
      }
      let present = read_text(&path.join("present"));
      if !present.is_empty() && present != "1" {
        continue;
      }
      let percent = read_text(&path.join("capacity"))
        .parse::<i64>()
        .map_or(0, |value| value.clamp(0, 100) as u8);
      let status = read_text(&path.join("status"));
      return Self {
        present: true,
        percent,
        charging: status.eq_ignore_ascii_case("Charging"),
        full: status.eq_ignore_ascii_case("Full"),
      };
    }
    Self::default()
  }
}

/// The local system readings used by the system monitor.
///
/// `mem_used` and `mem_total` are in KiB, exactly the unit `/proc/meminfo`
/// reports. Keeping that unit avoids a conversion at read time and leaves the
/// formatting decision to [`format_memory`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SystemSnapshot {
  pub hostname: String,
  pub kernel: String,
  pub uptime: Duration,
  pub load: [f32; 3],
  pub mem_used: u64,
  pub mem_total: u64,
  /// Recent one-minute load samples, newest last. The app owns the refresh
  /// cadence and the renderer only reads this for the sparkline.
  pub load_history: Vec<f32>,
}

impl SystemSnapshot {
  /// Reads hostname, kernel, uptime, load average, and memory from `root`.
  ///
  /// Missing or unreadable files become empty strings and zeros. The function
  /// never returns an error because a greeter with a broken `/proc` mount still
  /// has to draw a login form.
  #[must_use]
  pub fn read(root: &Path) -> Self {
    let hostname = read_text(&root.join("proc/sys/kernel/hostname"));
    let kernel = read_text(&root.join("proc/sys/kernel/osrelease"));
    let uptime = read_raw(&root.join("proc/uptime"))
      .split_whitespace()
      .next()
      .and_then(|value| value.parse::<f64>().ok())
      .filter(|value| value.is_finite() && *value >= 0.0)
      .and_then(|value| Duration::try_from_secs_f64(value).ok())
      .unwrap_or_default();
    let load = parse_load(&read_raw(&root.join("proc/loadavg")));
    let (mem_total, mem_available) = parse_memory(&read_raw(&root.join("proc/meminfo")));
    Self {
      hostname,
      kernel,
      uptime,
      load,
      mem_used: mem_total.saturating_sub(mem_available),
      mem_total,
      load_history: Vec::new(),
    }
  }
}

/// Formats an uptime as a compact HUD label.
///
/// Days win over hours and hours win over minutes, so the string stays short:
/// `3d 04h`, `02h 14m`, or `14m`. A value under a minute reads as `0m`, which
/// is more honest than a blank field right after boot.
#[must_use]
pub fn format_uptime(uptime: Duration) -> String {
  let total = uptime.as_secs();
  let days = total / 86_400;
  let hours = (total % 86_400) / 3_600;
  let minutes = (total % 3_600) / 60;
  if days > 0 {
    format!("{days}d {hours:02}h")
  } else if hours > 0 {
    format!("{hours:02}h {minutes:02}m")
  } else {
    format!("{minutes}m")
  }
}

/// Formats used and total memory as GiB with one decimal.
///
/// Inputs are KiB, matching [`SystemSnapshot::mem_used`] and
/// [`SystemSnapshot::mem_total`]. The result looks like `6.2 / 15.5 GiB`.
#[must_use]
pub fn format_memory(used: u64, total: u64) -> String {
  let gib = |kib: u64| kib as f64 / (1024.0 * 1024.0);
  format!("{:.1} / {:.1} GiB", gib(used), gib(total))
}

/// Formats a charge percentage for the battery label.
#[must_use]
pub fn format_percent(percent: u8) -> String {
  format!("{percent}%")
}

/// Reads a file and turns it into a bounded, control-free string.
///
/// We read bytes rather than calling `read_to_string` so a file with one bad
/// byte still contributes its readable prefix instead of disappearing. Control
/// characters are stripped because the value is painted directly, and the
/// final string is capped so a weird proc file cannot stretch a widget.
fn read_text(path: &Path) -> String {
  read_raw(path)
    .chars()
    .filter(|character| !character.is_control())
    .collect::<String>()
    .trim()
    .chars()
    .take(MAX_STRING_CHARS)
    .collect()
}

/// Reads a bounded file without filtering newlines.
///
/// The multi-line proc parsers need the line structure, so they call this and
/// do their own trimming. String values use [`read_text`], which removes the
/// control characters that a HUD label must never contain.
fn read_raw(path: &Path) -> String {
  let Ok(file) = File::open(path) else {
    return String::new();
  };
  let mut bytes = Vec::new();
  if file.take(MAX_READ_BYTES).read_to_end(&mut bytes).is_err() {
    return String::new();
  }
  String::from_utf8_lossy(&bytes).into_owned()
}

/// Parses the first three floats of `/proc/loadavg`.
fn parse_load(text: &str) -> [f32; 3] {
  let mut load = [0.0f32; 3];
  for (slot, token) in load.iter_mut().zip(text.split_whitespace()) {
    *slot = token
      .parse::<f32>()
      .ok()
      .filter(|value| value.is_finite() && *value >= 0.0)
      .unwrap_or(0.0);
  }
  load
}

/// Parses `MemTotal` and `MemAvailable` from `/proc/meminfo`, in KiB.
fn parse_memory(text: &str) -> (u64, u64) {
  let mut total = 0;
  let mut available = 0;
  for line in text.lines() {
    let Some((key, rest)) = line.split_once(':') else {
      continue;
    };
    let value = rest
      .split_whitespace()
      .next()
      .and_then(|value| value.parse::<u64>().ok())
      .unwrap_or(0);
    match key.trim() {
      "MemTotal" => total = value,
      "MemAvailable" => available = value,
      _ => {}
    }
  }
  (total, available)
}

#[cfg(test)]
mod tests {
  use super::{BatterySnapshot, SystemSnapshot, format_memory, format_percent, format_uptime};
  use std::path::{Path, PathBuf};
  use std::time::Duration;

  fn root(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dawn-sysinfo-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
  }

  fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
  }

  #[test]
  fn reads_a_present_battery_and_clamps_capacity() {
    let root = root("battery");
    write(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
    write(&root, "sys/class/power_supply/BAT0/present", "1\n");
    write(&root, "sys/class/power_supply/BAT0/capacity", "180\n");
    write(&root, "sys/class/power_supply/BAT0/status", "Charging\n");
    let battery = BatterySnapshot::read(&root);
    assert!(battery.present);
    assert_eq!(battery.percent, 100, "capacity must be clamped");
    assert!(battery.charging);
    assert!(!battery.full);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn a_non_battery_entry_is_skipped() {
    let root = root("non-battery");
    write(&root, "sys/class/power_supply/AC/type", "Mains\n");
    write(&root, "sys/class/power_supply/AC/present", "1\n");
    write(&root, "sys/class/power_supply/AC/capacity", "99\n");
    let battery = BatterySnapshot::read(&root);
    assert!(!battery.present);
    assert_eq!(battery.percent, 0);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn a_missing_present_file_still_counts_as_present() {
    let root = root("missing-present");
    write(&root, "sys/class/power_supply/BAT1/type", "Battery\n");
    write(&root, "sys/class/power_supply/BAT1/capacity", "42\n");
    write(&root, "sys/class/power_supply/BAT1/status", "Full\n");
    let battery = BatterySnapshot::read(&root);
    assert!(battery.present);
    assert_eq!(battery.percent, 42);
    assert!(battery.full);
    assert!(!battery.charging);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn a_missing_root_degrades_to_no_battery() {
    let battery = BatterySnapshot::read(Path::new("/nonexistent/dawn-sysinfo"));
    assert!(!battery.present);
    assert_eq!(battery.percent, 0);
  }

  #[test]
  fn reads_system_files_and_ignores_missing_ones() {
    let root = root("system");
    write(&root, "proc/sys/kernel/hostname", "dawn-host\n");
    write(&root, "proc/sys/kernel/osrelease", "6.12.4-dawn\n");
    write(&root, "proc/uptime", "93784.50 120.00\n");
    write(&root, "proc/loadavg", "0.42 0.31 0.20 2/300 1234\n");
    write(
      &root,
      "proc/meminfo",
      "MemTotal:       16252928 kB\nMemFree:  1000 kB\nMemAvailable:   6501171 kB\n",
    );
    let system = SystemSnapshot::read(&root);
    assert_eq!(system.hostname, "dawn-host");
    assert_eq!(system.kernel, "6.12.4-dawn");
    assert_eq!(system.uptime.as_secs(), 93_784);
    assert_eq!(system.load, [0.42, 0.31, 0.20]);
    assert_eq!(system.mem_total, 16_252_928);
    assert_eq!(system.mem_used, 16_252_928 - 6_501_171);
    assert!(system.load_history.is_empty());

    let empty = SystemSnapshot::read(Path::new("/nonexistent/dawn-sysinfo"));
    assert!(empty.hostname.is_empty());
    assert_eq!(empty.uptime, Duration::ZERO);
    assert_eq!(empty.load, [0.0, 0.0, 0.0]);
    assert_eq!(empty.mem_total, 0);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn strings_are_stripped_and_bounded() {
    let root = root("bounded");
    let long = format!("{}{}\n", "h".repeat(200), "\u{7}");
    write(&root, "proc/sys/kernel/hostname", &long);
    let system = SystemSnapshot::read(&root);
    assert_eq!(system.hostname.chars().count(), 128);
    assert!(!system.hostname.chars().any(char::is_control));
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn uptime_formatting_stays_compact() {
    assert_eq!(
      format_uptime(Duration::from_secs(3 * 86_400 + 4 * 3_600)),
      "3d 04h"
    );
    assert_eq!(
      format_uptime(Duration::from_secs(2 * 3_600 + 14 * 60)),
      "02h 14m"
    );
    assert_eq!(format_uptime(Duration::from_secs(14 * 60)), "14m");
    assert_eq!(format_uptime(Duration::from_secs(30)), "0m");
  }

  #[test]
  fn memory_and_percent_formatting_match_the_hud_labels() {
    assert_eq!(format_memory(6_501_171, 16_252_928), "6.2 / 15.5 GiB");
    assert_eq!(format_memory(0, 0), "0.0 / 0.0 GiB");
    assert_eq!(format_percent(7), "7%");
    assert_eq!(format_percent(100), "100%");
  }
}
