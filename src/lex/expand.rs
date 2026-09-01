//! Everything that begins with `$`.
//!
//! Three of the four forms need the lexer to change how it reads: `${...}` has a small language of
//! its own, `$((...))` is arithmetic rather than shell, and `$(...)` is shell again and so needs
//! nothing at all — its closing `)` is an ordinary operator and the parser is what matches it up.

use super::{BraceStage, Lexer, Mode};
use crate::tree::SyntaxKind;

/// The parameters whose names are punctuation, and so end after one character.
const SPECIAL: &str = "?$!#*@-_0123456789";

/// The characters that make up an operator inside `${...}`.
const PARAM_OP: &str = ":-=?+#%/^,";

fn is_name_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_name(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

impl Lexer<'_> {
    pub(super) fn dollar(&mut self) -> SyntaxKind {
        self.cursor.bump();
        match self.cursor.peek() {
            Some('(') if self.cursor.peek_at(1) == Some('(') => {
                self.cursor.eat("((");
                self.push_mode(Mode::Arithmetic { depth: 0 });
                SyntaxKind::DollarParenParen
            }
            Some('(') => {
                self.cursor.bump();
                SyntaxKind::DollarParen
            }
            Some('{') => {
                self.cursor.bump();
                self.push_mode(Mode::Brace {
                    stage: BraceStage::Start,
                });
                SyntaxKind::DollarBrace
            }
            Some(ch) if is_name_start(ch) => {
                self.cursor.eat_while(is_name);
                SyntaxKind::DollarName
            }
            Some(ch) if SPECIAL.contains(ch) => {
                self.cursor.bump();
                SyntaxKind::DollarSpecial
            }
            // A `$` before anything else is just a dollar sign: `echo 5$` prints `5$`.
            _ => SyntaxKind::Dollar,
        }
    }

    /// A piece inside `${...}`.
    pub(super) fn brace_piece(&mut self, stage: BraceStage) -> SyntaxKind {
        if self.cursor.eat_char('}') {
            self.pop_mode();
            return SyntaxKind::RBrace;
        }
        match self.cursor.peek() {
            // `${#name}` and `${!name}` — an operator before the name rather than after it.
            Some('#' | '!') if stage == BraceStage::Start => {
                self.cursor.bump();
                SyntaxKind::ParamOp
            }
            Some('[') if stage == BraceStage::Name => {
                self.cursor.bump();
                SyntaxKind::LBracket
            }
            Some(']') if stage == BraceStage::Name => {
                self.cursor.bump();
                SyntaxKind::RBracket
            }
            Some(ch) if stage != BraceStage::Operand && PARAM_OP.contains(ch) => {
                self.cursor.eat_while(|ch| PARAM_OP.contains(ch));
                self.set_brace_stage(BraceStage::Operand);
                SyntaxKind::ParamOp
            }
            Some(ch) if stage != BraceStage::Operand && is_name(ch) => {
                self.cursor.eat_while(is_name);
                self.set_brace_stage(BraceStage::Name);
                SyntaxKind::Text
            }
            // Past the operator the rest is an ordinary word, and can hold anything a word can.
            _ => self.word_piece(),
        }
    }

    /// A run of arithmetic, up to the `)` that begins the closing `))`.
    pub(super) fn arithmetic_run(&mut self) -> SyntaxKind {
        while let Some(ch) = self.cursor.peek() {
            match ch {
                ')' if self.arith_depth() == 0 => break,
                ')' => {
                    self.bump_arith_depth(-1);
                    self.cursor.bump();
                }
                '(' => {
                    self.bump_arith_depth(1);
                    self.cursor.bump();
                }
                '\\' => {
                    self.cursor.bump();
                    self.cursor.bump();
                }
                _ => {
                    self.cursor.bump();
                }
            }
        }
        SyntaxKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lex;

    fn kinds(text: &str) -> Vec<SyntaxKind> {
        lex(text).into_iter().map(|token| token.kind).collect()
    }

    #[test]
    fn names_and_specials_are_told_apart() {
        assert_eq!(kinds("$HOME"), [SyntaxKind::DollarName]);
        assert_eq!(kinds("$?"), [SyntaxKind::DollarSpecial]);
        assert_eq!(kinds("$1"), [SyntaxKind::DollarSpecial]);
        assert_eq!(kinds("$$"), [SyntaxKind::DollarSpecial]);
    }

    #[test]
    fn a_dollar_before_nothing_special_is_a_dollar() {
        assert_eq!(kinds("$"), [SyntaxKind::Dollar]);
        assert_eq!(kinds("$."), [SyntaxKind::Dollar, SyntaxKind::Text]);
    }

    #[test]
    fn a_name_stops_where_it_stops() {
        assert_eq!(kinds("$a-b"), [SyntaxKind::DollarName, SyntaxKind::Text]);
    }
}
