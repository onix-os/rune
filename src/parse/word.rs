//! Gathering the lexer's pieces back into words.
//!
//! Adjacency is the whole rule, so this reads the raw stream rather than the one the grammar sees:
//! a space ends a word, and the trivia-skipping the rest of the parser does would step over it.
//! A line continuation is the exception the shell makes — it is removed before anything else, so
//! `ab\<newline>cd` is one word.

use super::Parser;
use crate::tree::SyntaxKind;

impl Parser<'_> {
    pub(super) fn at_word(&self) -> bool {
        self.peek().is_some_and(|kind| {
            kind.is_word_piece()
                // A closing backtick is a word piece that will not start a word, and saying
                // otherwise leaves a caller waiting for a word that never arrives.
                && !(kind == SyntaxKind::Backtick && self.in_backticks())
        })
    }

    /// One word, however many pieces it is written in.
    pub(super) fn word(&mut self) {
        self.start(SyntaxKind::Word);
        self.word_rest();
        self.finish_node();
    }

    /// Every remaining piece of the word already under construction.
    pub(super) fn word_rest(&mut self) {
        while self.raw().is_some_and(|kind| {
            (kind.is_word_piece() || kind == SyntaxKind::LineContinuation)
                // The backtick that closes a substitution is a word piece like the one that opened
                // it. Only knowing we are inside one tells them apart.
                && !(kind == SyntaxKind::Backtick && self.in_backticks())
        }) {
            self.word_piece();
        }
    }

    /// One piece. Four of them open a structure that has to be parsed rather than collected.
    pub(super) fn word_piece(&mut self) {
        match self.raw() {
            Some(SyntaxKind::DollarParen) => self.command_substitution(SyntaxKind::RParen),
            Some(SyntaxKind::Backtick) => self.command_substitution(SyntaxKind::Backtick),
            Some(SyntaxKind::ProcSubIn | SyntaxKind::ProcSubOut) => self.process_substitution(),
            Some(SyntaxKind::DollarParenParen) => self.arithmetic_expansion(),
            Some(SyntaxKind::DollarBrace) => self.parameter_expansion(),
            _ => self.bump_raw(),
        }
    }

    /// `<(cmd)` or `>(cmd)`, whose contents are a script and whose value is a filename.
    fn process_substitution(&mut self) {
        let opened_at = self.position();
        let opener = if self.raw() == Some(SyntaxKind::ProcSubIn) {
            "<("
        } else {
            ">("
        };
        self.open(SyntaxKind::ProcessSubstitution);
        self.bump_raw();
        self.push_guard(SyntaxKind::RParen);
        self.command_list_until(&[SyntaxKind::RParen]);
        self.pop_guard();
        if !self.eat(SyntaxKind::RParen) {
            self.unclosed(opener, opened_at, &[SyntaxKind::RParen]);
        }
        self.finish_node();
    }

    /// `$(...)` or `` `...` ``, whose contents are a script in their own right.
    fn command_substitution(&mut self, closer: SyntaxKind) {
        let opened_at = self.position();
        self.open(SyntaxKind::CommandSubstitution);
        self.bump_raw();
        if closer == SyntaxKind::Backtick {
            self.enter_backticks();
        }
        self.push_guard(closer);
        self.command_list_until(&[closer]);
        self.pop_guard();
        if closer == SyntaxKind::Backtick {
            self.leave_backticks();
        }
        if !self.eat(closer) {
            let opener = if closer == SyntaxKind::Backtick {
                "`"
            } else {
                "$("
            };
            self.unclosed(opener, opened_at, &[closer]);
        }
        self.finish_node();
    }

    /// `$((...))`. What is between the parentheses stays as text; it is not shell.
    fn arithmetic_expansion(&mut self) {
        let opened_at = self.position();
        self.open(SyntaxKind::ArithmeticExpansion);
        self.bump_raw();
        while self.raw() == Some(SyntaxKind::Text) {
            self.bump_raw();
        }
        if !self.eat(SyntaxKind::RParenRParen) {
            self.unclosed("$((", opened_at, &[SyntaxKind::RParenRParen]);
        }
        self.finish_node();
    }

    /// `${...}`, including its subscript if it has one.
    fn parameter_expansion(&mut self) {
        let opened_at = self.position();
        self.open(SyntaxKind::ParameterExpansion);
        self.bump_raw();
        loop {
            match self.raw() {
                None | Some(SyntaxKind::RBrace) => break,
                Some(SyntaxKind::LBracket) => self.subscript(),
                _ => self.word_piece(),
            }
        }
        if !self.eat(SyntaxKind::RBrace) {
            self.unclosed("${", opened_at, &[SyntaxKind::RBrace]);
        }
        self.finish_node();
    }

    fn subscript(&mut self) {
        self.open(SyntaxKind::Subscript);
        self.bump_raw();
        while !matches!(
            self.raw(),
            None | Some(SyntaxKind::RBracket | SyntaxKind::RBrace)
        ) {
            self.word_piece();
        }
        self.eat(SyntaxKind::RBracket);
        self.finish_node();
    }
}
