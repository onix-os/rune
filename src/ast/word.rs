//! Words, and the pieces they are written in.

use super::typed;
use crate::source::Source;
use crate::span::Span;
use crate::tree::{Element, Node, SyntaxKind};

typed! {
    /// One argument, however many pieces it took to write.
    Word => Word
}

typed! {
    /// `$(…)` or `` `…` ``.
    CommandSubstitution => CommandSubstitution
}

typed! {
    /// `<(…)` or `>(…)`.
    ProcessSubstitution => ProcessSubstitution
}

typed! {
    /// `${…}`.
    ParameterExpansion => ParameterExpansion
}

typed! {
    /// `$((…))`.
    ArithmeticExpansion => ArithmeticExpansion
}

/// What a word is made of.
///
/// The distinction that matters is not what each piece says but what the shell will do to it:
/// [`WordPiece::Literal`] survives untouched, [`WordPiece::Quoted`] is protected from splitting and
/// globbing, and the rest are replaced by something that is not known until the script runs.
#[derive(Debug, Clone, Copy)]
pub enum WordPiece<'a> {
    /// Ordinary characters.
    Literal(Span),
    /// `'…'` or a backslash escape — text that is protected.
    Quoted(Span),
    /// A `"` opening or closing a run of protected text.
    DoubleQuote(Span),
    /// `$name` or `$?`.
    Parameter(Span),
    Expansion(ParameterExpansion<'a>),
    Command(CommandSubstitution<'a>),
    Process(ProcessSubstitution<'a>),
    Arithmetic(ArithmeticExpansion<'a>),
    /// `~` or `~user`.
    Tilde(Span),
}

impl<'a> Word<'a> {
    /// The word exactly as written, quoting and all.
    pub fn text(self, source: &Source) -> &str {
        source.slice(self.0.span())
    }

    /// Every piece, in order.
    pub fn pieces(self) -> impl Iterator<Item = WordPiece<'a>> {
        self.0.children().filter_map(|child| match child {
            Element::Node(node) => WordPiece::of_node(node),
            Element::Token(token) => WordPiece::of_token(token.kind(), token.span()),
        })
    }

    /// Whether anything in the word is replaced before the command sees it.
    ///
    /// A word that does not expand is the same string every time it runs, which is what lets a
    /// checker say something definite about it.
    pub fn expands(self) -> bool {
        self.pieces().any(|piece| {
            matches!(
                piece,
                WordPiece::Parameter(_)
                    | WordPiece::Expansion(_)
                    | WordPiece::Command(_)
                    | WordPiece::Process(_)
                    | WordPiece::Arithmetic(_)
                    | WordPiece::Tilde(_)
            )
        })
    }

    /// The word's value, if it is the same every time.
    ///
    /// Quoting is removed; anything that expands gives `None`, because the answer is not knowable
    /// until it runs.
    pub fn literal(self, source: &Source) -> Option<String> {
        let mut out = String::new();
        for piece in self.pieces() {
            match piece {
                WordPiece::Literal(span) => out.push_str(source.slice(span)),
                WordPiece::DoubleQuote(_) => {}
                WordPiece::Quoted(span) => {
                    let text = source.slice(span);
                    if let Some(inner) = text.strip_prefix('\'') {
                        out.push_str(inner.strip_suffix('\'').unwrap_or(inner));
                    } else if let Some(escaped) = text.strip_prefix('\\') {
                        out.push_str(escaped);
                    } else {
                        out.push_str(text);
                    }
                }
                _ => return None,
            }
        }
        Some(out)
    }
}

impl<'a> WordPiece<'a> {
    fn of_node(node: &'a Node) -> Option<Self> {
        Some(match node.kind() {
            SyntaxKind::ParameterExpansion => Self::Expansion(ParameterExpansion(node)),
            SyntaxKind::CommandSubstitution => Self::Command(CommandSubstitution(node)),
            SyntaxKind::ProcessSubstitution => Self::Process(ProcessSubstitution(node)),
            SyntaxKind::ArithmeticExpansion => Self::Arithmetic(ArithmeticExpansion(node)),
            _ => return None,
        })
    }

    fn of_token(kind: SyntaxKind, span: Span) -> Option<Self> {
        Some(match kind {
            SyntaxKind::Text => Self::Literal(span),
            SyntaxKind::SingleQuoted | SyntaxKind::Escaped => Self::Quoted(span),
            SyntaxKind::DoubleQuote => Self::DoubleQuote(span),
            SyntaxKind::DollarName | SyntaxKind::DollarSpecial => Self::Parameter(span),
            SyntaxKind::Tilde => Self::Tilde(span),
            // A `$` that expands nothing is an ordinary character, and so is a stray brace.
            SyntaxKind::Dollar | SyntaxKind::RBrace => Self::Literal(span),
            _ => return None,
        })
    }
}

impl<'a> CommandSubstitution<'a> {
    pub fn body(self) -> Option<super::CommandList<'a>> {
        self.0.nodes().find_map(super::CommandList::cast)
    }

    /// True for `` `…` ``, whose escaping rules differ from `$(…)`.
    pub fn is_backticked(self) -> bool {
        self.0.token(SyntaxKind::Backtick).is_some()
    }
}

impl<'a> ProcessSubstitution<'a> {
    pub fn body(self) -> Option<super::CommandList<'a>> {
        self.0.nodes().find_map(super::CommandList::cast)
    }

    /// True for `<(…)`: the command writes and the caller reads.
    pub fn reads_from_command(self) -> bool {
        self.0.token(SyntaxKind::ProcSubIn).is_some()
    }
}

impl ParameterExpansion<'_> {
    /// The parameter's name.
    pub fn name(self, source: &Source) -> &str {
        self.0
            .tokens()
            .find(|token| token.kind() == SyntaxKind::Text)
            .map_or("", |token| token.text(source))
    }

    /// The operator inside, such as `:-` or `##`.
    pub fn operator(self, source: &Source) -> Option<&str> {
        self.0
            .tokens()
            .find(|token| token.kind() == SyntaxKind::ParamOp)
            .map(|token| token.text(source))
    }

    /// The subscript of `${a[i]}`, without its brackets.
    pub fn subscript(self, source: &Source) -> Option<&str> {
        let node = self.0.node(SyntaxKind::Subscript)?;
        let text = source.slice(node.span());
        let text = text.strip_prefix('[').unwrap_or(text);
        Some(text.strip_suffix(']').unwrap_or(text))
    }
}

impl ArithmeticExpansion<'_> {
    /// The expression, still as source text.
    pub fn expression(self, source: &Source) -> &str {
        let text = source.slice(self.0.span());
        let text = text.strip_prefix("$((").unwrap_or(text);
        text.strip_suffix("))").unwrap_or(text)
    }
}
