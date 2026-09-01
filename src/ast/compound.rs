//! The commands built out of reserved words.
//!
//! `while` and `until` share a type, and so do `for`, `for ((…))` and `select`: they are written
//! the same way and read the same way, and which one it is stays available as the node's kind.

use super::typed;
use super::{CommandList, Word};
use crate::source::Source;
use crate::span::Span;
use crate::tree::{Node, SyntaxKind};

typed! {
    /// `if … then … elif … else … fi`.
    IfCommand => IfCommand
}

typed! {
    /// `case x in … esac`.
    CaseCommand => CaseCommand
}

typed! {
    /// One branch of a `case`.
    CaseItem => CaseItem
}

typed! {
    /// `( … )`, which runs in a child shell.
    Subshell => Subshell
}

typed! {
    /// `{ … ; }`, which does not.
    Group => Group
}

typed! {
    /// `(( … ))`.
    ArithCommand => ArithCommand
}

typed! {
    /// `[[ … ]]`.
    CondCommand => CondCommand
}

/// Write a type by hand for the node kinds that share a shape.
macro_rules! several {
    ($(#[$doc:meta])* $name:ident => $($kind:ident)|+) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name<'a>(pub(crate) &'a Node);

        impl<'a> $name<'a> {
            pub fn cast(node: &'a Node) -> Option<Self> {
                matches!(node.kind(), $(SyntaxKind::$kind)|+).then_some(Self(node))
            }

            pub const fn syntax(self) -> &'a Node {
                self.0
            }

            pub fn span(self) -> Span {
                self.0.span()
            }

            /// Which of the shapes this is, so a caller that cares can still tell.
            pub fn kind(self) -> SyntaxKind {
                self.0.kind()
            }
        }
    };
}

several! {
    /// `while … do … done` or `until … do … done`.
    LoopCommand => WhileCommand | UntilCommand
}

several! {
    /// `for … do … done`, in any of its three spellings.
    ForCommand => ForCommand | ArithForCommand | SelectCommand
}

impl<'a> IfCommand<'a> {
    /// The list between `if` and `then`.
    pub fn condition(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }

    /// The list between `then` and the first of `elif`, `else` or `fi`.
    pub fn then_branch(self) -> Option<CommandList<'a>> {
        self.0.nodes().filter_map(CommandList::cast).nth(1)
    }

    pub fn elif_clauses(self) -> impl Iterator<Item = ElifClause<'a>> {
        self.0.nodes().filter_map(ElifClause::cast)
    }

    pub fn else_branch(self) -> Option<CommandList<'a>> {
        let clause = self.0.node(SyntaxKind::ElseClause)?;
        clause.nodes().find_map(CommandList::cast)
    }
}

typed! {
    /// `elif … then …`.
    ElifClause => ElifClause
}

impl<'a> ElifClause<'a> {
    pub fn condition(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }

    pub fn then_branch(self) -> Option<CommandList<'a>> {
        self.0.nodes().filter_map(CommandList::cast).nth(1)
    }
}

impl<'a> LoopCommand<'a> {
    /// True for `until`, whose body runs while the test *fails*.
    pub fn is_until(self) -> bool {
        self.0.kind() == SyntaxKind::UntilCommand
    }

    pub fn condition(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }

    pub fn body(self) -> Option<CommandList<'a>> {
        self.0.nodes().filter_map(CommandList::cast).nth(1)
    }
}

impl<'a> ForCommand<'a> {
    /// The variable the loop sets. Absent from the `for ((…))` form, which sets its own.
    pub fn variable(self) -> Option<Word<'a>> {
        (self.0.kind() != SyntaxKind::ArithForCommand)
            .then(|| self.0.nodes().find_map(Word::cast))
            .flatten()
    }

    /// The words after `in`, if there were any. `for i; do` iterates the positional parameters.
    pub fn items(self) -> impl Iterator<Item = Word<'a>> {
        let skip = usize::from(self.variable().is_some());
        self.0.nodes().filter_map(Word::cast).skip(skip)
    }

    pub fn body(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }
}

impl<'a> CaseCommand<'a> {
    /// The word being matched against.
    pub fn word(self) -> Option<Word<'a>> {
        self.0.nodes().find_map(Word::cast)
    }

    pub fn items(self) -> impl Iterator<Item = CaseItem<'a>> {
        self.0.nodes().filter_map(CaseItem::cast)
    }
}

impl<'a> CaseItem<'a> {
    /// The patterns on the left of the `)`.
    pub fn patterns(self) -> impl Iterator<Item = Word<'a>> {
        self.0
            .nodes()
            .filter(|node| node.kind() == SyntaxKind::CasePattern)
            .filter_map(|node| node.nodes().find_map(Word::cast))
    }

    pub fn body(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }

    /// `;;`, `;&` or `;;&` — what happens after this branch runs.
    ///
    /// The three are three different programs, so the distinction is kept rather than inferred.
    pub fn terminator(self) -> Option<SyntaxKind> {
        self.0.tokens().map(|token| token.kind()).find(|kind| {
            matches!(
                kind,
                SyntaxKind::SemiSemi | SyntaxKind::SemiAmp | SyntaxKind::SemiSemiAmp
            )
        })
    }
}

impl<'a> Subshell<'a> {
    pub fn body(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }
}

impl<'a> Group<'a> {
    pub fn body(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }
}

impl<'a> CondCommand<'a> {
    /// The words of the test, `[[` and `]]` left out.
    pub fn words(self) -> impl Iterator<Item = Word<'a>> {
        self.0.nodes().filter_map(Word::cast)
    }
}

impl ArithCommand<'_> {
    /// The expression, still as source text.
    ///
    /// It stays text because it has to: POSIX expands parameters and command substitutions over it
    /// before any of it is arithmetic, so it cannot be evaluated — or usefully parsed — until it
    /// runs.
    pub fn expression(self, source: &Source) -> &str {
        let text = source.slice(self.0.span());
        let text = text.strip_prefix("((").unwrap_or(text);
        text.strip_suffix("))").unwrap_or(text)
    }
}
