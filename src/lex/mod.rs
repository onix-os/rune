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

/// Something that was still open when the input ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unclosed {
    /// What opened it, written the way it appears in the source.
    pub opener: &'static str,
    /// Where the opener starts.
    pub at: u32,
}

/// Everything reading the text produced.
#[derive(Debug, Clone, Default)]
pub struct Lexing {
    pub tokens: Vec<Lexed>,
    /// Constructs left open at the end of the input, outermost first.
    ///
    /// An unterminated quote is invisible in the token stream — the run simply reaches the end —
    /// so the only thing that can report one is the reader that was inside it.
    pub unclosed: Vec<Unclosed>,
}

/// Split `text` into tokens whose lengths sum to its length.
pub fn lex(text: &str) -> Lexing {
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
    ///
    /// `cases` is how many `case` constructs are open inside it, because a `case` *pattern* ends
    /// with a `)` that closes nothing: in `"$(case a in a) echo hi;; esac)"` the first `)` is the
    /// pattern's, and counting parentheses alone read it as the end of the substitution — after
    /// which the rest was lexed as the string it was nested in, and both the `case` and the `$(`
    /// were reported unclosed. While a `case` is open, a `)` at depth zero is a pattern's.
    CommandSub { depth: i32, cases: i32 },
    /// Inside `` `...` ``, which is a command substitution written the old way.
    Backtick,
}

impl Mode {
    /// What opens this mode, as it is written.
    const fn opener(self) -> &'static str {
        match self {
            Self::Normal => "",
            Self::DoubleQuoted => "\"",
            Self::Brace { .. } => "${",
            Self::Arithmetic { .. } => "$((",
            Self::CommandSub { .. } => "$(",
            Self::Backtick => "`",
        }
    }
}

pub(crate) struct Lexer<'a> {
    pub(crate) cursor: Cursor<'a>,
    text: &'a str,
    out: Vec<Lexed>,
    /// Each open mode, with where the token that opened it began.
    modes: Vec<(Mode, u32)>,
    /// Where the token being read starts, so a mode can record what opened it.
    token_start: u32,
    unclosed: Vec<Unclosed>,
    /// Bodies owed, to be read at the end of the line that asked for them.
    heredocs: Vec<Heredoc>,
    /// Set while the word naming a here-document delimiter is being collected; the flag inside is
    /// whether the operator was `<<-`.
    awaiting_delimiter: Option<(bool, u32)>,
    delimiter_text: String,
    /// Whether the next token would start a word rather than continue one.
    ///
    /// `#` opens a comment only here, and `~` names a home directory only here: `echo a#b` prints
    /// `a#b`, and `echo a~b` is not a path.
    pub(crate) at_word_start: bool,
    /// Whether the next word inside a command substitution would begin a command.
    ///
    /// Starts true: the first thing inside a `$(` is a command. Only maintained while lexing a
    /// command substitution, which is the only place it is asked about.
    command_can_start: bool,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            cursor: Cursor::new(text),
            text,
            out: Vec::new(),
            modes: vec![(Mode::Normal, 0)],
            token_start: 0,
            unclosed: Vec::new(),
            heredocs: Vec::new(),
            awaiting_delimiter: None,
            delimiter_text: String::new(),
            at_word_start: true,
            command_can_start: true,
        }
    }

    fn run(mut self) -> Lexing {
        let source: &'a str = self.text;
        while !self.cursor.is_eof() {
            let start = self.cursor.offset();
            self.token_start = start;
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
        // Whatever is still on the stack was opened and never closed.
        for (mode, at) in self.modes.iter().skip(1) {
            self.unclosed.push(Unclosed {
                opener: mode.opener(),
                at: *at,
            });
        }
        self.unclosed.sort_by_key(|open| open.at);
        Lexing {
            tokens: self.out,
            unclosed: self.unclosed,
        }
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
            Mode::CommandSub { depth, cases } => {
                if self.cursor.peek() == Some(')') {
                    self.cursor.bump();
                    if depth == 0 && cases == 0 {
                        self.pop_mode();
                    } else if depth > 0 {
                        self.bump_sub_depth(-1);
                    }
                    // A `case` item's body begins after its pattern's `)`, so a `case` nested
                    // directly inside another one is at a command start here. This branch returns
                    // early, so it has to say so itself.
                    self.command_can_start = true;
                    // With a `case` open and nothing else nested, this closed a pattern and the
                    // substitution is still going: neither counter moves.
                    return SyntaxKind::RParen;
                }
                // **Not gated on `at_word_start`.** That flag is about the word *outside* — in
                // `x=$(case …)` the `$(` is a piece of the word `x=$(…)`, so it reads false for the
                // very first token inside, which is exactly where a `case` most often is.
                let at_command_start = self.command_can_start;
                let started_at = self.token_start;
                let kind = self.normal_token();
                // Anything that opens a parenthesis owes a `)` that is not the closing one.
                if matches!(
                    kind,
                    SyntaxKind::LParen | SyntaxKind::ProcSubIn | SyntaxKind::ProcSubOut
                ) {
                    self.bump_sub_depth(1);
                }
                let text = self
                    .text
                    .get(started_at as usize..self.cursor.offset() as usize)
                    .unwrap_or("");
                if !kind.is_trivia() {
                    self.command_can_start = Self::ends_a_command(kind, text);
                }
                self.note_case_word(text, at_command_start);
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
        self.modes.last().map_or(Mode::Normal, |(mode, _)| *mode)
    }

    /// Whether any backquote substitution is open, however deeply the modes are stacked.
    ///
    /// Not [`Self::mode`]: a here-document's body is read at the end of the line that asked for it,
    /// and by then the word around the `<<` may have opened modes of its own.
    pub(crate) fn inside_backtick(&self) -> bool {
        self.modes.iter().any(|(mode, _)| *mode == Mode::Backtick)
    }

    pub(crate) fn push_mode(&mut self, mode: Mode) {
        self.modes.push((mode, self.token_start));
    }

    /// Record something that ran to the end of the input without being closed.
    pub(crate) fn note_unclosed(&mut self, opener: &'static str, at: u32) {
        self.unclosed.push(Unclosed { opener, at });
    }

    /// Where the token being read starts.
    pub(crate) const fn token_start(&self) -> u32 {
        self.token_start
    }

    /// Leave the innermost mode. The outermost cannot be left.
    pub(crate) fn pop_mode(&mut self) {
        if self.modes.len() > 1 {
            self.modes.pop();
        }
    }

    pub(crate) fn set_brace_stage(&mut self, stage: BraceStage) {
        if let Some((Mode::Brace { stage: current }, _)) = self.modes.last_mut() {
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
        if let Some((Mode::Arithmetic { depth }, _)) = self.modes.last_mut() {
            *depth = depth.saturating_add(by).max(0);
        }
    }

    pub(crate) fn bump_sub_depth(&mut self, by: i32) {
        if let Some((Mode::CommandSub { depth, .. }, _)) = self.modes.last_mut() {
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

    /// Whether a word starting here would be the first word of a command.
    ///
    /// Only ever an approximation, and only used to decide whether a `case` is the keyword — see
    /// [`Self::note_case_word`]. Everything listed ends a command, so what follows begins one.
    /// Whether a token of this kind and text leaves a place where a command can begin after it.
    ///
    /// **The reserved words are checked by text, because to the lexer they are not reserved.** `{`,
    /// `do` and `then` all arrive as `Text` — the tree shows `LBrace` because the *parser* decides
    /// that, long after this runs — so a rule written on kinds alone saw `f() { case …` as a `case`
    /// in the middle of a command and did not count it.
    fn ends_a_command(kind: SyntaxKind, text: &str) -> bool {
        matches!(
            kind,
            SyntaxKind::Newline
                | SyntaxKind::Semi
                | SyntaxKind::SemiSemi
                | SyntaxKind::SemiAmp
                | SyntaxKind::SemiSemiAmp
                | SyntaxKind::Amp
                | SyntaxKind::AndAnd
                | SyntaxKind::PipePipe
                | SyntaxKind::Pipe
                | SyntaxKind::PipeAmp
                | SyntaxKind::LParen
                // A `case` *pattern* ends with one, and the item's body begins after it — which is
                // how a `case` nested directly inside another one is counted.
                | SyntaxKind::RParen
                | SyntaxKind::DollarParen
                | SyntaxKind::Backtick
        ) || matches!(text, "{" | "do" | "then" | "else" | "elif" | "!")
    }

    /// Count a `case` that opened or closed inside a command substitution.
    ///
    /// **Position matters for `case` and not for `esac`.** `"$(echo case)"` must still close where
    /// it closes, so the opener counts only where a command can begin; `esac` is asked for only
    /// when a `case` is already open, so a word of that spelling anywhere else cannot take one
    /// away. Neither is a reserved word to the lexer — the parser decides that — so this is a
    /// count kept for the sole purpose of knowing whether a `)` closes a pattern or a
    /// substitution.
    fn note_case_word(&mut self, text: &str, at_command_start: bool) {
        match text {
            "case" if at_command_start => self.bump_sub_cases(1),
            "esac" => self.bump_sub_cases(-1),
            _ => {}
        }
    }

    fn bump_sub_cases(&mut self, by: i32) {
        if let Some((Mode::CommandSub { cases, .. }, _)) = self.modes.last_mut() {
            *cases = cases.saturating_add(by).max(0);
        }
    }
}

#[cfg(test)]
mod tests;
