//! The expression inside `[[ ... ]]`.
//!
//! This is a different language from the one around it. `<` and `>` compare strings rather than
//! redirecting, `&&` and `||` join tests rather than commands, and the operands are not split or
//! globbed. Reading it as a run of words would throw away exactly the structure that a caller
//! lowering it to something executable needs, so it gets a grammar.

use super::Parser;
use crate::tree::SyntaxKind;

/// The tests that take one operand.
///
/// All twenty-six bash has. `-S` and `-O` were the two missing, and a missing one does not degrade
/// to a word: this grammar has nowhere to put an operator it does not know, so `[[ -S "$sock" ]]`
/// reported `this '[[' was never closed` and the recovery took the rest of the construct with it.
/// Both appear in ordinary scripts — a socket test and an owned-by-me test.
const UNARY: [&str; 26] = [
    "-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-p", "-r", "-s", "-t", "-u", "-w", "-x",
    "-z", "-n", "-o", "-v", "-R", "-G", "-L", "-N", "-O", "-S",
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
            self.unclosed("[[", opened_at, &[SyntaxKind::RBracketBracket]);
        }
        self.trailing_redirects();
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

    /// A term, which may begin after newlines: the test is a compound command, and bash skips
    /// them before every term and after every complete one. `[[ -n $a⏎&& -n $b ]]` is one test.
    fn cond_not(&mut self) {
        self.skip_newlines();
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
            self.skip_newlines();
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
            self.skip_newlines();
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
        // A bare word is a test of its own, and bash takes nothing after it but an operator or
        // the end: `[[ a⏎]]` is an error there, so the newline is left for `]]` to trip over.
        if let Some(operator) = self.at_binary() {
            let regex = self.peek_text(0) == "=~";
            self.start_at(start, SyntaxKind::CondBinary);
            self.bump_as(operator);
            self.right_operand(regex);
            self.finish_node();
            self.skip_newlines();
        }
    }

    /// The operand on the right of a comparison: one word.
    ///
    /// A regex owns punctuation a word would give away. In `[[ $x =~ ^(cat|dog)$ ]]` the
    /// parentheses group *the regex*, and in `=~ (^| )$c( |$)` the spaces inside them are its
    /// own. The lexer reads that operand as a run of text, so it arrives as one word; the
    /// parentheses and `|` taken here are for a regex the lexer did not see as one.
    ///
    /// A `)` with nothing of the operand's own to close is not part of it: in
    /// `[[ x && (a == a) ]]` it closes the group the comparison is in.
    fn right_operand(&mut self, regex: bool) {
        self.skip_trivia();
        if self.at_cond_end() || self.raw() == Some(SyntaxKind::Newline) {
            self.error("this comparison has nothing on its right");
            return;
        }
        self.start(SyntaxKind::CondWord);
        let mut depth = 0u32;
        while let Some(kind) = self.raw() {
            match kind {
                _ if kind.is_word_piece() => self.word_piece(),
                SyntaxKind::LParen if regex => {
                    depth += 1;
                    self.bump_raw();
                }
                SyntaxKind::RParen if regex && depth > 0 => {
                    depth -= 1;
                    self.bump_raw();
                }
                SyntaxKind::Pipe if regex => self.bump_raw(),
                _ if regex && depth > 0 => self.bump_raw(),
                _ => break,
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
                if BINARY.contains(&self.peek_text(0))
                    || BINARY_AGE.contains(&self.peek_text(0)) =>
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
        assert_eq!(
            shape("[[ $a == b ]]"),
            "CondCommand CondBinary CondWord CondWord"
        );
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
        assert_eq!(
            shape("[[ a < b ]]"),
            "CondCommand CondBinary CondWord CondWord"
        );
        assert!(parse("[[ a < b ]]").is_clean());
        // Outside the brackets the same character still redirects.
        assert!(!parse("cmd < b").tree().dump().contains("CondBinary"));
    }

    #[test]
    fn a_regex_keeps_its_own_punctuation() {
        // The parentheses group the regex, not the expression, and `]]` must still be found.
        for source in [
            "[[ cat =~ ^(cat|dog)$ ]]",
            "[[ ab =~ (a)(x)?(b) ]]",
            "[[ $d =~ ^([0-9]{4})-([0-9]{2}) ]]",
            "[[ $x =~ a ]] && echo y",
        ] {
            assert!(
                parse(source).is_clean(),
                "{source:?} reported {:?}",
                parse(source).errors()
            );
            assert!(
                shape(source).contains("CondBinary"),
                "{source:?}: {}",
                shape(source)
            );
        }
    }

    #[test]
    fn an_unclosed_test_is_reported() {
        let parsed = parse("[[ -f x");
        assert_eq!(parsed.errors().len(), 1);
        assert_eq!(parsed.errors()[0].message, "this `[[` was never closed");
    }
}
