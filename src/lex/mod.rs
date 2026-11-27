//! Turning text into a flat run of tokens.
//!
//! The output covers the whole input with no gaps and no overlaps — that is the contract, and it
//! is what lets the parser hand lengths straight to the tree builder.
//!
//! The kinds here are *advice*. A word is lexed as text whatever it spells, and the parser decides
//! that this particular `if` is a reserved word and that one is an argument to `echo`. Shell cannot
//! be tokenized without that split: the same letters are a keyword or a filename depending on
//! where they appear, and only the grammar knows where it is.

mod cond;
mod cursor;
mod enclosed;
mod expand;
mod heredoc;
mod operator;
mod position;
mod subscript;
mod unquote;
mod word;

use crate::tree::SyntaxKind;
use cond::CondState;
use cursor::Cursor;
pub use enclosed::enclosed;
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
    /// Inside `$((...))` or an `((...))` command, counting nested parentheses to find the end.
    ///
    /// `command` is the second: what follows its `))` is a new word. Its depth starts at -1,
    /// because the second `(` of the opener has still to be read.
    Arithmetic { depth: i32, command: bool },
    /// Inside `$(...)`, `<(...)` or `>(...)`, counting parentheses to find the one that closes it.
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
    /// Inside an `extglob` group — `@(a|b)`, `!(*.txt)` — counting parentheses to find its end.
    ///
    /// The group is part of the word it is in, spaces and `|` included, so it cannot be read in
    /// normal mode, where both end a word. Quotes and expansions inside keep their meaning.
    ExtGlob { depth: i32 },
    /// Inside `[[ ... ]]`: ordinary shell, except for the operand of `=~` and a leading `!(`.
    Cond(CondState),
    /// The operand of `=~`, where a parenthesised group is the regex's own, spaces and all.
    Regex { depth: i32 },
    /// The subscript of an assignment, `a[$i + 1]=x`, which runs to its matching `]`.
    Subscript { depth: i32 },
}

impl Mode {
    /// What opens this mode, as it is written.
    const fn opener(self) -> &'static str {
        match self {
            Self::Normal => "",
            Self::DoubleQuoted => "\"",
            Self::Brace { .. } => "${",
            Self::Arithmetic { command: false, .. } => "$((",
            Self::Arithmetic { command: true, .. } => "((",
            Self::CommandSub { .. } => "$(",
            Self::Backtick => "`",
            Self::ExtGlob { .. } | Self::Regex { .. } => "(",
            Self::Cond(_) => "[[",
            Self::Subscript { .. } => "[",
        }
    }

    /// Whether this mode holds ordinary shell, read a command at a time.
    const fn holds_commands(self) -> bool {
        matches!(self, Self::CommandSub { .. } | Self::Backtick)
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
    /// Set by a token that closes a substitution: the word it sits in goes on after it.
    continues_word: bool,
    /// Whether the next word would begin a command.
    ///
    /// Starts true: the first thing in a script, and inside a `$(`, is a command. Asked inside a
    /// command substitution to count `case`, and everywhere to read `!(`: where a command can
    /// start it is `!` and a subshell, anywhere else an `extglob` group.
    command_can_start: bool,
    /// Whether the next word is a `case` pattern: after `in`, `;;`, `;&` or `;;&`.
    ///
    /// A pattern follows a newline too, where a command could otherwise start, so `!(x))` there is
    /// a group rather than a negated subshell.
    pattern_next: bool,
    /// Whether a word starting here could be an assignment: at a command start, or after one.
    assign_ok: bool,
    /// Whether the word being read looks like an assignment, so the next one may be one too.
    word_assigns: bool,
    /// [`Self::word_assigns`] of each word a substitution interrupted, innermost last.
    saved_assigns: Vec<bool>,
    /// Set when a name has just been read whose `[` opens an assignment subscript.
    subscript_next: bool,
    /// Whether the last word was a `for` at a command start, so a `((` next is its header.
    for_next: bool,
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
            continues_word: false,
            command_can_start: true,
            pattern_next: false,
            assign_ok: true,
            word_assigns: false,
            saved_assigns: Vec::new(),
            subscript_next: false,
            for_next: false,
        }
    }

    fn run(mut self) -> Lexing {
        while !self.cursor.is_eof() {
            self.step();
        }
        // Whatever is still on the stack was opened and never closed. A `[[` is the parser's to
        // report: it decided the test was one, and it knows where it began.
        for (mode, at) in self.modes.iter().skip(1) {
            if matches!(mode, Mode::Cond(_)) {
                continue;
            }
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

    /// Read one token, emit it, and keep the here-document bookkeeping true to it.
    fn step(&mut self) -> SyntaxKind {
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

        let text = self.token_text();
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
        kind
    }

    fn token(&mut self) -> SyntaxKind {
        match self.mode() {
            Mode::DoubleQuoted => self.quoted_piece(),
            Mode::Brace { stage } => self.brace_piece(stage),
            Mode::Arithmetic { depth: -1, .. } => {
                self.cursor.bump();
                self.bump_arith_depth(1);
                SyntaxKind::LParen
            }
            Mode::Arithmetic { .. } => {
                if self.cursor.peek() == Some(')') {
                    return self.close_arithmetic();
                }
                self.arithmetic_run()
            }
            Mode::CommandSub { depth, cases } => {
                if self.cursor.peek() == Some(')') {
                    self.cursor.bump();
                    let closes = depth == 0 && cases == 0;
                    if closes {
                        self.pop_mode();
                        self.continues_word = true;
                    } else if depth > 0 {
                        self.bump_sub_depth(-1);
                    }
                    // A `case` item's body begins after its pattern's `)`, so a `case` nested
                    // directly inside another one is at a command start here. The substitution's
                    // own `)` is the other way round: the word it was in goes on. This branch
                    // returns early, so it has to say so itself.
                    self.command_can_start = !closes;
                    if !closes {
                        self.assign_ok = true;
                    }
                    // With a `case` open and nothing else nested, this closed a pattern and the
                    // substitution is still going: neither counter moves.
                    return SyntaxKind::RParen;
                }
                // **Not gated on `at_word_start`.** That flag is about the word *outside* — in
                // `x=$(case …)` the `$(` is a piece of the word `x=$(…)`, so it reads false for the
                // very first token inside, which is exactly where a `case` most often is.
                let at_command_start = self.command_can_start;
                let kind = self.command_token();
                // Anything that opens a parenthesis owes a `)` that is not the closing one. A
                // process substitution opens a mode of its own, which counts for itself.
                if kind == SyntaxKind::LParen {
                    self.bump_sub_depth(1);
                }
                let text = self.token_text();
                self.note_case_word(text, at_command_start);
                kind
            }
            Mode::ExtGlob { depth } => self.extglob_piece(depth),
            Mode::Cond(state) => self.cond_token(state),
            Mode::Regex { depth } => self.regex_piece(depth),
            Mode::Subscript { depth } => self.subscript_piece(depth),
            Mode::Normal | Mode::Backtick => self.command_token(),
        }
    }

    /// A token of ordinary shell, with the position bookkeeping that follows from it.
    fn command_token(&mut self) -> SyntaxKind {
        let opens_test = self.command_can_start && !self.pattern_next;
        let word_start = self.at_word_start;
        // `((` where a command can start, or after `for`, is arithmetic when its parentheses
        // close together — read as arithmetic, so `((x = 1 << 2))` holds no here-document.
        if (opens_test || self.for_next) && self.cursor.starts_with("((") && self.arithmetic_ahead()
        {
            self.cursor.bump();
            self.push_mode(Mode::Arithmetic {
                depth: -1,
                command: true,
            });
            self.for_next = false;
            return SyntaxKind::LParen;
        }
        let kind = self.normal_token();
        let text = self.token_text();
        self.note_position(kind, text, word_start);
        if kind == SyntaxKind::Text && word_start && opens_test && text == "[[" {
            self.open_test();
        }
        if !kind.is_trivia() {
            self.for_next = kind == SyntaxKind::Text && word_start && opens_test && text == "for";
        }
        kind
    }

    /// The text of the token being read, from where it started to the cursor.
    fn token_text(&self) -> &'a str {
        let source: &'a str = self.text;
        source
            .get(self.token_start as usize..self.cursor.offset() as usize)
            .unwrap_or("")
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

    /// Whether the word just read ends here: what follows is a blank, an operator or nothing.
    pub(crate) fn word_ends_here(&self) -> bool {
        self.cursor
            .peek()
            .is_none_or(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n') || operator::starts_one(ch))
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
        // The first thing inside a command substitution is a command, wherever its `$(` was.
        if mode.holds_commands() {
            self.command_can_start = true;
            self.pattern_next = false;
            self.saved_assigns.push(self.word_assigns);
            self.assign_ok = true;
            self.word_assigns = false;
        }
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
        if self.modes.len() > 1
            && let Some((mode, _)) = self.modes.pop()
            && mode.holds_commands()
        {
            self.word_assigns = self.saved_assigns.pop().unwrap_or(false);
        }
    }

    pub(crate) fn set_brace_stage(&mut self, stage: BraceStage) {
        if let Some((Mode::Brace { stage: current }, _)) = self.modes.last_mut() {
            *current = stage;
        }
    }

    pub(crate) fn arith_depth(&self) -> i32 {
        match self.mode() {
            Mode::Arithmetic { depth, .. } => depth,
            _ => 0,
        }
    }

    pub(crate) fn bump_arith_depth(&mut self, by: i32) {
        if let Some((Mode::Arithmetic { depth, .. }, _)) = self.modes.last_mut() {
            *depth = depth.saturating_add(by).max(0);
        }
    }

    /// Set the depth of the innermost mode that counts its own brackets.
    pub(crate) fn set_depth(&mut self, to: i32) {
        if let Some((
            Mode::ExtGlob { depth } | Mode::Regex { depth } | Mode::Subscript { depth },
            _,
        )) = self.modes.last_mut()
        {
            *depth = to;
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
        //
        // A substitution holds commands, so its first word starts inside it; the word around it
        // goes on after the token that closes it. `$(# note)` is a comment, `$((1))#x` is one word.
        if kind != SyntaxKind::LineContinuation {
            let continues = std::mem::take(&mut self.continues_word);
            self.at_word_start = match kind {
                SyntaxKind::DollarParen | SyntaxKind::ProcSubIn | SyntaxKind::ProcSubOut => true,
                _ => !kind.is_word_piece() && !continues,
            };
        }
        self.out.push(Lexed { kind, len });
    }
}

#[cfg(test)]
mod tests;
