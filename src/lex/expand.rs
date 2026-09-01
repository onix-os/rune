//! Everything that begins with `$`.
//!
//! Three of the four forms need the lexer to change how it reads: `${...}` has a small language of
//! its own, `$((...))` is arithmetic rather than shell, and `$(...)` is shell again and so needs
//! nothing at all — its closing `)` is an ordinary operator and the parser is what matches it up.

use super::{BraceStage, Lexer, Mode};
use crate::tree::SyntaxKind;

/// The parameters whose names are punctuation, and so end after one character.
const SPECIAL: &str = "?$!#*@-_0123456789";

/// The operators that can follow a parameter name, longest match first.
///
/// A run of the characters would not do: the operator in `${x:-/tmp}` is `:-`, and `/tmp` is the
/// default value, not more operator.
const PARAM_OPS: &[&str] = &[
    ":-", ":=", ":?", ":+", "##", "%%", "//", "/#", "/%", "^^", ",,", ":", "-", "=", "?", "+", "#",
    "%", "/", "^", ",",
];

/// Whether a character could begin one of [`PARAM_OPS`].
fn starts_param_op(ch: char) -> bool {
    matches!(
        ch,
        ':' | '-' | '=' | '?' | '+' | '#' | '%' | '/' | '^' | ','
    )
}

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
                self.push_mode(Mode::CommandSub { depth: 0 });
                SyntaxKind::DollarParen
            }
            // `$'...'` interprets backslash escapes, so `\'` does not end it. Inside double
            // quotes it is inert — `"$'a'"` is a dollar sign and a quoted `a` — which is why this
            // asks where it is.
            Some('\'') if self.mode() != Mode::DoubleQuoted => self.ansi_c_quoted(),
            // `$"..."` is a double-quoted string that gets translated. For reading it, the
            // translation changes nothing.
            Some('"') => {
                self.cursor.bump();
                self.push_mode(Mode::DoubleQuoted);
                SyntaxKind::DoubleQuote
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

    /// `$'...'`, whose backslash escapes protect the quote that would otherwise end it.
    fn ansi_c_quoted(&mut self) -> SyntaxKind {
        self.cursor.bump();
        loop {
            match self.cursor.peek() {
                None => {
                    self.note_unclosed("$'", self.token_start());
                    break;
                }
                Some('\\') => {
                    self.cursor.bump();
                    self.cursor.bump();
                }
                Some('\'') => {
                    self.cursor.bump();
                    break;
                }
                _ => {
                    self.cursor.bump();
                }
            }
        }
        SyntaxKind::AnsiCQuoted
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
                self.set_brace_stage(BraceStage::Subscript);
                SyntaxKind::LBracket
            }
            Some(']') if stage == BraceStage::Subscript => {
                self.cursor.bump();
                self.set_brace_stage(BraceStage::Name);
                SyntaxKind::RBracket
            }
            Some(ch)
                if matches!(stage, BraceStage::Start | BraceStage::Name) && starts_param_op(ch) =>
            {
                for operator in PARAM_OPS {
                    if self.cursor.eat(operator) {
                        break;
                    }
                }
                self.set_brace_stage(BraceStage::Operand);
                SyntaxKind::ParamOp
            }
            Some(ch) if stage != BraceStage::Operand && is_name(ch) => {
                self.cursor.eat_while(is_name);
                if stage != BraceStage::Subscript {
                    self.set_brace_stage(BraceStage::Name);
                }
                SyntaxKind::Text
            }
            // Past the operator the rest is an ordinary word, and can hold anything a word can.
            _ => self.word_piece(),
        }
    }

    /// The `))` that ends an arithmetic expansion, as one token.
    ///
    /// A lone `)` here means the expansion was never finished properly; it is taken as the close
    /// anyway, and the parser is what says so.
    pub(super) fn close_arithmetic(&mut self) -> SyntaxKind {
        self.pop_mode();
        self.cursor.bump();
        if self.cursor.eat_char(')') {
            SyntaxKind::RParenRParen
        } else {
            SyntaxKind::RParen
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
        lex(text)
            .tokens
            .into_iter()
            .map(|token| token.kind)
            .collect()
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

    #[test]
    fn every_parameter_operator_is_reachable() {
        for operator in PARAM_OPS {
            let first = operator.chars().next().expect("no operator is empty");
            assert!(starts_param_op(first), "{operator} cannot be reached");
        }
        for (index, operator) in PARAM_OPS.iter().enumerate() {
            for earlier in &PARAM_OPS[..index] {
                assert!(
                    !operator.starts_with(earlier),
                    "{operator} is unreachable: {earlier} matches first"
                );
            }
        }
    }
}
