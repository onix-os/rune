//! A reading head over the source text.
//!
//! Everything is measured in bytes and moved in characters, so an offset is always on a boundary
//! and the lexer never has to think about it.

pub(crate) struct Cursor<'a> {
    text: &'a str,
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) const fn new(text: &'a str) -> Self {
        Self { text, offset: 0 }
    }

    pub(crate) const fn offset(&self) -> u32 {
        self.offset as u32
    }

    pub(crate) fn is_eof(&self) -> bool {
        self.offset >= self.text.len()
    }

    /// What is left, from the cursor on.
    pub(crate) fn rest(&self) -> &'a str {
        self.text.get(self.offset..).unwrap_or("")
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// The character `n` positions ahead, counting the one under the cursor as zero.
    pub(crate) fn peek_at(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    pub(crate) fn starts_with(&self, text: &str) -> bool {
        self.rest().starts_with(text)
    }

    /// Move past one character and report it.
    pub(crate) fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }

    /// Move past `text` if it is next, and say whether it was.
    pub(crate) fn eat(&mut self, text: &str) -> bool {
        if self.starts_with(text) {
            self.offset += text.len();
            return true;
        }
        false
    }

    /// Move past `ch` if it is next, and say whether it was.
    pub(crate) fn eat_char(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.offset += ch.len_utf8();
            return true;
        }
        false
    }

    /// Move past every character the predicate accepts.
    pub(crate) fn eat_while(&mut self, accept: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&accept) {
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_characters_not_bytes() {
        let mut cursor = Cursor::new("éx");
        assert_eq!(cursor.bump(), Some('é'));
        assert_eq!(cursor.offset(), 2);
        assert_eq!(cursor.peek(), Some('x'));
    }

    #[test]
    fn peeks_ahead_by_characters() {
        let cursor = Cursor::new("<<-EOF");
        assert_eq!(cursor.peek_at(0), Some('<'));
        assert_eq!(cursor.peek_at(2), Some('-'));
        assert_eq!(cursor.peek_at(99), None);
    }

    #[test]
    fn eats_only_what_matches() {
        let mut cursor = Cursor::new(">>x");
        assert!(!cursor.eat("<<"));
        assert!(cursor.eat(">>"));
        assert_eq!(cursor.rest(), "x");
    }

    #[test]
    fn runs_off_the_end_without_complaint() {
        let mut cursor = Cursor::new("");
        assert!(cursor.is_eof());
        assert_eq!(cursor.bump(), None);
        assert_eq!(cursor.peek(), None);
        assert_eq!(cursor.rest(), "");
    }

    #[test]
    fn eats_a_run() {
        let mut cursor = Cursor::new("   x");
        cursor.eat_while(|ch| ch == ' ');
        assert_eq!(cursor.offset(), 3);
    }
}
