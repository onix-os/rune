//! Inside `[[ ... ]]`, where two things are read differently from ordinary shell.
//!
//! The operand of `=~` is a regular expression, and a group in it belongs to the regex: in
//! `[[ $s =~ (^| )$c( |$) ]]` the spaces and the `|` inside the parentheses are part of the one
//! operand. bash reads a parenthesised group there as a matched pair, so everything up to the
//! matching `)` is literal — blanks, `;`, `&`, `<`, `#` and newlines included. Outside a group the
//! word ends where any word ends, except at `|`, which is the regex's.
//!
//! And `!(` at the start of a test is negation and a group, not an `extglob` pattern, because a
//! test term is not a word that could be one: `[[ !(a == b) ]]` negates the comparison.

use super::{Lexer, Mode};
use crate::tree::SyntaxKind;

/// What the lexer remembers about one `[[ ... ]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CondState {
    /// How many of the test's own `(` are open.
    depth: i32,
    /// Whether the next word is the operand of `=~`.
    regex: bool,
    /// Whether the next word begins a test term: after `[[`, `(`, `!`, `&&` and `||`.
    term: bool,
}

/// Where a regex operand ends outside a group: at a blank, an operator or a newline.
fn ends_regex(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\n' | ';' | '&' | '<' | '>' | ')')
}

impl Lexer<'_> {
    /// Enter `[[`, which the token just read opened.
    pub(super) fn open_test(&mut self) {
        self.push_mode(Mode::Cond(CondState {
            depth: 0,
            regex: false,
            term: true,
        }));
    }

    /// Whether the word about to be read begins a test term, so `!(` there is not a pattern.
    pub(super) fn at_test_term(&self) -> bool {
        matches!(self.mode(), Mode::Cond(state) if state.term)
    }

    fn set_cond(&mut self, state: CondState) {
        if let Some((Mode::Cond(current), _)) = self.modes.last_mut() {
            *current = state;
        }
    }

    /// Whether a `)` here closes a substitution the test sits in, because the test has none open.
    fn closes_enclosing(&self, state: CondState) -> bool {
        state.depth == 0
            && self
                .modes
                .iter()
                .rev()
                .skip(1)
                .find(|(mode, _)| !matches!(mode, Mode::Cond(_)))
                .is_some_and(|(mode, _)| matches!(mode, Mode::CommandSub { .. }))
    }

    /// One token of a test.
    pub(super) fn cond_token(&mut self, mut state: CondState) -> SyntaxKind {
        // A `[[` never closed inside `$( … )` must not take the substitution's `)` with it.
        if self.cursor.peek() == Some(')') && self.closes_enclosing(state) {
            self.pop_mode();
            return self.token();
        }
        if state.regex
            && !self
                .cursor
                .peek()
                .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r'))
        {
            state.regex = false;
            self.set_cond(state);
            // A `#` there starts a comment, as it does wherever a word could start.
            let begins = self
                .cursor
                .peek()
                .is_some_and(|ch| !ends_regex(ch) && ch != '#');
            let closes = self.cursor.starts_with("]]") && {
                let rest = self.cursor.rest().get(2..).unwrap_or("");
                rest.chars()
                    .next()
                    .is_none_or(|ch| ends_regex(ch) || ch == '|')
            };
            if begins && !closes {
                self.push_mode(Mode::Regex { depth: 0 });
                return self.regex_piece(0);
            }
        }

        let word_start = self.at_word_start;
        let kind = self.normal_token();
        let text = self.token_text();
        self.note_position(kind, text, word_start);
        match kind {
            SyntaxKind::LParen => {
                state.depth += 1;
                state.term = true;
            }
            SyntaxKind::RParen => {
                state.depth = (state.depth - 1).max(0);
                state.term = false;
            }
            SyntaxKind::AndAnd | SyntaxKind::PipePipe => state.term = true,
            SyntaxKind::Text if word_start && self.word_ends_here() => match text {
                "]]" => {
                    self.pop_mode();
                    return kind;
                }
                "=~" => {
                    state.regex = true;
                    state.term = false;
                }
                "!" => state.term = true,
                _ => state.term = false,
            },
            _ if kind.is_word_piece() && word_start => state.term = false,
            _ => {}
        }
        self.set_cond(state);
        kind
    }

    /// A piece of a regex operand: a run of its text, or a quote or expansion inside it.
    pub(super) fn regex_piece(&mut self, depth: i32) -> SyntaxKind {
        match self.cursor.peek() {
            Some('\'' | '"' | '\\' | '`' | '$') => self.word_piece(),
            Some(ch) if depth == 0 && ends_regex(ch) => {
                self.pop_mode();
                self.token()
            }
            _ => {
                let mut depth = depth;
                while let Some(ch) = self.cursor.peek() {
                    match ch {
                        '\'' | '"' | '\\' | '`' | '$' => break,
                        _ if depth == 0 && ends_regex(ch) => break,
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    self.cursor.bump();
                }
                self.set_depth(depth);
                SyntaxKind::Text
            }
        }
    }
}
