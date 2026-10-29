//! A small single-line text field with a cursor.
//!
//! This is not a general text editor. It only supports what a login form
//! needs: insert, backspace, delete, home/end, and left/right. The underlying
//! buffer is always a [`Zeroizing<String>`] so clearing the password field (or
//! dropping the input) wipes the bytes. Cursor movement is by chars, never by
//! bytes, so multi-byte usernames and passwords edit correctly.

use zeroize::{Zeroize, Zeroizing};

use crate::secret::Secret;

pub struct TextInput {
  text: Zeroizing<String>,
  cursor: usize,
  max_chars: usize,
}

impl TextInput {
  #[must_use]
  pub fn new(max_chars: usize) -> Self {
    Self {
      text: Zeroizing::new(String::new()),
      cursor: 0,
      max_chars,
    }
  }

  #[must_use]
  pub fn value(&self) -> &str {
    &self.text
  }

  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.text.is_empty()
  }

  #[must_use]
  pub fn cursor(&self) -> usize {
    self.cursor
  }

  /// The text before the cursor. Used to place the caret.
  #[must_use]
  pub fn cursor_prefix(&self) -> &str {
    self.text.get(..self.cursor).unwrap_or("")
  }

  pub fn insert(&mut self, ch: char) {
    if ch.is_control() || self.char_count() >= self.max_chars {
      return;
    }
    self.text.insert(self.cursor, ch);
    self.cursor += ch.len_utf8();
  }

  pub fn backspace(&mut self) {
    let Some((previous, _)) = self
      .text
      .get(..self.cursor)
      .and_then(|prefix| prefix.char_indices().next_back())
    else {
      return;
    };
    self.text.drain(previous..self.cursor);
    self.cursor = previous;
  }

  pub fn delete(&mut self) {
    let Some(ch) = self
      .text
      .get(self.cursor..)
      .and_then(|rest| rest.chars().next())
    else {
      return;
    };
    let end = self.cursor + ch.len_utf8();
    self.text.drain(self.cursor..end);
  }

  pub fn move_left(&mut self) {
    if let Some((previous, _)) = self
      .text
      .get(..self.cursor)
      .and_then(|prefix| prefix.char_indices().next_back())
    {
      self.cursor = previous;
    }
  }

  pub fn move_right(&mut self) {
    if let Some(ch) = self
      .text
      .get(self.cursor..)
      .and_then(|rest| rest.chars().next())
    {
      self.cursor += ch.len_utf8();
    }
  }

  pub fn move_home(&mut self) {
    self.cursor = 0;
  }

  pub fn move_end(&mut self) {
    self.cursor = self.text.len();
  }

  /// Wipes the buffer. `Zeroize` on `String` overwrites the bytes before the
  /// length is reset, so a `clear()` alone would not be enough.
  pub fn clear(&mut self) {
    self.text.zeroize();
    self.text.clear();
    self.cursor = 0;
  }

  /// Moves the value into a [`Secret`] and leaves the field empty. Used for
  /// password submission; the zeroizing buffer moves with the value.
  pub fn take_secret(&mut self) -> Secret {
    let value = std::mem::take(&mut *self.text);
    self.cursor = 0;
    Secret::new(value)
  }

  /// Renders the value as one bullet per character. The caller draws this, not
  /// the real value, so a password never reaches the rasteriser as text.
  #[must_use]
  pub fn masked(&self) -> String {
    self.masked_with("•")
  }

  /// Masked text using the configured glyph. A multi-character mask uses its
  /// first character so the count still matches the password length.
  #[must_use]
  pub fn masked_with(&self, mask: &str) -> String {
    let glyph = mask.chars().next().unwrap_or('•');
    std::iter::repeat_n(glyph, self.char_count()).collect()
  }

  /// Masked equivalent of [`Self::cursor_prefix`], for caret placement.
  #[must_use]
  pub fn masked_cursor_prefix(&self) -> String {
    self.masked_cursor_prefix_with("•")
  }

  /// Masked cursor prefix using the configured glyph.
  #[must_use]
  pub fn masked_cursor_prefix_with(&self, mask: &str) -> String {
    let glyph = mask.chars().next().unwrap_or('•');
    std::iter::repeat_n(
      glyph,
      self
        .text
        .get(..self.cursor)
        .map_or(0, |prefix| prefix.chars().count()),
    )
    .collect()
  }

  fn char_count(&self) -> usize {
    self.text.chars().count()
  }
}

#[cfg(test)]
mod tests {
  use super::TextInput;

  #[test]
  fn edits_at_char_boundaries() {
    let mut input = TextInput::new(32);
    for ch in "héllo".chars() {
      input.insert(ch);
    }
    input.move_left();
    input.insert('!');
    assert_eq!(input.value(), "héll!o");
    input.backspace();
    assert_eq!(input.value(), "héllo");
    input.move_home();
    input.delete();
    assert_eq!(input.value(), "éllo");
    input.move_end();
    input.backspace();
    assert_eq!(input.value(), "éll");
  }

  #[test]
  fn bounds_input_length() {
    let mut input = TextInput::new(2);
    for ch in "abc".chars() {
      input.insert(ch);
    }
    assert_eq!(input.value(), "ab");
  }

  #[test]
  fn rejects_control_characters() {
    let mut input = TextInput::new(8);
    input.insert('\n');
    input.insert('\t');
    input.insert('x');
    assert_eq!(input.value(), "x");
  }

  #[test]
  fn masks_and_wipes_secrets() {
    let mut input = TextInput::new(8);
    input.insert('p');
    input.insert('w');
    assert_eq!(input.masked(), "••");
    let secret = input.take_secret();
    assert!(input.is_empty());
    assert_eq!(secret.expose(), "pw");
    input.insert('x');
    input.clear();
    assert!(input.is_empty());
    assert_eq!(input.cursor(), 0);
  }

  #[test]
  fn moving_past_the_edges_is_a_no_op() {
    let mut input = TextInput::new(8);
    input.move_left();
    input.move_right();
    input.delete();
    input.backspace();
    assert_eq!(input.value(), "");
    input.insert('a');
    input.move_end();
    input.move_right();
    assert_eq!(input.cursor(), 1);
  }
}
