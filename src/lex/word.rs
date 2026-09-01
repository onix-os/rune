//! The pieces a word is made of.
//!
//! A word is not a token. `pre"$mid"post` is one word built from five pieces, and what holds it
//! together is nothing more than the pieces being adjacent — no whitespace and no operator between
//! them. The lexer emits the pieces and `SyntaxKind::is_word_piece` says which ones can be joined;
//! the parser does the joining.

use super::{Lexer, Mode, operator};
use crate::tree::SyntaxKind;

/// Whether a character can sit inside a run of plain text.
///
/// `}` breaks a run so that a `${...}` can end on one. Outside a parameter expansion that only
/// means `a}b` arrives as two pieces of the same word, which is a distinction without a difference.
///
/// `]` is not in here, because it is not always a terminator: `]]` is a single reserved word, and
/// splitting it would leave the parser unable to see the end of a `[[ ... ]]`. Inside `${...}` it
/// does end a run, and [`Lexer::plain_text`] adds it there.
fn is_plain(ch: char) -> bool {
    !matches!(
        ch,
        '\'' | '"' | '\\' | '$' | '`' | '}' | '\n' | ' ' | '\t' | '\r'
    ) && !operator::starts_one(ch)
}

impl Lexer<'_> {
    pub(super) fn word_piece(&mut self) -> SyntaxKind {
        match self.cursor.peek() {
            Some('\'') => self.single_quoted(),
            Some('"') => {
                self.cursor.bump();
                self.push_mode(Mode::DoubleQuoted);
                SyntaxKind::DoubleQuote
            }
            Some('\\') => self.escaped(),
            Some('`') => self.backtick(),
            Some('$') => self.dollar(),
            Some('~') if self.at_word_start => self.tilde(),
            _ => self.plain_text(),
        }
    }

    /// A piece inside `"..."`, where the only things left with any meaning are `$`, `` ` ``, some
    /// escapes, and the closing quote.
    pub(super) fn quoted_piece(&mut self) -> SyntaxKind {
        match self.cursor.peek() {
            Some('"') => {
                self.cursor.bump();
                self.pop_mode();
                SyntaxKind::DoubleQuote
            }
            Some('$') => self.dollar(),
            Some('`') => self.backtick(),
            // Inside double quotes a backslash escapes only these. Before anything else it is an
            // ordinary character, which is why `"\d"` is a backslash and a d.
            Some('\\') if matches!(self.cursor.peek_at(1), Some('$' | '`' | '"' | '\\' | '\n')) => {
                self.escaped()
            }
            _ => {
                self.cursor.bump();
                self.cursor
                    .eat_while(|ch| !matches!(ch, '"' | '$' | '`' | '\\'));
                SyntaxKind::Text
            }
        }
    }

    /// Either end of a `` `...` ``, which one being whichever it has to be.
    ///
    /// Like `$(...)`, the inside is ordinary shell even when the outside is quoted, so this opens
    /// a mode rather than emitting a character and carrying on.
    fn backtick(&mut self) -> SyntaxKind {
        self.cursor.bump();
        if self.mode() == Mode::Backtick {
            self.pop_mode();
        } else {
            self.push_mode(Mode::Backtick);
        }
        SyntaxKind::Backtick
    }

    /// `'...'`, which has no interior structure at all — not even a backslash escape.
    fn single_quoted(&mut self) -> SyntaxKind {
        self.cursor.bump();
        self.cursor.eat_while(|ch| ch != '\'');
        // An unterminated quote runs to the end of the file. The parser is what complains.
        self.cursor.eat_char('\'');
        SyntaxKind::SingleQuoted
    }

    /// A backslash and whatever it protects. A trailing backslash protects nothing.
    fn escaped(&mut self) -> SyntaxKind {
        self.cursor.bump();
        self.cursor.bump();
        SyntaxKind::Escaped
    }

    /// `~`, `~user`, or `~+`/`~-`, up to the first `/`.
    fn tilde(&mut self) -> SyntaxKind {
        self.cursor.bump();
        self.cursor
            .eat_while(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '+' | '.'));
        SyntaxKind::Tilde
    }

    fn plain_text(&mut self) -> SyntaxKind {
        let in_brace = matches!(self.mode(), Mode::Brace { .. });
        self.cursor.bump();
        self.cursor
            .eat_while(|ch| is_plain(ch) && !(in_brace && ch == ']'));
        SyntaxKind::Text
    }
}
