//! The commands built out of reserved words: conditionals, loops, and `case`.
//!
//! Each one knows the word it is waiting for, which is the whole point of writing the parser by
//! hand. When `fi` never arrives the report can say which `if` is still open and where it started,
//! rather than that something went wrong near the end of the file.

use super::Parser;
use crate::error::Error;
use crate::span::Span;
use crate::tree::SyntaxKind;

impl Parser<'_> {
    /// Try to parse a compound command, and say whether there was one.
    pub(super) fn compound_command(&mut self) -> bool {
        if self.at_keyword(SyntaxKind::If) {
            self.if_command();
        } else if self.at_keyword(SyntaxKind::While) {
            self.loop_command(SyntaxKind::While, SyntaxKind::WhileCommand);
        } else if self.at_keyword(SyntaxKind::Until) {
            self.loop_command(SyntaxKind::Until, SyntaxKind::UntilCommand);
        } else if self.at_keyword(SyntaxKind::For) {
            self.for_command();
        } else if self.at_keyword(SyntaxKind::Select) {
            self.select_command();
        } else if self.at_keyword(SyntaxKind::Case) {
            self.case_command();
        } else if self.at_keyword(SyntaxKind::LBrace) {
            self.group();
        } else if self.at_word_exactly("[[") {
            self.cond_command();
        } else if self.at(SyntaxKind::LParen) {
            if self.next_is_adjacent(SyntaxKind::LParen) {
                self.arith_command();
            } else {
                self.subshell();
            }
        } else {
            return false;
        }
        true
    }

    /// Report a construct that was opened and never closed.
    ///
    /// **Pointed at the opener.** The parser notices at the end of the body, which for an unclosed
    /// block is the end of the file — and an error there says only that the script ran out, which
    /// the reader can already see. The `if` that is still open is the thing to go and look at, so
    /// that is what the span names.
    pub(super) fn unclosed(&mut self, opener: &str, opened_at: u32, expected: &[SyntaxKind]) {
        let span = Span::new(opened_at, opened_at + opener.len() as u32);
        let mut error = Error::new(span, format!("this `{opener}` was never closed"))
            .expecting(expected.iter().copied())
            .opened_at(span);
        // Ran out of input rather than met something it could not use: another line finishes it.
        if self.at_end() {
            error = error.unfinished();
        }
        self.push_error(error);
    }

    /// Take a reserved word, or report that it is missing without consuming anything.
    fn expect_keyword(&mut self, kind: SyntaxKind, opener: &str, opened_at: u32) -> bool {
        if self.eat_keyword(kind) {
            return true;
        }
        self.unclosed(opener, opened_at, &[kind]);
        false
    }

    fn if_command(&mut self) {
        let opened_at = self.position();
        self.start(SyntaxKind::IfCommand);
        self.bump_as(SyntaxKind::If);
        self.command_list_until(&[]);
        if !self.expect_keyword(SyntaxKind::Then, "if", opened_at) {
            // Without a `then` there is no body to look for a `fi` after. Saying so twice about
            // the same `if` tells nobody anything new.
            self.finish_node();
            return;
        }
        self.command_list_until(&[]);

        while self.at_keyword(SyntaxKind::Elif) {
            let elif_at = self.position();
            self.start(SyntaxKind::ElifClause);
            self.bump_as(SyntaxKind::Elif);
            self.command_list_until(&[]);
            self.expect_keyword(SyntaxKind::Then, "elif", elif_at);
            self.command_list_until(&[]);
            self.finish_node();
        }
        if self.at_keyword(SyntaxKind::Else) {
            self.start(SyntaxKind::ElseClause);
            self.bump_as(SyntaxKind::Else);
            self.command_list_until(&[]);
            self.finish_node();
        }
        self.expect_keyword(SyntaxKind::Fi, "if", opened_at);
        self.trailing_redirects();
        self.finish_node();
    }

    fn loop_command(&mut self, keyword: SyntaxKind, node: SyntaxKind) {
        let opened_at = self.position();
        let opener = keyword.static_text().unwrap_or("while");
        self.start(node);
        self.bump_as(keyword);
        self.command_list_until(&[]);
        if self.expect_keyword(SyntaxKind::Do, opener, opened_at) {
            self.command_list_until(&[]);
            self.expect_keyword(SyntaxKind::Done, opener, opened_at);
        }
        self.trailing_redirects();
        self.finish_node();
    }

    fn for_command(&mut self) {
        let opened_at = self.position();
        if self.peek_nth(1) == Some(SyntaxKind::LParen) {
            return self.arithmetic_for(opened_at);
        }
        self.start(SyntaxKind::ForCommand);
        self.bump_as(SyntaxKind::For);
        if self.at_word() {
            self.word();
        } else {
            self.error("`for` needs the name of a variable to set");
        }
        if self.eat_keyword(SyntaxKind::In) {
            while self.at_word() {
                self.word();
            }
        }
        self.end_of_header();
        if self.expect_keyword(SyntaxKind::Do, "for", opened_at) {
            self.command_list_until(&[]);
            self.expect_keyword(SyntaxKind::Done, "for", opened_at);
        }
        self.trailing_redirects();
        self.finish_node();
    }

    /// `select name in words; do ... done`. Written like `for`, and read like it.
    fn select_command(&mut self) {
        let opened_at = self.position();
        self.start(SyntaxKind::SelectCommand);
        self.bump_as(SyntaxKind::Select);
        if self.at_word() {
            self.word();
        } else {
            self.error("`select` needs the name of a variable to set");
        }
        if self.eat_keyword(SyntaxKind::In) {
            while self.at_word() {
                self.word();
            }
        }
        self.end_of_header();
        if self.expect_keyword(SyntaxKind::Do, "select", opened_at) {
            self.command_list_until(&[]);
            self.expect_keyword(SyntaxKind::Done, "select", opened_at);
        }
        self.trailing_redirects();
        self.finish_node();
    }

    /// `for ((init; cond; step)) do ... done`.
    fn arithmetic_for(&mut self, opened_at: u32) {
        self.start(SyntaxKind::ArithForCommand);
        self.bump_as(SyntaxKind::For);
        self.take_double_parens();
        self.end_of_header();
        if self.expect_keyword(SyntaxKind::Do, "for", opened_at) {
            self.command_list_until(&[]);
            self.expect_keyword(SyntaxKind::Done, "for", opened_at);
        }
        self.trailing_redirects();
        self.finish_node();
    }

    /// The `;` and newlines between a loop's header and its `do`.
    fn end_of_header(&mut self) {
        while self.at(SyntaxKind::Semi) || self.at(SyntaxKind::Newline) {
            self.bump();
        }
    }

    fn group(&mut self) {
        let opened_at = self.position();
        self.start(SyntaxKind::Group);
        self.bump_as(SyntaxKind::LBrace);
        self.command_list_until(&[]);
        self.expect_keyword(SyntaxKind::RBrace, "{", opened_at);
        self.trailing_redirects();
        self.finish_node();
    }

    fn subshell(&mut self) {
        let opened_at = self.position();
        self.start(SyntaxKind::Subshell);
        self.bump();
        self.push_guard(SyntaxKind::RParen);
        self.command_list_until(&[SyntaxKind::RParen]);
        self.pop_guard();
        if !self.eat(SyntaxKind::RParen) {
            self.unclosed("(", opened_at, &[SyntaxKind::RParen]);
        }
        self.trailing_redirects();
        self.finish_node();
    }

    /// `(( expr ))`, whose contents are arithmetic and stay as text.
    fn arith_command(&mut self) {
        self.start(SyntaxKind::ArithCommand);
        self.take_double_parens();
        self.trailing_redirects();
        self.finish_node();
    }

    /// Consume `((`, everything up to the matching `))`, and the `))`.
    fn take_double_parens(&mut self) {
        let opened_at = self.position();
        if !self.eat(SyntaxKind::LParen) || !self.eat(SyntaxKind::LParen) {
            self.unclosed("((", opened_at, &[SyntaxKind::LParen]);
            return;
        }
        while !self.at_end() {
            if self.at(SyntaxKind::RParen) && self.next_is_adjacent(SyntaxKind::RParen) {
                self.bump();
                self.bump();
                return;
            }
            self.bump();
        }
        self.unclosed("((", opened_at, &[SyntaxKind::RParen]);
    }

    fn case_command(&mut self) {
        let opened_at = self.position();
        self.start(SyntaxKind::CaseCommand);
        self.bump_as(SyntaxKind::Case);
        if self.at_word() {
            self.word();
        } else {
            self.error("`case` needs a word to match against");
        }
        self.skip_newlines();
        if !self.expect_keyword(SyntaxKind::In, "case", opened_at) {
            self.finish_node();
            return;
        }
        self.skip_newlines();
        while !self.at_end() && !self.at_keyword(SyntaxKind::Esac) {
            let before = self.progress();
            self.case_item();
            if self.progress() == before {
                self.error("this is not a `case` pattern");
                self.start(SyntaxKind::Error);
                self.bump();
                self.finish_node();
            }
            self.skip_newlines();
        }
        self.expect_keyword(SyntaxKind::Esac, "case", opened_at);
        self.trailing_redirects();
        self.finish_node();
    }

    fn case_item(&mut self) {
        self.start(SyntaxKind::CaseItem);
        self.eat(SyntaxKind::LParen);
        loop {
            self.start(SyntaxKind::CasePattern);
            if self.at_word() {
                self.word();
            }
            self.finish_node();
            if !self.eat(SyntaxKind::Pipe) {
                break;
            }
        }
        if !self.eat(SyntaxKind::RParen) {
            self.error("a `case` pattern ends with `)`");
        }
        self.command_list_until(&[
            SyntaxKind::SemiSemi,
            SyntaxKind::SemiAmp,
            SyntaxKind::SemiSemiAmp,
        ]);
        if self.at(SyntaxKind::SemiSemi)
            || self.at(SyntaxKind::SemiAmp)
            || self.at(SyntaxKind::SemiSemiAmp)
        {
            self.bump();
        }
        self.finish_node();
    }
}
