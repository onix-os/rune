//! `coproc [NAME] command`, a command run alongside the shell with a pipe each way.
//!
//! bash's grammar takes a name only before a compound command: `coproc NAME { …; }` publishes its
//! descriptors in `NAME`, while in `coproc cat file` the first word is the command. The command
//! that follows is one command, simple or compound, and its redirections are its own.

use super::Parser;
use crate::tree::SyntaxKind;

/// The reserved words a compound command opens with, besides `(`.
const COMPOUND_OPENERS: [&str; 8] = ["{", "[[", "if", "while", "until", "for", "case", "select"];

impl Parser<'_> {
    /// `coproc`, an optional name, and the command it runs.
    pub(super) fn coproc_command(&mut self) {
        self.start(SyntaxKind::CoprocCommand);
        self.bump_as(SyntaxKind::Coproc);
        if self.at_coproc_name() {
            self.word();
        }
        if self.at_word() || self.at_redirect() || self.at(SyntaxKind::LParen) {
            self.command();
        } else {
            self.error("`coproc` needs a command to run");
        }
        self.finish_node();
    }

    /// Whether the next word names the coprocess: one plain word, then a compound command.
    fn at_coproc_name(&self) -> bool {
        let Some(name) = self.significant(0) else {
            return false;
        };
        if self.tokens[name].kind != SyntaxKind::Text || self.opens_compound(name) {
            return false;
        }
        let mut next = name + 1;
        if self
            .tokens
            .get(next)
            .is_some_and(|token| token.kind.is_word_piece())
        {
            return false;
        }
        while self
            .tokens
            .get(next)
            .is_some_and(|token| token.kind == SyntaxKind::Whitespace)
        {
            next += 1;
        }
        next < self.tokens.len() && self.opens_compound(next)
    }

    /// Whether the token at `index` begins a compound command.
    fn opens_compound(&self, index: usize) -> bool {
        match self.tokens[index].kind {
            SyntaxKind::LParen => true,
            SyntaxKind::Text => {
                COMPOUND_OPENERS.contains(&self.token_text(index))
                    && !self.tokens.get(index + 1).is_some_and(|next| {
                        next.kind.is_word_piece() || next.kind == SyntaxKind::LineContinuation
                    })
            }
            _ => false,
        }
    }
}
