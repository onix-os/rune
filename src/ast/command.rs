//! What a statement actually runs.

use super::compound::{
    ArithCommand, CaseCommand, CondCommand, ForCommand, Group, IfCommand, LoopCommand, Subshell,
};
use super::typed;
use super::word::Word;
use crate::source::Source;
use crate::span::Span;
use crate::tree::{Node, SyntaxKind};

typed! {
    /// `name=value ... command arg ... >file`.
    SimpleCommand => SimpleCommand
}

typed! {
    /// `name=value`, or `name+=value`, or `name=(a b c)`.
    Assignment => Assignment
}

typed! {
    /// `2>&1`, `>file`, `<<EOF`.
    Redirect => Redirect
}

typed! {
    /// `name() { ... }`.
    FunctionDef => FunctionDef
}

/// Every kind of command, as one thing to match on.
#[derive(Debug, Clone, Copy)]
pub enum Command<'a> {
    Simple(SimpleCommand<'a>),
    If(IfCommand<'a>),
    /// `while` and `until`, which differ only in the sense of the test.
    Loop(LoopCommand<'a>),
    For(ForCommand<'a>),
    Case(CaseCommand<'a>),
    Subshell(Subshell<'a>),
    Group(Group<'a>),
    Arithmetic(ArithCommand<'a>),
    Conditional(CondCommand<'a>),
    Function(FunctionDef<'a>),
}

impl<'a> Command<'a> {
    pub fn cast(node: &'a Node) -> Option<Self> {
        Some(match node.kind() {
            SyntaxKind::SimpleCommand => Self::Simple(SimpleCommand(node)),
            SyntaxKind::IfCommand => Self::If(IfCommand(node)),
            SyntaxKind::WhileCommand | SyntaxKind::UntilCommand => Self::Loop(LoopCommand(node)),
            SyntaxKind::ForCommand | SyntaxKind::ArithForCommand | SyntaxKind::SelectCommand => {
                Self::For(ForCommand(node))
            }
            SyntaxKind::CaseCommand => Self::Case(CaseCommand(node)),
            SyntaxKind::Subshell => Self::Subshell(Subshell(node)),
            SyntaxKind::Group => Self::Group(Group(node)),
            SyntaxKind::ArithCommand => Self::Arithmetic(ArithCommand(node)),
            SyntaxKind::CondCommand => Self::Conditional(CondCommand(node)),
            SyntaxKind::FunctionDef => Self::Function(FunctionDef(node)),
            _ => return None,
        })
    }

    pub const fn syntax(self) -> &'a Node {
        match self {
            Self::Simple(inner) => inner.syntax(),
            Self::If(inner) => inner.syntax(),
            Self::Loop(inner) => inner.syntax(),
            Self::For(inner) => inner.syntax(),
            Self::Case(inner) => inner.syntax(),
            Self::Subshell(inner) => inner.syntax(),
            Self::Group(inner) => inner.syntax(),
            Self::Arithmetic(inner) => inner.syntax(),
            Self::Conditional(inner) => inner.syntax(),
            Self::Function(inner) => inner.syntax(),
        }
    }

    pub fn span(self) -> Span {
        self.syntax().span()
    }
}

impl<'a> SimpleCommand<'a> {
    /// The assignments written before the command word.
    pub fn assignments(self) -> impl Iterator<Item = Assignment<'a>> {
        self.0.nodes().filter_map(Assignment::cast)
    }

    /// Every word, the command name first.
    pub fn words(self) -> impl Iterator<Item = Word<'a>> {
        self.0.nodes().filter_map(Word::cast)
    }

    pub fn redirects(self) -> impl Iterator<Item = Redirect<'a>> {
        self.0.nodes().filter_map(Redirect::cast)
    }

    /// The command being run, if there is one. `x=1` on its own is a command with no name.
    pub fn name(self) -> Option<Word<'a>> {
        self.words().next()
    }

    pub fn arguments(self) -> impl Iterator<Item = Word<'a>> {
        self.words().skip(1)
    }
}

impl<'a> Assignment<'a> {
    /// The name on the left of the operator.
    pub fn name(self, source: &Source) -> &str {
        self.0
            .tokens()
            .find(|token| token.kind() == SyntaxKind::Text)
            .map_or("", |token| token.text(source))
    }

    /// `=` or `+=`.
    pub fn operator(self) -> Option<SyntaxKind> {
        self.0
            .tokens()
            .map(|token| token.kind())
            .find(|kind| matches!(kind, SyntaxKind::Equal | SyntaxKind::PlusEqual))
    }

    /// Whether the value adds to what is already there.
    pub fn is_appending(self) -> bool {
        self.operator() == Some(SyntaxKind::PlusEqual)
    }

    /// The value, unless it was written as an array.
    pub fn value(self) -> Option<Word<'a>> {
        self.0.nodes().find_map(Word::cast)
    }

    /// The elements, if the value was written `(a b c)`.
    pub fn array(self) -> Option<impl Iterator<Item = Word<'a>>> {
        let array = self.0.node(SyntaxKind::ArrayValue)?;
        Some(array.nodes().filter_map(Word::cast))
    }
}

impl<'a> Redirect<'a> {
    /// The descriptor written in front, as in `2>&1`.
    pub fn descriptor(self, source: &Source) -> Option<u32> {
        let first = self.0.tokens().next()?;
        (first.kind() == SyntaxKind::Text)
            .then(|| first.text(source).parse().ok())
            .flatten()
    }

    /// Which redirection it is.
    pub fn operator(self) -> Option<SyntaxKind> {
        self.0
            .tokens()
            .map(|token| token.kind())
            .find(|kind| kind.is_redirect_operator())
    }

    /// The file, descriptor or delimiter on the right.
    pub fn target(self) -> Option<Word<'a>> {
        self.0.nodes().find_map(Word::cast)
    }

    /// Whether this opens a here-document rather than naming a file.
    pub fn is_heredoc(self) -> bool {
        matches!(
            self.operator(),
            Some(SyntaxKind::LessLess | SyntaxKind::LessLessDash)
        )
    }
}

impl<'a> FunctionDef<'a> {
    pub fn name(self) -> Option<Word<'a>> {
        self.0.nodes().find_map(Word::cast)
    }

    /// What the function runs, usually a group.
    pub fn body(self) -> Option<Command<'a>> {
        self.0.nodes().find_map(Command::cast)
    }
}
