//! A very small `.desktop` parser, scoped to what a greeter needs.
//!
//! We only read `[Desktop Entry]`. Localised keys (`Name[de]`) are deliberately
//! ignored for now: the session name is shown exactly once, and guessing a
//! locale from a root-run greeter is less reliable than just using `Name`.
//! Unknown keys are ignored because desktop files carry lots of data that has
//! nothing to do with launching a session.

/// The fields we care about from a session desktop file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
  pub name: Option<String>,
  pub exec: Option<String>,
  pub try_exec: Option<String>,
  pub desktop_names: Option<String>,
  pub entry_type: Option<String>,
  pub no_display: bool,
  pub hidden: bool,
}

/// Parses the relevant part of a desktop file. Malformed lines are skipped:
/// one bad line must not make the whole session disappear.
#[must_use]
pub fn parse(content: &str) -> DesktopEntry {
  let mut entry = DesktopEntry::default();
  let mut in_entry_group = false;

  for line in content.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    if line.starts_with('[') {
      in_entry_group = line.eq_ignore_ascii_case("[Desktop Entry]");
      continue;
    }
    if !in_entry_group {
      continue;
    }
    let Some((key, value)) = line.split_once('=') else {
      continue;
    };
    let key = key.trim();
    let value = unescape(value.trim());

    match key {
      "Name" if entry.name.is_none() => entry.name = Some(value),
      "Exec" if entry.exec.is_none() => entry.exec = Some(value),
      "TryExec" if entry.try_exec.is_none() => entry.try_exec = Some(value),
      "DesktopNames" if entry.desktop_names.is_none() => entry.desktop_names = Some(value),
      "Type" if entry.entry_type.is_none() => entry.entry_type = Some(value),
      "NoDisplay" => entry.no_display = value.eq_ignore_ascii_case("true"),
      "Hidden" => entry.hidden = value.eq_ignore_ascii_case("true"),
      _ => {}
    }
  }

  entry
}

/// Handles the escapes the desktop entry spec allows.
fn unescape(value: &str) -> String {
  let mut out = String::with_capacity(value.len());
  let mut chars = value.chars();
  while let Some(ch) = chars.next() {
    if ch != '\\' {
      out.push(ch);
      continue;
    }
    match chars.next() {
      Some('s') => out.push(' '),
      Some('n') => out.push('\n'),
      Some('t') => out.push('\t'),
      Some('r') => out.push('\r'),
      Some('\\') => out.push('\\'),
      Some(other) => {
        out.push('\\');
        out.push(other);
      }
      None => out.push('\\'),
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::parse;

  #[test]
  fn reads_the_entry_group_only() {
    let entry = parse(
      r#"
[Desktop Entry]
Type=Application
Name=Hyprland
Name[de]=Hyprland DE
Exec=Hyprland --start
DesktopNames=Hyprland;wlroots
NoDisplay=false

[Desktop Action new-window]
Name=Wrong
Exec=wrong
"#,
    );
    assert_eq!(entry.name.as_deref(), Some("Hyprland"));
    assert_eq!(entry.exec.as_deref(), Some("Hyprland --start"));
    assert_eq!(entry.desktop_names.as_deref(), Some("Hyprland;wlroots"));
    assert_eq!(entry.entry_type.as_deref(), Some("Application"));
    assert!(!entry.no_display);
  }

  #[test]
  fn unescapes_spec_sequences() {
    let entry = parse("[Desktop Entry]\nName=Hello\\sWorld\\nTest\\\\");
    assert_eq!(entry.name.as_deref(), Some("Hello World\nTest\\"));
  }

  #[test]
  fn booleans_are_case_insensitive() {
    let entry = parse("[Desktop Entry]\nHidden=TRUE\nNoDisplay=True\n");
    assert!(entry.hidden);
    assert!(entry.no_display);
  }

  #[test]
  fn handles_crlf_and_comments() {
    let entry = parse("# comment\r\n[Desktop Entry]\r\nName=X\r\nExec=x\r\n");
    assert_eq!(entry.name.as_deref(), Some("X"));
    assert_eq!(entry.exec.as_deref(), Some("x"));
  }
}
