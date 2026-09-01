//! Recursive descent over the token stream.
//!
//! One function per production, each one knowing what it was looking for. That is the whole reason
//! this is not a generated parser: a PEG can say no alternative matched, but only a function called
//! `if_command` knows it wanted `fi` and knows which `if` sent it looking.
//!
//! Parsing does not fail. [`parse`] always returns a tree covering the whole input, with whatever
//! went wrong collected alongside it.

mod assign;
mod command;
mod compound;
mod redirect;
mod word;

use crate::error::Completeness;
use crate::error::Error;
use crate::lex::{Lexed, Unclosed, lex};
use crate::source::Source;
use crate::span::Span;
use crate::tree::{Builder, SyntaxKind, Tree};

/// A parsed script: the tree, and everything the parser could not make sense of.
#[derive(Debug, Clone)]
pub struct Parsed {
    tree: Tree,
    errors: Vec<Error>,
}

impl Parsed {
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn errors(&self) -> &[Error] {
        &self.errors
    }

    /// Whether the script parsed with nothing to report.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    /// Whether this is a whole program, an unfinished one, or not shell at all.
    ///
    /// The three-way answer is what an interactive prompt needs: run it, read another line, or
    /// report it. What separates the last two is *where* the trouble is — a construct still open
    /// when the input ran out will be finished by the next line; anything else will not.
    pub fn completeness(&self) -> Completeness {
        if self.errors.is_empty() {
            return Completeness::Complete;
        }
        let end = self.tree.source().len();
        let ran_out = self
            .errors
            .iter()
            .all(|error| error.opened_at.is_some() && error.span.start >= end);
        if ran_out {
            Completeness::Unfinished
        } else {
            Completeness::Invalid
        }
    }
}

/// Parse `text`. This cannot fail; a file of nonsense produces a tree of error nodes and a list.
pub fn parse(text: &str) -> Parsed {
    let mut parser = Parser::new(text);
    parser.script();
    parser.finish()
}

pub(crate) struct Parser<'a> {
    text: &'a str,
    tokens: Vec<Lexed>,
    /// What the lexer had open when the input ran out.
    unclosed: Vec<Unclosed>,
    /// Where each token starts, so a rule can name a position without counting.
    starts: Vec<u32>,
    /// Index into `tokens`, trivia included.
    pos: usize,
    /// How many backtick substitutions are open, so a closing `` ` `` is not read as an opening one.
    backticks: u32,
    /// How many rules are on the stack, so nesting can be stopped before the stack is.
    depth: u32,
    builder: Builder,
    errors: Vec<Error>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        let lexing = lex(text);
        let mut starts = Vec::with_capacity(lexing.tokens.len() + 1);
        let mut at = 0;
        for token in &lexing.tokens {
            starts.push(at);
            at += token.len;
        }
        starts.push(at);
        Self {
            text,
            tokens: lexing.tokens,
            unclosed: lexing.unclosed,
            starts,
            pos: 0,
            backticks: 0,
            depth: 0,
            builder: Builder::new(Source::new(text), SyntaxKind::Script),
            errors: Vec::new(),
        }
    }

    fn finish(mut self) -> Parsed {
        self.report_unclosed();
        Parsed {
            tree: self.builder.build(),
            errors: self.errors,
        }
    }

    /// Turn what the lexer left open into errors, unless a rule already said so.
    ///
    /// An unterminated quote has no rule to catch it — the word simply runs to the end of the
    /// file — so it can only be reported from here. The others usually have been, which is why
    /// this checks before speaking twice about one mistake.
    fn report_unclosed(&mut self) {
        let end = self.starts.last().copied().unwrap_or_default();
        for open in std::mem::take(&mut self.unclosed) {
            let already = self
                .errors
                .iter()
                .any(|error| error.opened_at.is_some_and(|span| span.start == open.at));
            if already {
                continue;
            }
            let width = open.opener.len() as u32;
            self.errors.push(
                Error::new(
                    Span::empty(end),
                    format!("this `{}` was never closed", open.opener),
                )
                .opened_at(Span::new(open.at, open.at + width)),
            );
        }
        self.errors.sort_by_key(|error| error.span.start);
    }

    // ---- looking ----

    /// Whether a kind is carried into the tree without the grammar branching on it.
    ///
    /// Here-document bodies are in here because of where they land. `cat <<EOF` is followed by the
    /// rest of its line and only then by the body, so the body arrives in the stream nowhere near
    /// the redirection that asked for it, and no rule is in a position to want it.
    fn skippable(kind: SyntaxKind) -> bool {
        kind.is_trivia() || matches!(kind, SyntaxKind::HeredocText | SyntaxKind::HeredocEnd)
    }

    /// The index of the nth token the grammar can see, counting from the cursor.
    fn significant(&self, nth: usize) -> Option<usize> {
        let mut seen = 0;
        for index in self.pos..self.tokens.len() {
            if Self::skippable(self.tokens[index].kind) {
                continue;
            }
            if seen == nth {
                return Some(index);
            }
            seen += 1;
        }
        None
    }

    /// What the grammar sees next, without moving.
    pub(crate) fn peek(&self) -> Option<SyntaxKind> {
        Some(self.tokens[self.significant(0)?].kind)
    }

    pub(crate) fn peek_nth(&self, nth: usize) -> Option<SyntaxKind> {
        Some(self.tokens[self.significant(nth)?].kind)
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.peek() == Some(kind)
    }

    pub(crate) fn at_end(&self) -> bool {
        self.significant(0).is_none()
    }

    /// The text of the nth token the grammar can see.
    pub(crate) fn peek_text(&self, nth: usize) -> &'a str {
        match self.significant(nth) {
            Some(index) => self.token_text(index),
            None => "",
        }
    }

    fn token_text(&self, index: usize) -> &'a str {
        let start = self.starts.get(index).copied().unwrap_or_default() as usize;
        let end = self.starts.get(index + 1).copied().unwrap_or_default() as usize;
        self.text.get(start..end).unwrap_or("")
    }

    /// The kind of the token under the cursor, trivia included.
    pub(crate) fn raw(&self) -> Option<SyntaxKind> {
        self.tokens.get(self.pos).map(|token| token.kind)
    }

    /// Whether the token straight after the next one is `kind`, with nothing in between.
    ///
    /// `((` is two tokens, and so is `( (`. Only the first opens an arithmetic command.
    pub(crate) fn next_is_adjacent(&self, kind: SyntaxKind) -> bool {
        let Some(first) = self.significant(0) else {
            return false;
        };
        self.tokens
            .get(first + 1)
            .is_some_and(|next| next.kind == kind)
    }

    /// How far the parser has got. A rule that returns without changing this made no progress.
    pub(crate) const fn progress(&self) -> usize {
        self.pos
    }

    /// Whether a `` ` `` here would be closing a substitution rather than opening one.
    pub(crate) const fn in_backticks(&self) -> bool {
        self.backticks > 0
    }

    pub(crate) const fn enter_backticks(&mut self) {
        self.backticks += 1;
    }

    pub(crate) const fn leave_backticks(&mut self) {
        self.backticks = self.backticks.saturating_sub(1);
    }

    /// Where the next token the grammar can see begins.
    pub(crate) fn position(&self) -> u32 {
        let index = self.significant(0).unwrap_or(self.tokens.len());
        self.starts
            .get(index)
            .copied()
            .unwrap_or(self.builder.offset())
    }

    // ---- moving ----

    /// Carry trivia and here-document bodies into the tree without consuming a grammar token.
    pub(crate) fn skip_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.pos) {
            if !Self::skippable(token.kind) {
                return;
            }
            self.builder.token(token.kind, token.len);
            self.pos += 1;
        }
    }

    /// Take the next token the grammar can see, keeping the kind the lexer gave it.
    pub(crate) fn bump(&mut self) {
        let Some(kind) = self.peek() else {
            return;
        };
        self.bump_as(kind);
    }

    /// Take the next token the grammar can see, under a kind the grammar chose.
    ///
    /// This is where `if` stops being a word. The lexer cannot know; the rule that is looking at
    /// the start of a command can.
    pub(crate) fn bump_as(&mut self, kind: SyntaxKind) {
        self.skip_trivia();
        let Some(token) = self.tokens.get(self.pos) else {
            return;
        };
        let len = token.len;
        self.builder.token(kind, len);
        self.pos += 1;
    }

    /// Take the token under the cursor even if it is trivia. Words are built with this.
    pub(crate) fn bump_raw(&mut self) {
        let Some(token) = self.tokens.get(self.pos) else {
            return;
        };
        let (kind, len) = (token.kind, token.len);
        self.builder.token(kind, len);
        self.pos += 1;
    }

    /// Emit part of the token under the cursor, without moving past the rest of it.
    ///
    /// The lexer has no reason to split `x=1`; the grammar does, because a name, an `=` and a
    /// value are three different things. Every call must be closed by [`Parser::end_token`].
    pub(crate) fn bump_slice(&mut self, kind: SyntaxKind, len: u32) {
        if len > 0 {
            self.builder.token(kind, len);
        }
    }

    /// Move past a token that was emitted in slices, making up any part that was not.
    pub(crate) fn end_token(&mut self) {
        let expected = self.starts.get(self.pos + 1).copied();
        self.pos += 1;
        // If the slices did not add up, the tree would silently lose the difference. Take it as
        // unknown text instead: a wrong tree is recoverable, a wrong offset is not.
        if let Some(end) = expected
            && self.builder.offset() < end
        {
            self.builder
                .token(SyntaxKind::Unknown, end - self.builder.offset());
        }
    }

    /// Whether the next word is written exactly as `text`, and is a whole word.
    ///
    /// `if` is a keyword; `iffy` is not, and neither is `if"x"`, which is the word `ifx` written
    /// oddly. Adjacency is what tells them apart.
    pub(crate) fn at_word_exactly(&self, text: &str) -> bool {
        let Some(index) = self.significant(0) else {
            return false;
        };
        if self.tokens[index].kind != SyntaxKind::Text || self.token_text(index) != text {
            return false;
        }
        !self.tokens.get(index + 1).is_some_and(|next| {
            next.kind.is_word_piece() || next.kind == SyntaxKind::LineContinuation
        })
    }

    /// Whether the next word is the reserved word `kind` is written as.
    pub(crate) fn at_keyword(&self, kind: SyntaxKind) -> bool {
        kind.static_text()
            .is_some_and(|text| self.at_word_exactly(text))
    }

    /// Take the next word as a reserved word.
    pub(crate) fn eat_keyword(&mut self, kind: SyntaxKind) -> bool {
        if self.at_keyword(kind) {
            self.bump_as(kind);
            return true;
        }
        false
    }

    /// Take the next token if it is what was expected, and say whether it was.
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            return true;
        }
        false
    }

    // ---- building ----

    /// Open a node, letting any trivia before it fall outside.
    pub(crate) fn start(&mut self, kind: SyntaxKind) {
        self.skip_trivia();
        self.builder.start(kind);
    }

    /// Open a node exactly here, trivia and all. Used inside a word, where there is none.
    pub(crate) fn open(&mut self, kind: SyntaxKind) {
        self.builder.start(kind);
    }

    pub(crate) fn finish_node(&mut self) {
        self.builder.finish();
    }

    pub(crate) fn checkpoint(&mut self) -> crate::tree::Checkpoint {
        self.skip_trivia();
        self.builder.checkpoint()
    }

    pub(crate) fn start_at(&mut self, at: crate::tree::Checkpoint, kind: SyntaxKind) {
        self.builder.start_at(at, kind);
    }

    // ---- complaining ----

    pub(crate) fn error(&mut self, message: impl Into<String>) {
        let at = self.position();
        let end = at.saturating_add(self.peek_text(0).len() as u32);
        self.errors.push(Error::new(Span::new(at, end), message));
    }

    pub(crate) fn push_error(&mut self, error: Error) {
        self.errors.push(error);
    }
}

#[cfg(test)]
mod tests;
