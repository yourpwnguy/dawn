//! `dawn use`: switch the theme the next login starts.
//!
//! The command edits only the `package` key inside the `[theme]` table of the
//! machine config. Comments, ordering, and every other key stay as they were,
//! which matters because admins hand-edit this file. The patched text is parsed
//! again before it is written, so a bad edit is rejected instead of replacing a
//! working config.
//!
//! When the config does not exist yet, the embedded first-run template is used
//! as the starting point. The write path is shared with `dawn init`, including
//! the 0644 mode the greeter account needs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::config::{ENV_CONFIG, SYSTEM_CONFIG};
use crate::fsutil::write_atomic;
use crate::init::DEFAULT_CONFIG;
use crate::theme::{Origin, list, valid_id};

/// Runs `dawn use ID` and turns every outcome into an exit code.
pub fn run(id: &str, explicit: Option<&Path>) -> ExitCode {
  if !valid_id(id) {
    eprintln!("dawn: invalid theme id {id:?}");
    return ExitCode::FAILURE;
  }

  let (entries, _) = list();
  let Some(entry) = entries.iter().find(|entry| entry.id == id) else {
    eprintln!("dawn: theme {id:?} is not installed; run `dawn list` to see the ids");
    return ExitCode::FAILURE;
  };
  if entry.origin == Origin::User {
    eprintln!(
      "warning: {id} is in the user store; the greeter may not be able to read it. \
       Reinstall it with `dawn add {} --force`.",
      entry.path.display()
    );
  }

  let path = target_path(explicit);
  let (source, created) = match fs::read_to_string(&path) {
    Ok(source) => (source, false),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      (DEFAULT_CONFIG.to_string(), true)
    }
    Err(error) => {
      eprintln!("dawn: cannot read {}: {error}", path.display());
      return ExitCode::FAILURE;
    }
  };

  let patched = match set_theme_package(&source, id) {
    Ok(patched) => patched,
    Err(error) => {
      eprintln!("dawn: {error}");
      return ExitCode::FAILURE;
    }
  };
  if let Err(error) = write_atomic(&path, &patched, 0o644) {
    eprintln!("dawn: {error}");
    return ExitCode::FAILURE;
  }

  if created {
    println!("created {} and selected {id}", path.display());
  } else {
    println!("selected {id} in {}", path.display());
  }
  println!("the next login uses it");
  ExitCode::SUCCESS
}

/// The config a switch should edit: explicit path, `DAWN_CONFIG`, then the
/// system file. This mirrors the run-time discovery order on purpose.
pub(crate) fn target_path(explicit: Option<&Path>) -> PathBuf {
  if let Some(path) = explicit {
    return path.to_path_buf();
  }
  if let Some(value) = std::env::var_os(ENV_CONFIG).filter(|value| !value.is_empty()) {
    return PathBuf::from(value);
  }
  PathBuf::from(SYSTEM_CONFIG)
}

/// Rewrites the `[theme].package` value in place.
///
/// The edit is line based so the rest of the file, including comments, stays
/// byte for byte identical. The result is validated as TOML before it is
/// returned; a syntax error means the original file had a shape this editor
/// does not understand, and the caller must not write it.
fn set_theme_package(source: &str, id: &str) -> Result<String, String> {
  let newline = if source.contains("\r\n") {
    "\r\n"
  } else {
    "\n"
  };
  let mut lines: Vec<String> = source.split_inclusive('\n').map(str::to_string).collect();
  let replacement = format!("package = \"{id}\"");

  match lines
    .iter()
    .position(|line| table_header(line) == Some("theme"))
  {
    Some(header) => {
      let end = next_table(&lines, header + 1);
      match (header + 1..end).find(|&index| is_key(&lines[index], "package")) {
        Some(index) => {
          let ending = line_ending(&lines[index]);
          let (indent, comment) = split_comment(&lines[index]);
          let indent = &indent[..indent.len() - indent.trim_start().len()];
          let mut line = format!("{indent}{replacement}");
          if !comment.is_empty() {
            line.push(' ');
            line.push_str(comment.trim_end());
          }
          lines[index] = format!("{line}{ending}");
        }
        None => {
          if !lines[header].ends_with('\n') {
            lines[header].push_str(newline);
          }
          lines.insert(header + 1, format!("{replacement}{newline}"));
        }
      }
    }
    None => {
      if !source.is_empty() && !source.ends_with('\n') {
        lines.push(newline.to_string());
      }
      if !source.is_empty() {
        lines.push(newline.to_string());
      }
      lines.push(format!("[theme]{newline}"));
      lines.push(format!("{replacement}{newline}"));
    }
  }

  let patched = lines.concat();
  let value: toml::Value = toml::from_str(&patched)
    .map_err(|error| format!("the edited config is not valid TOML: {error}"))?;
  let selected = value
    .get("theme")
    .and_then(|theme| theme.get("package"))
    .and_then(toml::Value::as_str);
  if selected != Some(id) {
    return Err("the edited config did not keep the selected theme".to_string());
  }
  Ok(patched)
}

/// The table name when the line is exactly `[name]`, ignoring a trailing
/// comment. `[theme.package]` is a different table and returns None here.
fn table_header(line: &str) -> Option<&str> {
  let (code, _) = split_comment(line);
  let code = code.trim().trim_end_matches('\r');
  code
    .strip_prefix('[')
    .and_then(|rest| rest.strip_suffix(']'))
    .filter(|name| !name.contains('[') && !name.contains(']'))
}

fn next_table(lines: &[String], start: usize) -> usize {
  lines
    .iter()
    .enumerate()
    .skip(start)
    .find(|(_, line)| table_header(line).is_some())
    .map_or(lines.len(), |(index, _)| index)
}

fn is_key(line: &str, key: &str) -> bool {
  let (code, _) = split_comment(line);
  let Some(rest) = code.trim_start().strip_prefix(key) else {
    return false;
  };
  rest.trim_start().starts_with('=')
}

fn line_ending(line: &str) -> &'static str {
  if line.ends_with("\r\n") {
    "\r\n"
  } else if line.ends_with('\n') {
    "\n"
  } else {
    ""
  }
}

/// Splits a line into the part before a `#` that is outside a string and the
/// comment itself. Inline TOML strings may contain a hash, so a plain
/// `find('#')` would corrupt `package = "a#b"`.
fn split_comment(line: &str) -> (&str, &str) {
  let mut in_basic = false;
  let mut in_literal = false;
  let mut escaped = false;
  for (index, character) in line.char_indices() {
    if escaped {
      escaped = false;
      continue;
    }
    match character {
      '\\' if in_basic => escaped = true,
      '"' if !in_literal => in_basic = !in_basic,
      '\'' if !in_basic => in_literal = !in_literal,
      '#' if !in_basic && !in_literal => return (&line[..index], &line[index..]),
      _ => {}
    }
  }
  (line, "")
}

#[cfg(test)]
mod tests {
  use super::set_theme_package;

  #[test]
  fn replaces_an_existing_package_and_keeps_the_comment() {
    let source = "[theme]\npreset = \"glass-dark\"\npackage = \"old\" # keep me\n\n[login]\ngreeting = \"Hi\"\n";
    let patched = set_theme_package(source, "midnight").expect("patch");
    assert!(
      patched.contains("package = \"midnight\" # keep me"),
      "{patched}"
    );
    assert!(!patched.contains("package = \"old\""), "{patched}");
    assert!(patched.contains("[login]\ngreeting = \"Hi\""), "{patched}");
  }

  #[test]
  fn inserts_the_key_into_an_existing_table() {
    let source = "[theme]\npreset = \"minimal\"\n\n[login]\nwidth = 400\n";
    let patched = set_theme_package(source, "midnight").expect("patch");
    let theme = patched
      .split("[login]")
      .next()
      .expect("theme section present");
    assert!(theme.contains("package = \"midnight\""), "{patched}");
    assert!(theme.contains("preset = \"minimal\""), "{patched}");
    assert!(patched.contains("[login]\nwidth = 400"), "{patched}");
  }

  #[test]
  fn appends_a_theme_table_when_missing() {
    let source = "[login]\nwidth = 400\n";
    let patched = set_theme_package(source, "midnight").expect("patch");
    assert!(
      patched.ends_with("[theme]\npackage = \"midnight\"\n"),
      "{patched}"
    );
  }

  #[test]
  fn a_header_without_a_trailing_newline_still_gets_a_valid_insert() {
    let patched = set_theme_package("[theme]", "midnight").expect("patch");
    assert_eq!(patched, "[theme]\npackage = \"midnight\"\n");
  }

  #[test]
  fn does_not_touch_a_package_key_in_another_table() {
    let source = "[other]\npackage = \"keep\"\n\n[theme]\npreset = \"minimal\"\n";
    let patched = set_theme_package(source, "midnight").expect("patch");
    assert!(patched.contains("[other]\npackage = \"keep\""), "{patched}");
    assert_eq!(patched.matches("package =").count(), 2, "{patched}");
  }

  #[test]
  fn keeps_crlf_endings() {
    let source = "[theme]\r\npreset = \"minimal\"\r\n";
    let patched = set_theme_package(source, "midnight").expect("patch");
    assert!(
      patched.contains("\r\npackage = \"midnight\"\r\n"),
      "{patched}"
    );
    assert!(
      !patched.contains("\npackage") || patched.contains("\r\npackage"),
      "{patched}"
    );
  }

  #[test]
  fn a_hash_inside_a_string_is_not_a_comment() {
    let source = "[theme]\npreset = \"a#b\"\n";
    let patched = set_theme_package(source, "midnight").expect("patch");
    assert!(patched.contains("preset = \"a#b\""), "{patched}");
    assert!(patched.contains("package = \"midnight\""), "{patched}");
  }

  #[test]
  fn an_invalid_patch_is_rejected() {
    // A duplicated table would be invalid TOML; the editor must not write it.
    let source = "[theme]\n[theme]\n";
    assert!(set_theme_package(source, "midnight").is_err());
  }
}
