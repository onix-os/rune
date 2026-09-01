//! Turning text into a flat run of tokens.
//!
//! The output covers the whole input with no gaps and no overlaps — that is the contract, and it
//! is what lets the parser hand lengths straight to the tree builder.
//!
//! The kinds here are *advice*. A word is lexed as text whatever it spells, and the parser decides
//! that this particular `if` is a reserved word and that one is an argument to `echo`. Shell cannot
//! be tokenized without that split: the same letters are a keyword or a filename depending on
//! where they appear, and only the grammar knows where it is.

mod cursor;
mod expand;
mod heredoc;
mod operator;
mod word;

use crate::tree::SyntaxKind;
use cursor::Cursor;
use heredoc::Heredoc;

/// One token: what it is, and how many bytes of source it takes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lexed {
    pub kind: SyntaxKind,
    pub len: u32,
}

/// Split `text` into tokens whose lengths sum to its length.
pub fn lex(text: &str) -> Vec<Lexed> {
    Lexer::new(text).run()
}

/// How far through a `${...}` the lexer is.
///
/// The same characters mean different things at each point: the `#` in `${#x}` asks for a length
/// and the one in `${x#y}` strips a prefix, and past the operator `#` is an ordinary character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BraceStage {
    /// Before the parameter name, where a `#` or `!` prefix can still appear.
    Start,
    /// The name has been read; an operator or a subscript may follow.
    Name,
    /// Between `[` and `]`. What is in there is arithmetic, so `-` and `+` are not operators on
    /// the parameter: `${h[i+1]}` indexes, it does not default.
    Subscript,
    /// Past the operator. What is left is an ordinary word.
    Operand,
}

/// Where the lexer is, for the decisions that cannot be made character by character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Ordinary shell text. Also what is inside `$(...)`, which is ordinary shell.
    Normal,
    /// Inside `"..."`, where whitespace does not separate and most punctuation is inert.
    DoubleQuoted,
    /// Inside `${...}`.
    Brace { stage: BraceStage },
    /// Inside `$((...))`, counting nested parentheses to find the end.
    Arithmetic { depth: i32 },
    /// Inside `$(...)`, counting parentheses to find the one that closes it.
    ///
    /// A command substitution holds ordinary shell wherever it appears, so this has to be a mode
    /// of its own rather than a continuation of the one around it: in `"$(ls)"` the quoting stops
    /// at the `$(` and starts again after the `)`.
    CommandSub { depth: i32 },
    /// Inside `` `...` ``, which is a command substitution written the old way.
    Backtick,
}

pub(crate) struct Lexer<'a> {
    pub(crate) cursor: Cursor<'a>,
    text: &'a str,
    out: Vec<Lexed>,
    modes: Vec<Mode>,
    /// Bodies owed, to be read at the end of the line that asked for them.
    heredocs: Vec<Heredoc>,
    /// Set while the word naming a here-document delimiter is being collected; the flag inside is
    /// whether the operator was `<<-`.
    awaiting_delimiter: Option<bool>,
    delimiter_text: String,
    /// Whether the next token would start a word rather than continue one.
    ///
    /// `#` opens a comment only here, and `~` names a home directory only here: `echo a#b` prints
    /// `a#b`, and `echo a~b` is not a path.
    pub(crate) at_word_start: bool,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            cursor: Cursor::new(text),
            text,
            out: Vec::new(),
            modes: vec![Mode::Normal],
            heredocs: Vec::new(),
            awaiting_delimiter: None,
            delimiter_text: String::new(),
            at_word_start: true,
        }
    }

    fn run(mut self) -> Vec<Lexed> {
        let source: &'a str = self.text;
        while !self.cursor.is_eof() {
            let start = self.cursor.offset();
            let mut kind = self.token();
            if self.cursor.offset() == start {
                // A branch that consumed nothing would spin here forever. Take a character and
                // call it unknown; the parser reports it and carries on.
                self.cursor.bump();
                kind = SyntaxKind::Unknown;
            }
            let end = self.cursor.offset();
            self.emit(kind, end - start);

            let text = source.get(start as usize..end as usize).unwrap_or("");
            self.collect_delimiter(kind, text);
            match kind {
                SyntaxKind::LessLess => self.expect_delimiter(false),
                SyntaxKind::LessLessDash => self.expect_delimiter(true),
                SyntaxKind::Newline => {
                    self.finish_delimiter();
                    self.take_heredoc_bodies();
                }
                _ => {}
            }
        }
        self.out
    }

    fn token(&mut self) -> SyntaxKind {
        match self.mode() {
            Mode::DoubleQuoted => self.quoted_piece(),
            Mode::Brace { stage } => self.brace_piece(stage),
            Mode::Arithmetic { .. } => {
                if self.cursor.peek() == Some(')') {
                    return self.close_arithmetic();
                }
                self.arithmetic_run()
            }
            Mode::CommandSub { depth } => {
                if self.cursor.peek() == Some(')') {
                    self.cursor.bump();
                    if depth == 0 {
                        self.pop_mode();
                    } else {
                        self.bump_sub_depth(-1);
                    }
                    return SyntaxKind::RParen;
                }
                let kind = self.normal_token();
                // Anything that opens a parenthesis owes a `)` that is not the closing one.
                if matches!(
                    kind,
                    SyntaxKind::LParen | SyntaxKind::ProcSubIn | SyntaxKind::ProcSubOut
                ) {
                    self.bump_sub_depth(1);
                }
                kind
            }
            Mode::Normal | Mode::Backtick => self.normal_token(),
        }
    }

    fn normal_token(&mut self) -> SyntaxKind {
        match self.cursor.peek() {
            Some('\n') => {
                self.cursor.bump();
                SyntaxKind::Newline
            }
            Some(' ' | '\t' | '\r') => {
                self.cursor.eat_while(|ch| matches!(ch, ' ' | '\t' | '\r'));
                SyntaxKind::Whitespace
            }
            Some('\\') if self.cursor.peek_at(1) == Some('\n') => {
                self.cursor.eat("\\\n");
                SyntaxKind::LineContinuation
            }
            Some('#') if self.at_word_start => {
                self.cursor.eat_while(|ch| ch != '\n');
                SyntaxKind::Comment
            }
            Some(ch) if operator::starts_one(ch) => self.operator(),
            Some(_) => self.word_piece(),
            None => SyntaxKind::Unknown,
        }
    }

    pub(crate) fn mode(&self) -> Mode {
        self.modes.last().copied().unwrap_or(Mode::Normal)
    }

    pub(crate) fn push_mode(&mut self, mode: Mode) {
        self.modes.push(mode);
    }

    /// Leave the innermost mode. The outermost cannot be left.
    pub(crate) fn pop_mode(&mut self) {
        if self.modes.len() > 1 {
            self.modes.pop();
        }
    }

    pub(crate) fn set_brace_stage(&mut self, stage: BraceStage) {
        if let Some(Mode::Brace { stage: current }) = self.modes.last_mut() {
            *current = stage;
        }
    }

    pub(crate) fn arith_depth(&self) -> i32 {
        match self.mode() {
            Mode::Arithmetic { depth } => depth,
            _ => 0,
        }
    }

    pub(crate) fn bump_arith_depth(&mut self, by: i32) {
        if let Some(Mode::Arithmetic { depth }) = self.modes.last_mut() {
            *depth = depth.saturating_add(by).max(0);
        }
    }

    pub(crate) fn bump_sub_depth(&mut self, by: i32) {
        if let Some(Mode::CommandSub { depth }) = self.modes.last_mut() {
            *depth = depth.saturating_add(by).max(0);
        }
    }

    fn emit(&mut self, kind: SyntaxKind, len: u32) {
        // A word runs on while its pieces are adjacent; trivia and operators end it. A line
        // continuation is neither: the shell removes it, so `ab\<newline>cd` is the one word
        // `abcd` and the `#` in `ab\<newline>#c` is not a comment.
        if kind != SyntaxKind::LineContinuation {
            self.at_word_start = !kind.is_word_piece();
        }
        self.out.push(Lexed { kind, len });
    }
}

#[cfg(test)]
mod tests;
