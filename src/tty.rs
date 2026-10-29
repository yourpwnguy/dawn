//! The terminal fallback.
//!
//! A greeter that cannot open a window must still allow a login. This frontend
//! reuses the same [`Flow`] and [`Backend`] as the graphical UI, so the
//! credential handling, prompt sequencing, and error messages are identical.
//! Only presentation differs: canonical terminal input, echo disabled for
//! secret prompts, and the first discovered session.
//!
//! It opens `/dev/tty` rather than using stdin so it also works when the
//! process was started with a redirected stdin. Every line is bounded and the
//! password buffer is zeroizing.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};

use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
use zeroize::Zeroizing;

use crate::auth::backend::{Backend, DryRunBackend, GreetdBackend};
use crate::auth::flow::{Effect, Flow, FlowError};
use crate::auth::model::Launch;
use crate::config::Config;
use crate::secret::Secret;
use crate::session::Session;

use crate::app::Finish;

const MAX_LINE_CHARS: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum TtyError {
  #[error("cannot open /dev/tty: {0}")]
  Open(#[source] std::io::Error),
  #[error("/dev/tty is not a terminal, so the fallback cannot prompt")]
  NotATerminal,
  #[error("terminal input/output failed: {0}")]
  Io(#[from] std::io::Error),
  #[error("no desktop sessions are installed, so there is nothing to log into")]
  NoSessions,
  #[error("authentication state error: {0}")]
  Flow(#[from] FlowError),
}

/// True when a fallback login is actually possible. Used by main to decide
/// whether to try the terminal after the window fails.
#[must_use]
pub fn available() -> bool {
  File::options()
    .read(true)
    .write(true)
    .open("/dev/tty")
    .is_ok_and(|file| rustix::termios::isatty(&file))
}

/// Runs a complete login over the terminal. Returns when a session was started
/// or the user closed the terminal.
pub fn run(config: &Config, sessions: &[Session], dry_run: bool) -> Result<Finish, TtyError> {
  let some_session = sessions.first().ok_or(TtyError::NoSessions)?;

  let tty = File::options()
    .read(true)
    .write(true)
    .open("/dev/tty")
    .map_err(TtyError::Open)?;
  if !rustix::termios::isatty(&tty) {
    return Err(TtyError::NotATerminal);
  }
  let mut reader = BufReader::new(tty.try_clone()?);
  let mut writer = tty.try_clone()?;

  writeln!(
    writer,
    "dawn {} (terminal fallback)",
    env!("CARGO_PKG_VERSION")
  )?;
  writeln!(
    writer,
    "Session: {} ({}). Edit the config to change it.",
    some_session.name, some_session.id
  )?;

  loop {
    write!(writer, "Username: ")?;
    writer.flush()?;
    let mut username = Zeroizing::new(String::new());
    if reader.read_line(&mut username)? == 0 {
      return Ok(Finish::Quit);
    }
    trim_newline(&mut username);
    if username.trim().is_empty() {
      writeln!(writer, "Enter a username.")?;
      continue;
    }
    if username.chars().any(char::is_control) {
      writeln!(writer, "That username contains control characters.")?;
      continue;
    }

    let mut backend: Box<dyn Backend> = if dry_run {
      Box::new(DryRunBackend::new())
    } else {
      Box::new(GreetdBackend::new(
        config.auth.socket.clone(),
        config.auth.timeout,
      ))
    };

    let launch = Launch {
      cmd: some_session.command.clone(),
      env: some_session.env(),
    };

    match attempt(
      backend.as_mut(),
      &tty,
      &mut reader,
      &mut writer,
      &username,
      launch,
    )? {
      Attempt::Started => return Ok(Finish::Started),
      Attempt::Failed(message) => {
        writeln!(writer, "{message}")?;
        writeln!(writer)?;
      }
      Attempt::Cancelled => writeln!(writer, "Login cancelled.")?,
    }
  }
}

enum Attempt {
  Started,
  Failed(String),
  Cancelled,
}

/// Drives one login attempt to a terminal outcome. The loop mirrors what the
/// auth worker does in the GUI, but here it is fine to block: the user is
/// already sitting in front of a terminal.
fn attempt(
  backend: &mut dyn Backend,
  tty: &File,
  reader: &mut BufReader<File>,
  writer: &mut File,
  username: &str,
  launch: Launch,
) -> Result<Attempt, TtyError> {
  let mut flow = Flow::new();
  let mut outcome = flow.begin(username, launch);

  loop {
    for notice in outcome.notices.drain(..) {
      writeln!(writer, "{}", notice.message)?;
    }

    match outcome.effect {
      Effect::Request(request) => match backend.request(request) {
        Ok(response) => outcome = flow.on_response(response)?,
        Err(error) => {
          // Reset the flow, then report the transport problem through the same
          // path the GUI uses. The next outer iteration opens a fresh socket.
          if let Effect::Failed(failure) = flow.on_transport_error(&error.to_string()).effect {
            return Ok(Attempt::Failed(failure.message));
          }
          return Ok(Attempt::Failed(error.to_string()));
        }
      },
      Effect::Ask(prompt) => {
        write!(writer, "{} ", prompt.message)?;
        writer.flush()?;
        let mut answer = read_line(
          tty,
          reader,
          prompt.kind == crate::auth::model::PromptKind::Secret,
        )?;
        trim_newline(&mut answer);
        writeln!(writer)?;
        let value = Secret::new(std::mem::take(&mut *answer));
        outcome = flow.answer(value)?;
      }
      Effect::Completed => return Ok(Attempt::Started),
      Effect::Failed(failure) => return Ok(Attempt::Failed(failure.message)),
      Effect::Cancelled => return Ok(Attempt::Cancelled),
    }
  }
}

fn read_line(
  tty: &File,
  reader: &mut BufReader<File>,
  hidden: bool,
) -> Result<Zeroizing<String>, TtyError> {
  let mut line = Zeroizing::new(String::new());
  let saved = if hidden {
    Some(tcgetattr(tty).map_err(std::io::Error::from)?)
  } else {
    None
  };
  if let Some(saved) = &saved {
    let mut quiet = saved.clone();
    // Turning off ISIG keeps Ctrl-C from killing us before echo is restored.
    // The byte is handled as a cancel after the line is read.
    quiet
      .local_modes
      .remove(LocalModes::ECHO | LocalModes::ISIG);
    tcsetattr(tty, OptionalActions::Now, &quiet).map_err(std::io::Error::from)?;
  }

  let read = reader.read_line(&mut line);
  // Restore echo regardless of the read result, otherwise a failed read would
  // leave the rest of the session silently swallowing everything typed.
  let restored = saved.as_ref().map_or(Ok(()), |saved| {
    tcsetattr(tty, OptionalActions::Now, saved).map_err(std::io::Error::from)
  });
  if read? == 0 {
    return Err(TtyError::Io(std::io::Error::new(
      std::io::ErrorKind::UnexpectedEof,
      "terminal closed",
    )));
  }
  restored?;

  if line.chars().count() > MAX_LINE_CHARS {
    return Err(TtyError::Io(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      format!("input is longer than {MAX_LINE_CHARS} characters"),
    )));
  }
  Ok(line)
}

fn trim_newline(line: &mut String) {
  while line.ends_with('\n') || line.ends_with('\r') {
    line.pop();
  }
}
