//! Lists, and-or chains, pipelines, and the simple command at the bottom of them.

use super::Parser;
use crate::tree::SyntaxKind;

/// How deep the grammar will nest before it stops descending.
///
/// `$($($(...)))` is a stack overflow waiting to happen, and a parser is exactly the program most
/// likely to be handed one on purpose.
const MAX_DEPTH: u32 = 96;

/// What ends a statement.
const SEPARATORS: [SyntaxKind; 3] = [SyntaxKind::Semi, SyntaxKind::Amp, SyntaxKind::Newline];

/// The builtins whose arguments are parsed as assignments rather than as words.
///
/// `declare -a x=(a b)` has to see an array on the right; without this the `(` reads as a subshell
/// and every reserved word inside it is taken as a keyword.
const DECLARATIONS: [&str; 5] = ["declare", "typeset", "local", "export", "readonly"];

impl Parser<'_> {
    pub(super) fn script(&mut self) {
        loop {
            self.command_list_until(&[]);
            if self.at_end() {
                return;
            }
            // Something no rule could use — a `done` with no `do`, say. Report it once and skip to
            // the next place a command could start, so one mistake is one message.
            let before = self.progress();
            self.error("this is not something a command can start with");
            self.recover(&[]);
            if self.progress() == before {
                self.bump();
            }
        }
    }

    /// Panic-mode recovery: take the token that went wrong, then run to the next separator.
    ///
    /// Without this a single stray word turns into an error on every token after it, and a report
    /// that cries wolf is a report that gets turned off.
    fn recover(&mut self, closers: &[SyntaxKind]) {
        self.start(SyntaxKind::Error);
        if !self.at_end() {
            self.bump();
        }
        while !self.at_end()
            && !self.at_any(closers)
            && !self.at_any(&SEPARATORS)
            && !self.at_list_end()
        {
            self.bump();
        }
        self.finish_node();
    }

    /// A run of statements, stopping before any of `closers` without consuming it.
    pub(super) fn command_list_until(&mut self, closers: &[SyntaxKind]) {
        if self.depth >= MAX_DEPTH {
            self.error("this is nested too deeply to parse");
            while !self.at_end() && !self.at_any(closers) {
                self.bump();
            }
            return;
        }
        self.depth += 1;
        self.start(SyntaxKind::CommandList);
        loop {
            // Blank lines and stray separators belong to the list, not to any statement in it.
            while self.at(SyntaxKind::Newline) && !closers.contains(&SyntaxKind::Newline) {
                self.bump();
            }
            if self.at_end() || self.at_any(closers) || self.at_list_end() {
                break;
            }
            let before = self.progress();
            self.list_item();
            if self.progress() == before {
                // The statement rule did not move. Take a token so the loop cannot spin.
                self.error("this is not something a command can start with");
                self.recover(closers);
            }
        }
        self.finish_node();
        self.depth -= 1;
    }

    /// The reserved words that close a body, and so end the list inside it.
    fn at_list_end(&self) -> bool {
        [
            SyntaxKind::Fi,
            SyntaxKind::Done,
            SyntaxKind::Esac,
            SyntaxKind::Then,
            SyntaxKind::Elif,
            SyntaxKind::Else,
            SyntaxKind::Do,
            SyntaxKind::RBrace,
        ]
        .iter()
        .any(|kind| self.at_keyword(*kind))
    }

    fn at_any(&self, kinds: &[SyntaxKind]) -> bool {
        self.peek().is_some_and(|kind| kinds.contains(&kind))
    }

    /// One statement, and whatever terminates it.
    fn list_item(&mut self) {
        self.start(SyntaxKind::ListItem);
        self.and_or();
        if self.at_any(&SEPARATORS) {
            self.bump();
        }
        self.finish_node();
    }

    /// `a && b || c`. The node appears only when there is an operator to justify it.
    fn and_or(&mut self) {
        let start = self.checkpoint();
        self.pipeline();
        if !self.at(SyntaxKind::AndAnd) && !self.at(SyntaxKind::PipePipe) {
            return;
        }
        self.start_at(start, SyntaxKind::AndOrList);
        while self.at(SyntaxKind::AndAnd) || self.at(SyntaxKind::PipePipe) {
            self.bump();
            self.skip_newlines();
            self.pipeline();
        }
        self.finish_node();
    }

    /// `! time a | b`. Also only wrapped when there is something to wrap.
    fn pipeline(&mut self) {
        let start = self.checkpoint();
        let negated = self.at_word_exactly("!");
        let timed = self.at_word_exactly("time");
        if negated || timed {
            self.start_at(start, SyntaxKind::Pipeline);
            if negated {
                self.bump_as(SyntaxKind::Bang);
            }
            if self.at_word_exactly("time") {
                self.bump_as(SyntaxKind::Time);
            }
            self.command();
            self.pipe_rest();
            self.finish_node();
            return;
        }
        self.command();
        if self.at(SyntaxKind::Pipe) || self.at(SyntaxKind::PipeAmp) {
            self.start_at(start, SyntaxKind::Pipeline);
            self.pipe_rest();
            self.finish_node();
        }
    }

    fn pipe_rest(&mut self) {
        while self.at(SyntaxKind::Pipe) || self.at(SyntaxKind::PipeAmp) {
            self.bump();
            self.skip_newlines();
            self.command();
        }
    }

    pub(super) fn skip_newlines(&mut self) {
        while self.at(SyntaxKind::Newline) {
            self.bump();
        }
    }

    /// One command: compound, a function definition, or a simple command.
    pub(super) fn command(&mut self) {
        if self.compound_command() {
            return;
        }
        if self.at_function_definition() {
            self.function_definition();
            return;
        }
        self.simple_command();
    }

    /// `name=value ... command arg ... >file`, in any order the shell allows.
    fn simple_command(&mut self) {
        self.start(SyntaxKind::SimpleCommand);
        let mut seen_command_word = false;
        let mut takes_assignments = true;
        loop {
            let before = self.progress();
            if self.at_redirect() {
                self.redirect();
            } else if takes_assignments && self.at_assignment() {
                self.assignment();
            } else if self.at_word() {
                if !seen_command_word {
                    // `declare -a x=(a b)` assigns; `echo x=(a b)` is an error even in bash. Only
                    // the declaration builtins keep reading their arguments as assignments.
                    takes_assignments = DECLARATIONS.contains(&self.peek_text(0));
                    seen_command_word = true;
                }
                self.word();
            } else {
                break;
            }
            // A rule that claimed the token but did not take it would spin here. Nothing should
            // reach this, and a parser that hangs is worse than one that is wrong.
            if self.progress() == before {
                break;
            }
        }
        self.finish_node();
    }
}
