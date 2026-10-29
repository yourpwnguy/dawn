//! Splits a desktop entry `Exec` value into an argv array.
//!
//! We deliberately do not run a shell: the session command comes from a desktop
//! file, and going through `sh -c` would turn a quoting bug into command
//! injection. The desktop entry spec has its own small quoting and field-code
//! rules, so this is a focused parser rather than a shell emulator.

use std::fmt;

const MAX_ARGS: usize = 64;
const MAX_ARG_CHARS: usize = 4096;
const MAX_TOTAL_CHARS: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
  Empty,
  TooManyArguments,
  ArgumentTooLong,
  CommandTooLong,
  UnknownFieldCode(char),
  TrailingPercent,
  UnterminatedQuote,
}

impl fmt::Display for ExecError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Empty => f.write_str("Exec is empty"),
      Self::TooManyArguments => write!(f, "Exec has more than {MAX_ARGS} arguments"),
      Self::ArgumentTooLong => write!(
        f,
        "Exec argument is longer than the {MAX_ARG_CHARS} character limit"
      ),
      Self::CommandTooLong => write!(f, "Exec is longer than the {MAX_TOTAL_CHARS} byte limit"),
      Self::UnknownFieldCode(code) => write!(f, "unknown Exec field code %{code}"),
      Self::TrailingPercent => f.write_str("Exec ends with a bare '%'"),
      Self::UnterminatedQuote => f.write_str("Exec has an unterminated quote"),
    }
  }
}

impl std::error::Error for ExecError {}

/// Parses an `Exec` value into arguments with field codes removed.
///
/// Field codes such as `%f` and `%u` describe a file or URL to open. A login
/// session does not have one, so dropping them is correct per the spec. `%i` is
/// the one code that expands to multiple arguments; it is dropped as well
/// because we do not read the `Icon` key it would refer to.
pub fn parse_exec(input: &str) -> Result<Vec<String>, ExecError> {
  if input.len() > MAX_TOTAL_CHARS {
    return Err(ExecError::CommandTooLong);
  }
  let mut args = Vec::new();
  let mut current = String::new();
  let mut has_token = false;
  let mut chars = input.chars().peekable();

  while let Some(ch) = chars.next() {
    match ch {
      ' ' | '\t' => {
        if has_token {
          push_arg(&mut args, std::mem::take(&mut current))?;
          has_token = false;
        }
      }
      '"' => {
        has_token = true;
        loop {
          match chars.next() {
            Some('"') => break,
            Some('\\') => match chars.next() {
              Some(escaped @ ('"' | '\\' | '`' | '$')) => current.push(escaped),
              Some(other) => {
                current.push('\\');
                current.push(other);
              }
              None => return Err(ExecError::UnterminatedQuote),
            },
            Some(other) => current.push(other),
            None => return Err(ExecError::UnterminatedQuote),
          }
        }
      }
      '\'' => {
        has_token = true;
        loop {
          match chars.next() {
            Some('\'') => break,
            Some(other) => current.push(other),
            None => return Err(ExecError::UnterminatedQuote),
          }
        }
      }
      '\\' => {
        has_token = true;
        match chars.next() {
          Some(escaped) => current.push(escaped),
          None => current.push('\\'),
        }
      }
      '%' => {
        has_token = true;
        match chars.next() {
          None => return Err(ExecError::TrailingPercent),
          Some('%') => current.push('%'),
          Some(code) if is_removable_field_code(code) => {
            // A field code that *is* the whole argument removes that
            // argument. If it follows literal text (`--flag=%d`) the
            // remaining text stays as its own argument.
            if current.is_empty() {
              has_token = false;
            }
          }
          Some(code) => return Err(ExecError::UnknownFieldCode(code)),
        }
      }
      other => {
        has_token = true;
        current.push(other);
      }
    }
    if current.chars().count() > MAX_ARG_CHARS {
      return Err(ExecError::ArgumentTooLong);
    }
  }

  if has_token {
    push_arg(&mut args, current)?;
  }
  if args.first().is_none_or(|arg| arg.is_empty()) {
    return Err(ExecError::Empty);
  }
  Ok(args)
}

fn push_arg(args: &mut Vec<String>, arg: String) -> Result<(), ExecError> {
  if args.len() >= MAX_ARGS {
    return Err(ExecError::TooManyArguments);
  }
  args.push(arg);
  Ok(())
}

fn is_removable_field_code(code: char) -> bool {
  matches!(
    code,
    'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'i' | 'c' | 'k' | 'v' | 'm'
  )
}

#[cfg(test)]
mod tests {
  use super::{ExecError, parse_exec};

  fn argv(input: &str) -> Vec<String> {
    parse_exec(input).unwrap()
  }

  #[test]
  fn splits_plain_arguments() {
    assert_eq!(
      argv("startplasma-wayland --x"),
      ["startplasma-wayland", "--x"]
    );
  }

  #[test]
  fn respects_quotes_and_escapes() {
    assert_eq!(
      argv(r#"cmd "arg with spaces" 'single quoted' "quote\"inside" plain\ space"#),
      [
        "cmd",
        "arg with spaces",
        "single quoted",
        "quote\"inside",
        "plain space"
      ]
    );
  }

  #[test]
  fn removes_field_codes_and_unquotes_percent() {
    assert_eq!(
      argv("app %f %U --flag=%d 100%%"),
      ["app", "--flag=", "100%"]
    );
  }

  #[test]
  fn keeps_empty_quoted_arguments_but_rejects_an_empty_program() {
    assert_eq!(argv(r#"prog "" tail"#), ["prog", "", "tail"]);
    assert_eq!(parse_exec("   "), Err(ExecError::Empty));
    assert_eq!(parse_exec("''"), Err(ExecError::Empty));
  }

  #[test]
  fn reports_unterminated_quotes_and_unknown_codes() {
    assert_eq!(parse_exec("app \"oops"), Err(ExecError::UnterminatedQuote));
    assert_eq!(parse_exec("app %Z"), Err(ExecError::UnknownFieldCode('Z')));
    assert_eq!(parse_exec("app %"), Err(ExecError::TrailingPercent));
  }

  #[test]
  fn bounds_input_size() {
    assert_eq!(
      parse_exec(&"x".repeat(32 * 1024)),
      Err(ExecError::CommandTooLong)
    );
  }
}
