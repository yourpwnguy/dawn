//! Best-effort username prefill for the login form.
//!
//! A greeter is usually a single-user door. Making the one person who owns the
//! machine type their own account name every boot is friction, not security:
//! the name is already printed on every other greeter and it is not a
//! credential. This module resolves a sensible starting value without ever
//! touching authentication.
//!
//! The order is deliberately conservative:
//!
//! 1. `login.prefill = "name"` uses exactly that name.
//! 2. `login.prefill = "none"` keeps the field empty.
//! 3. `login.prefill = "auto"` reads the last interactive login from
//!    `/var/log/wtmp`, then falls back to the lowest-uid regular account in
//!    `/etc/passwd`.
//!
//! Both files are untrusted input as far as this module is concerned. A
//! missing, unreadable, malformed, or truncated file simply returns `None`.
//! That is the same outcome as `none`, and it can never delay the greeter: the
//! reads are bounded and happen once at startup on the UI thread.
//!
//! We deliberately do not use `getpwent`, libc, or a subprocess. The passwd
//! format is line-oriented and the wtmp record layout is stable enough for the
//! two fields we need. If a distribution changes either format, the fallback
//! is the empty field, which is still a working login screen.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::config::{UsernamePrefill, valid_username};

/// The machine account database. A greeter has no useful `HOME`, so this is
/// the one account source every local login can see.
const PASSWD: &str = "/etc/passwd";
/// Linux records interactive logins here. It is usually mode 0664 owned by
/// root:utmp, so a `greeter` user may not be able to read it; that is fine.
const WTMP: &str = "/var/log/wtmp";
/// `struct utmp` is 384 bytes on the Linux ABIs dawn supports. We only read
/// `ut_type` and `ut_user`, so an unknown but larger record would still not
/// produce a valid account name and is ignored.
const WTMP_RECORD: usize = 384;
const WTMP_USER_PROCESS: i16 = 7;
const WTMP_USER_OFFSET: usize = 44;
const WTMP_USER_LEN: usize = 32;
/// Read only the tail of wtmp. The file grows forever and we need the newest
/// record, not a login history in memory.
const WTMP_TAIL_BYTES: u64 = 256 * 1024;
/// Regular login accounts start here on every mainstream distribution.
const UID_MIN: u32 = 1000;
const UID_MAX: u32 = 60_000;

/// Resolves the configured prefill policy against the local machine.
#[must_use]
pub fn resolve(prefill: &UsernamePrefill) -> Option<String> {
  match prefill {
    UsernamePrefill::None => None,
    UsernamePrefill::Name(name) => Some(name.clone()),
    UsernamePrefill::Auto => last_login().or_else(first_human),
  }
}

/// Pure version used by tests and by [`resolve`], so the parsers never need a
/// real `/etc/passwd` or a writable `/var/log/wtmp`.
#[must_use]
pub fn resolve_from(prefill: &UsernamePrefill, passwd: &str, wtmp: &[u8]) -> Option<String> {
  match prefill {
    UsernamePrefill::None => None,
    UsernamePrefill::Name(name) => Some(name.clone()),
    UsernamePrefill::Auto => last_login_from(wtmp).or_else(|| first_human_from(passwd)),
  }
}

fn last_login() -> Option<String> {
  let mut file = File::open(Path::new(WTMP)).ok()?;
  let length = file.metadata().ok()?.len();
  if length < WTMP_RECORD as u64 {
    return None;
  }
  let start = length.saturating_sub(WTMP_TAIL_BYTES);
  // Start on a record boundary or every field would be shifted by the
  // difference between the file length and our arbitrary tail size.
  let start = start - start % WTMP_RECORD as u64;
  file.seek(SeekFrom::Start(start)).ok()?;
  let mut data = Vec::new();
  file.take(WTMP_TAIL_BYTES).read_to_end(&mut data).ok()?;
  last_login_from(&data)
}

/// Returns the last `USER_PROCESS` name in a wtmp byte slice.
///
/// Records are fixed width. The last valid record wins, which is what "last
/// login" means even when the file also contains boots, runlevel changes, and
/// failed logins.
fn last_login_from(data: &[u8]) -> Option<String> {
  let mut last = None;
  for record in data.as_chunks::<WTMP_RECORD>().0 {
    let kind = i16::from_ne_bytes([record[0], record[1]]);
    if kind != WTMP_USER_PROCESS {
      continue;
    }
    let raw = &record[WTMP_USER_OFFSET..WTMP_USER_OFFSET + WTMP_USER_LEN];
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let Ok(name) = std::str::from_utf8(&raw[..end]) else {
      continue;
    };
    if account_name(name) {
      last = Some(name.to_string());
    }
  }
  last
}

fn first_human() -> Option<String> {
  let passwd = std::fs::read_to_string(Path::new(PASSWD)).ok()?;
  first_human_from(&passwd)
}

/// Picks the lowest-uid regular account with a real home and an interactive
/// shell. UID 1000 is the first human account on almost every desktop, which
/// is exactly the person standing at the greeter.
fn first_human_from(passwd: &str) -> Option<String> {
  let mut best: Option<(u32, String)> = None;
  for line in passwd.lines() {
    let mut fields = line.splitn(7, ':');
    let (Some(name), Some(_), Some(uid), Some(_), Some(_), Some(home), Some(shell)) = (
      fields.next(),
      fields.next(),
      fields.next(),
      fields.next(),
      fields.next(),
      fields.next(),
      fields.next(),
    ) else {
      continue;
    };
    let Ok(uid) = uid.parse::<u32>() else {
      continue;
    };
    if !(UID_MIN..UID_MAX).contains(&uid) || !account_name(name) {
      continue;
    }
    if home.is_empty() || home == "/" || !home.starts_with('/') {
      continue;
    }
    if !interactive_shell(shell) {
      continue;
    }
    if best.as_ref().is_none_or(|(best_uid, _)| uid < *best_uid) {
      best = Some((uid, name.to_string()));
    }
  }
  best.map(|(_, name)| name)
}

fn account_name(value: &str) -> bool {
  valid_username(value) && value != "greeter"
}

fn interactive_shell(shell: &str) -> bool {
  let shell = shell.trim();
  !shell.is_empty()
    && !shell.ends_with("/nologin")
    && !shell.ends_with("/false")
    && !shell.ends_with("/sync")
    && !shell.ends_with("/shutdown")
    && !shell.ends_with("/halt")
}

#[cfg(test)]
mod tests {
  use super::{first_human_from, last_login_from, resolve_from};
  use crate::config::UsernamePrefill;

  const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
alice:x:1000:1000:Alice:/home/alice:/bin/zsh
service:x:1001:1001:Service:/opt/service:/usr/sbin/nologin
meow:x:1002:1002:Meow:/home/meow:/bin/bash
";

  fn wtmp_record(kind: i16, name: &str) -> [u8; 384] {
    let mut record = [0u8; 384];
    record[..2].copy_from_slice(&kind.to_ne_bytes());
    let raw = name.as_bytes();
    let len = raw.len().min(31);
    record[44..44 + len].copy_from_slice(&raw[..len]);
    record
  }

  #[test]
  fn the_lowest_uid_with_a_real_shell_wins() {
    assert_eq!(first_human_from(PASSWD).as_deref(), Some("alice"));
  }

  #[test]
  fn a_missing_or_empty_passwd_is_not_an_error() {
    assert_eq!(first_human_from(""), None);
    assert_eq!(first_human_from("not a passwd line"), None);
  }

  #[test]
  fn wtmp_keeps_the_last_interactive_login() {
    let mut data = Vec::new();
    data.extend_from_slice(&wtmp_record(7, "meow"));
    data.extend_from_slice(&wtmp_record(2, "reboot"));
    data.extend_from_slice(&wtmp_record(7, "alice"));
    data.extend_from_slice(&wtmp_record(8, "shutdown"));
    assert_eq!(last_login_from(&data).as_deref(), Some("alice"));
  }

  #[test]
  fn the_greeter_and_non_utf8_names_are_skipped() {
    let mut data = Vec::new();
    data.extend_from_slice(&wtmp_record(7, "greeter"));
    data.extend_from_slice(&wtmp_record(7, "bad name"));
    data.extend_from_slice(&wtmp_record(7, "alice"));
    assert_eq!(last_login_from(&data).as_deref(), Some("alice"));
  }

  #[test]
  fn prefill_policies_are_explicit() {
    assert_eq!(
      resolve_from(&UsernamePrefill::None, PASSWD, &[]),
      None,
      "none must keep the field empty"
    );
    assert_eq!(
      resolve_from(&UsernamePrefill::Name("kiosk".to_string()), PASSWD, &[]),
      Some("kiosk".to_string())
    );
    assert_eq!(
      resolve_from(&UsernamePrefill::Auto, PASSWD, &[]),
      Some("alice".to_string()),
      "without a readable wtmp, the first regular account is the fallback"
    );
  }

  #[test]
  fn auto_prefers_the_last_login_over_the_first_account() {
    let mut data = Vec::new();
    data.extend_from_slice(&wtmp_record(7, "meow"));
    assert_eq!(
      resolve_from(&UsernamePrefill::Auto, PASSWD, &data),
      Some("meow".to_string())
    );
  }
}
