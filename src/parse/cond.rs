//! The expression inside `[[ ... ]]`.
//!
//! This is a different language from the one around it. `<` and `>` compare strings rather than
//! redirecting, `&&` and `||` join tests rather than commands, and the operands are not split or
//! globbed. Reading it as a run of words would throw away exactly the structure that a caller
//! lowering it to something executable needs, so it gets a grammar.

use super::Parser;
use crate::error::Error;
use crate::span::Span;
use crate::tree::SyntaxKind;

/// The tests that take one operand.
const UNARY: [&str; 24] = [
    "-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-p", "-r", "-s", "-t", "-u", "-w", "-x",
    "-z", "-n", "-o", "-v", "-R", "-G", "-L", "-N",
];

/// The tests that take an operand on each side, written as words.
///
/// `<` and `>` are missing because they arrive as operators rather than words; [`at_binary`]
/// looks for those separately.
const BINARY: [&str; 11] = [
    "=", "==", "!=", "=~", "-eq", "-ne", "-lt", "-le", "-gt", "-ge", "-ef",
];

/// The remaining file-age comparisons, kept apart only to stay inside one line each.
const BINARY_AGE: [&str; 2] = ["-nt", "-ot"];

impl Parser<'_> {
    /// `[[ ... ]]`, parsed as an expression rather than collected as words.
    pub(super) fn cond_command(&mut self) {
        let opened_at = self.position();
        self.start(SyntaxKind::CondCommand);
        self.bump_as(SyntaxKind::LBracketBracket);
        if !self.at_cond_end() {
            self.cond_or();
        }
        if self.at_word_exactly("]]") {
            self.bump_as(SyntaxKind::RBracketBracket);
        } else {
            self.push_error(
                Error::new(
                    Span::empty(self.position()),
                    "this `[[` was never closed".to_string(),
                )
                .expecting([SyntaxKind::RBracketBracket])
                .opened_at(Span::new(opened_at, opened_at + 2)),
            );
        }
        self.finish_node();
    }

    /// Whether the expression has run out, either at `]]` or at the end of the input.
    fn at_cond_end(&self) -> bool {
        self.at_end() || self.at_word_exactly("]]")
    }

    fn cond_or(&mut self) {
        let start = self.checkpoint();
        self.cond_and();
        if !self.at(SyntaxKind::PipePipe) {
            return;
        }
        self.start_at(start, SyntaxKind::CondOr);
        while self.eat(SyntaxKind::PipePipe) {
            self.skip_newlines();
            self.cond_and();
        }
        self.finish_node();
    }

    fn cond_and(&mut self) {
        let start = self.checkpoint();
        self.cond_not();
        if !self.at(SyntaxKind::AndAnd) {
            return;
        }
        self.start_at(start, SyntaxKind::CondAnd);
        while self.eat(SyntaxKind::AndAnd) {
            self.skip_newlines();
            self.cond_not();
        }
        self.finish_node();
    }

    fn cond_not(&mut self) {
        if !self.at_word_exactly("!") {
            return self.cond_primary();
        }
        self.start(SyntaxKind::CondNot);
        self.bump_as(SyntaxKind::Bang);
        self.cond_not();
        self.finish_node();
    }

    fn cond_primary(&mut self) {
        if self.at(SyntaxKind::LParen) {
            self.start(SyntaxKind::CondGroup);
            self.bump();
            if !self.at_cond_end() {
                self.cond_or();
            }
            if !self.eat(SyntaxKind::RParen) {
                self.error("this `(` was never closed");
            }
            self.finish_node();
            return;
        }

        if UNARY.contains(&self.peek_text(0)) && self.at_word() {
            self.start(SyntaxKind::CondUnary);
            self.word();
            if self.at_word() {
                self.word();
            } else {
                self.error("this test needs something to test");
            }
            self.finish_node();
            return;
        }

        if !self.at_word() {
            // Not a word and not an operator the grammar knows: take it so the loop moves on.
            self.error("this is not part of a `[[ ]]` test");
            self.start(SyntaxKind::Error);
            self.bump();
            self.finish_node();
            return;
        }

        let start = self.checkpoint();
        self.start(SyntaxKind::CondWord);
        self.word();
        self.finish_node();
        if let Some(operator) = self.at_binary() {
            self.start_at(start, SyntaxKind::CondBinary);
            self.bump_as(operator);
            self.right_operand();
            self.finish_node();
        }
    }

    /// The operand on the right of a comparison: everything up to the next space.
    ///
    /// It cannot be read as an ordinary word, because a regex owns the punctuation a word would
    /// give away. In `[[ $x =~ ^(cat|dog)$ ]]` the parentheses group *the regex*, and reading them
    /// as the grammar's own grouping loses the operand and then the `]]` with it. Whatever is
    /// written without a space in it is one operand, whatever it is made of.
    fn right_operand(&mut self) {
        self.skip_trivia();
        if self.at_cond_end() {
            self.error("this comparison has nothing on its right");
            return;
        }
        self.start(SyntaxKind::CondWord);
        while let Some(kind) = self.raw() {
            if kind.is_trivia() || kind == SyntaxKind::Newline {
                break;
            }
            if kind.is_word_piece() {
                self.word_piece();
            } else {
                self.bump_raw();
            }
        }
        self.finish_node();
    }

    /// The operator after a left operand, if there is one.
    ///
    /// Inside `[[ ]]` a `<` compares strings; the lexer has no way to know that, so it arrives as
    /// the redirection operator and is reinterpreted here.
    fn at_binary(&self) -> Option<SyntaxKind> {
        match self.peek()? {
            SyntaxKind::Less => Some(SyntaxKind::Less),
            SyntaxKind::Great => Some(SyntaxKind::Great),
            SyntaxKind::Text
                if BINARY.contains(&self.peek_text(0)) || BINARY_AGE.contains(&self.peek_text(0)) =>
            {
                Some(SyntaxKind::Text)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::parse::parse;

    fn shape(text: &str) -> String {
        parse(text)
            .tree()
            .dump()
            .lines()
            .filter(|line| line.contains("Cond"))
            .map(|line| line.trim().split('@').next().unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_unary_test_takes_one_operand() {
        assert_eq!(shape("[[ -f x ]]"), "CondCommand CondUnary");
        assert!(parse("[[ -f x ]]").is_clean());
    }

    #[test]
    fn a_binary_test_takes_one_on_each_side() {
        assert_eq!(shape("[[ $a == b ]]"), "CondCommand CondBinary CondWord CondWord");
        assert!(parse("[[ $a == b ]]").is_clean());
    }

    #[test]
    fn a_bare_word_is_a_test_on_its_own() {
        assert_eq!(shape("[[ $x ]]"), "CondCommand CondWord");
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // `a || b && c` is `a || (b && c)`.
        assert_eq!(
            shape("[[ -f a || -f b && -f c ]]"),
            "CondCommand CondOr CondUnary CondAnd CondUnary CondUnary"
        );
    }

    #[test]
    fn negation_and_grouping_nest() {
        assert_eq!(
            shape("[[ ! ( -f a || -f b ) ]]"),
            "CondCommand CondNot CondGroup CondOr CondUnary CondUnary"
        );
        assert!(parse("[[ ! ( -f a || -f b ) ]]").is_clean());
    }

    #[test]
    fn angle_brackets_compare_rather_than_redirect() {
        assert_eq!(shape("[[ a < b ]]"), "CondCommand CondBinary CondWord CondWord");
        assert!(parse("[[ a < b ]]").is_clean());
        // Outside the brackets the same character still redirects.
        assert!(!parse("cmd < b").tree().dump().contains("CondBinary"));
    }

    #[test]
    fn an_unclosed_test_is_reported() {
        let parsed = parse("[[ -f x");
        assert_eq!(parsed.errors().len(), 1);
        assert_eq!(parsed.errors()[0].message, "this `[[` was never closed");
    }
}
