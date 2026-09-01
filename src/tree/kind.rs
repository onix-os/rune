//! What a node or a token in the tree is.

/// The tag on every element of the tree.
///
/// Tokens and nodes share one enum so that a child can be either without a wrapper discriminant.
/// **The two groups must stay contiguous and in this order**: [`SyntaxKind::is_node`] is a
/// comparison against [`SyntaxKind::Script`], the first node kind, and the test at the bottom of
/// this file guards the boundary.
///
/// The word-piece kinds are deliberately thin. What exactly a `$` opens is the tokenizer's
/// decision, and it gets to name those kinds when it is written rather than having them guessed
/// for it here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // ---- trivia ----
    Whitespace,
    Newline,
    Comment,
    /// A backslash immediately before a newline, which the shell removes.
    LineContinuation,

    // ---- word pieces ----
    /// A run of ordinary characters.
    Text,
    /// A backslash and the character it escapes.
    Escaped,
    /// A whole `'...'`, which has no interior structure.
    SingleQuoted,
    /// Either end of a `"..."`. Which end it is follows from where it sits.
    DoubleQuote,
    /// A `$` that expands nothing — the last character of a word, or before a space.
    Dollar,
    /// `$name`.
    DollarName,
    /// One of `$?`, `$$`, `$!`, `$#`, `$*`, `$@`, `$-`, `$_`, or `$0` through `$9`.
    DollarSpecial,
    /// The `${` that opens a parameter expansion.
    DollarBrace,
    /// The `$(` that opens a command substitution. What is inside is ordinary shell.
    DollarParen,
    /// The `$((` that opens an arithmetic expansion.
    DollarParenParen,
    /// An operator inside `${...}`, such as `:-`, `##`, or `%%`.
    ParamOp,
    LBracket,
    RBracket,
    /// Either end of a `` `...` ``.
    Backtick,
    /// The `<(` that opens a process substitution.
    ProcSubIn,
    /// The `>(` that opens a process substitution.
    ProcSubOut,
    /// A `~` or `~user` at the start of a word.
    Tilde,
    /// The body of a here-document, from the line after `<<DELIM` to the line before its end.
    HeredocText,
    /// The line that closes a here-document, with its newline.
    HeredocEnd,
    /// A byte the tokenizer could not classify.
    Unknown,

    // ---- operators ----
    AndAnd,
    PipePipe,
    Pipe,
    PipeAmp,
    Semi,
    SemiSemi,
    SemiAmp,
    SemiSemiAmp,
    Amp,
    LParen,
    RParen,
    /// The `))` that closes a `$((`.
    ///
    /// One token rather than two so that an arithmetic expansion nested inside a command
    /// substitution does not look like it closed the substitution.
    RParenRParen,
    LBrace,
    RBrace,
    LBracketBracket,
    RBracketBracket,
    Less,
    Great,
    GreatGreat,
    LessGreat,
    LessAmp,
    GreatAmp,
    LessLess,
    LessLessDash,
    LessLessLess,
    GreatPipe,
    AmpGreat,
    AmpGreatGreat,
    Equal,
    PlusEqual,
    Bang,

    // ---- reserved words ----
    If,
    Then,
    Elif,
    Else,
    Fi,
    While,
    Until,
    For,
    In,
    Do,
    Done,
    Case,
    Esac,
    Function,
    Select,
    Time,
    Coproc,

    // ---- nodes ----
    /// The root. Everything else hangs off this.
    Script,
    CommandList,
    ListItem,
    AndOrList,
    Pipeline,
    SimpleCommand,
    Assignment,
    /// The value of an assignment written as `(a b c)`.
    ArrayValue,
    Word,
    /// `$(...)` or `` `...` ``, holding a whole script of its own.
    CommandSubstitution,
    /// `<(...)` or `>(...)`, which is a word: it expands to the name of a pipe.
    ProcessSubstitution,
    /// `${...}`.
    ParameterExpansion,
    /// The `[...]` of `${a[i]}`.
    Subscript,
    /// `$((...))`.
    ArithmeticExpansion,
    Redirect,
    HeredocBody,
    FunctionDef,
    IfCommand,
    ElifClause,
    ElseClause,
    WhileCommand,
    UntilCommand,
    ForCommand,
    ArithForCommand,
    /// `select name in words; do ... done`.
    SelectCommand,
    CaseCommand,
    CaseItem,
    CasePattern,
    Subshell,
    Group,
    ArithCommand,
    CondCommand,
    /// A region the parser could not make sense of. Its children hold the text verbatim.
    Error,
}

impl SyntaxKind {
    pub const fn is_node(self) -> bool {
        (self as u16) >= (Self::Script as u16)
    }

    pub const fn is_token(self) -> bool {
        !self.is_node()
    }

    /// Whether a token of this kind is part of a word.
    ///
    /// Adjacent pieces are one word: `pre"$mid"post` is a single argument. Nothing but adjacency
    /// joins them, which is why this is a property of the kind rather than of the grammar.
    pub const fn is_word_piece(self) -> bool {
        matches!(
            self,
            Self::Text
                | Self::Escaped
                | Self::SingleQuoted
                | Self::DoubleQuote
                | Self::Dollar
                | Self::DollarName
                | Self::DollarSpecial
                | Self::DollarBrace
                | Self::DollarParen
                | Self::DollarParenParen
                | Self::Backtick
                | Self::ProcSubIn
                | Self::ProcSubOut
                | Self::Tilde
                | Self::ParamOp
                | Self::LBracket
                | Self::RBracket
                | Self::RBrace
        )
    }

    /// Whether this operator sends a file descriptor somewhere.
    pub const fn is_redirect_operator(self) -> bool {
        matches!(
            self,
            Self::Less
                | Self::Great
                | Self::GreatGreat
                | Self::LessGreat
                | Self::LessAmp
                | Self::GreatAmp
                | Self::LessLess
                | Self::LessLessDash
                | Self::LessLessLess
                | Self::GreatPipe
                | Self::AmpGreat
                | Self::AmpGreatGreat
        )
    }

    /// Present in the tree, absent from the grammar.
    ///
    /// A newline is not in here. In shell it separates commands, which makes it as much a part of
    /// the grammar as `;` — a parser that skipped it would run two lines together.
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::Comment | Self::LineContinuation
        )
    }

    /// The source text of a token that is always written the same way.
    ///
    /// This is what lets an error say ``expected `fi` `` without the parser carrying its own table
    /// of spellings.
    pub const fn static_text(self) -> Option<&'static str> {
        Some(match self {
            Self::AndAnd => "&&",
            Self::PipePipe => "||",
            Self::Pipe => "|",
            Self::PipeAmp => "|&",
            Self::Semi => ";",
            Self::SemiSemi => ";;",
            Self::SemiAmp => ";&",
            Self::SemiSemiAmp => ";;&",
            Self::Amp => "&",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::RParenRParen => "))",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::LBracketBracket => "[[",
            Self::RBracketBracket => "]]",
            Self::Less => "<",
            Self::Great => ">",
            Self::GreatGreat => ">>",
            Self::LessGreat => "<>",
            Self::LessAmp => "<&",
            Self::GreatAmp => ">&",
            Self::LessLess => "<<",
            Self::LessLessDash => "<<-",
            Self::LessLessLess => "<<<",
            Self::GreatPipe => ">|",
            Self::AmpGreat => "&>",
            Self::AmpGreatGreat => "&>>",
            Self::Equal => "=",
            Self::PlusEqual => "+=",
            Self::Bang => "!",
            Self::If => "if",
            Self::Then => "then",
            Self::Elif => "elif",
            Self::Else => "else",
            Self::Fi => "fi",
            Self::While => "while",
            Self::Until => "until",
            Self::For => "for",
            Self::In => "in",
            Self::Do => "do",
            Self::Done => "done",
            Self::Case => "case",
            Self::Esac => "esac",
            Self::Function => "function",
            Self::Select => "select",
            Self::Time => "time",
            Self::Coproc => "coproc",
            _ => return None,
        })
    }

    /// The reserved word this text spells, if it spells one.
    ///
    /// Whether a reserved word is *reserved* where it appears is the parser's business; this only
    /// answers what the letters say.
    pub fn reserved_word(text: &str) -> Option<Self> {
        Some(match text {
            "if" => Self::If,
            "then" => Self::Then,
            "elif" => Self::Elif,
            "else" => Self::Else,
            "fi" => Self::Fi,
            "while" => Self::While,
            "until" => Self::Until,
            "for" => Self::For,
            "in" => Self::In,
            "do" => Self::Do,
            "done" => Self::Done,
            "case" => Self::Case,
            "esac" => Self::Esac,
            "function" => Self::Function,
            "select" => Self::Select,
            "time" => Self::Time,
            "coproc" => Self::Coproc,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boundary_between_tokens_and_nodes_holds() {
        assert!(
            SyntaxKind::Coproc.is_token(),
            "last token kind drifted into the node group"
        );
        assert!(
            SyntaxKind::Script.is_node(),
            "first node kind drifted into the token group"
        );
        assert!(SyntaxKind::Error.is_node());
        assert!(SyntaxKind::Whitespace.is_token());
    }

    #[test]
    fn trivia_is_only_trivia() {
        assert!(SyntaxKind::Comment.is_trivia());
        assert!(SyntaxKind::Whitespace.is_trivia());
        assert!(!SyntaxKind::Text.is_trivia());
        assert!(!SyntaxKind::Semi.is_trivia());
        assert!(
            !SyntaxKind::Newline.is_trivia(),
            "a newline separates commands"
        );
    }

    #[test]
    fn fixed_spellings_round_trip_to_their_kind() {
        for kind in [
            SyntaxKind::If,
            SyntaxKind::Fi,
            SyntaxKind::Done,
            SyntaxKind::Esac,
        ] {
            let text = kind
                .static_text()
                .expect("a reserved word is always spelled the same");
            assert_eq!(SyntaxKind::reserved_word(text), Some(kind));
        }
    }

    #[test]
    fn text_has_no_fixed_spelling() {
        assert_eq!(SyntaxKind::Text.static_text(), None);
        assert_eq!(SyntaxKind::Word.static_text(), None);
        assert_eq!(SyntaxKind::reserved_word("echo"), None);
    }
}
