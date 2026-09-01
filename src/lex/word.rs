//! The pieces a word is made of.
//!
//! A word is not a token. `pre"$mid"post` is one word built from five pieces, and what holds it
//! together is nothing more than the pieces being adjacent — no whitespace and no operator between
//! them. The lexer emits the pieces and [`continues_a_word`] says which ones can be joined; the
//! parser does the joining.

use super::{Lexer, Mode, operator};
use crate::tree::SyntaxKind;

/// Whether a token of this kind, placed next to another, keeps one word going.
pub(super) const fn continues_a_word(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Text
            | SyntaxKind::Escaped
            | SyntaxKind::SingleQuoted
            | SyntaxKind::DoubleQuote
            | SyntaxKind::Dollar
            | SyntaxKind::DollarName
            | SyntaxKind::DollarSpecial
            | SyntaxKind::DollarBrace
            | SyntaxKind::DollarParen
            | SyntaxKind::DollarParenParen
            | SyntaxKind::Backtick
            | SyntaxKind::Tilde
            | SyntaxKind::ParamOp
            | SyntaxKind::LBracket
            | SyntaxKind::RBracket
            | SyntaxKind::RBrace
    )
}

/// Whether a character can sit inside a run of plain text.
///
/// `}` and `]` break a run so that a `${...}` and a `${a[i]}` can end on one. Outside a parameter
/// expansion that only means `a}b` arrives as two pieces of the same word, which is a distinction
/// without a difference.
fn is_plain(ch: char) -> bool {
    !matches!(
        ch,
        '\'' | '"' | '\\' | '$' | '`' | '}' | ']' | '\n' | ' ' | '\t' | '\r'
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
            Some('`') => {
                self.cursor.bump();
                SyntaxKind::Backtick
            }
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
            Some('`') => {
                self.cursor.bump();
                SyntaxKind::Backtick
            }
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
        self.cursor.bump();
        self.cursor.eat_while(is_plain);
        SyntaxKind::Text
    }
}
