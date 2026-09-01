//! The control and redirection operators.
//!
//! `((` and `))` are missing on purpose. Whether `((` opens an arithmetic command or two subshells
//! depends on whether a command could start here, which is the parser's question; it sees two
//! adjacent `(` and decides. Braces are missing for the same reason — `{` is a reserved word, and
//! `a{b,c}` is one ordinary word containing two of them.

use super::Lexer;
use crate::tree::SyntaxKind;

/// Longest match first: `<<-` must be tried before `<<`, and `<<` before `<`.
const OPERATORS: &[(&str, SyntaxKind)] = &[
    (";;&", SyntaxKind::SemiSemiAmp),
    ("<<-", SyntaxKind::LessLessDash),
    ("<<<", SyntaxKind::LessLessLess),
    ("&>>", SyntaxKind::AmpGreatGreat),
    (";;", SyntaxKind::SemiSemi),
    (";&", SyntaxKind::SemiAmp),
    ("&&", SyntaxKind::AndAnd),
    ("||", SyntaxKind::PipePipe),
    ("|&", SyntaxKind::PipeAmp),
    (">>", SyntaxKind::GreatGreat),
    ("<<", SyntaxKind::LessLess),
    ("<>", SyntaxKind::LessGreat),
    ("<&", SyntaxKind::LessAmp),
    (">&", SyntaxKind::GreatAmp),
    (">|", SyntaxKind::GreatPipe),
    ("&>", SyntaxKind::AmpGreat),
    (";", SyntaxKind::Semi),
    ("&", SyntaxKind::Amp),
    ("|", SyntaxKind::Pipe),
    ("<", SyntaxKind::Less),
    (">", SyntaxKind::Great),
    ("(", SyntaxKind::LParen),
    (")", SyntaxKind::RParen),
];

/// Whether a character could begin an operator, and so ends the word before it.
pub(super) const fn starts_one(ch: char) -> bool {
    matches!(ch, ';' | '&' | '|' | '<' | '>' | '(' | ')')
}

impl Lexer<'_> {
    pub(super) fn operator(&mut self) -> SyntaxKind {
        // `<(cmd)` is a word, not a redirection: it expands to the name of a pipe, and `diff <(a)
        // <(b)` passes `diff` two ordinary arguments. The parenthesis has to be touching — with a
        // space, `<` is a redirection and bash rejects what follows.
        if self.cursor.eat("<(") {
            return SyntaxKind::ProcSubIn;
        }
        if self.cursor.eat(">(") {
            return SyntaxKind::ProcSubOut;
        }
        for (text, kind) in OPERATORS {
            if self.cursor.eat(text) {
                return *kind;
            }
        }
        self.cursor.bump();
        SyntaxKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operator_is_reachable_by_its_first_character() {
        for (text, _) in OPERATORS {
            let first = text.chars().next().expect("no operator is empty");
            assert!(starts_one(first), "{text} cannot be reached");
        }
    }

    #[test]
    fn longer_operators_come_before_their_prefixes() {
        for (index, (text, _)) in OPERATORS.iter().enumerate() {
            for (earlier, _) in &OPERATORS[..index] {
                assert!(
                    !text.starts_with(earlier),
                    "{text} is unreachable: {earlier} matches first"
                );
            }
        }
    }
}
