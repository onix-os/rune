//! A typed view over the tree.
//!
//! Nothing here owns anything. Each type is a node with a name on it, and every accessor is a walk
//! over that node's children, so the view can never drift from the tree it describes — there is no
//! second copy to fall out of step.
//!
//! Casting is how a node becomes typed: [`Command::cast`] answers what kind of command a node is,
//! and returns nothing if it is not a command at all.

mod command;
mod compound;
mod word;

pub use command::{Assignment, Command, FunctionDef, Redirect, SimpleCommand};
pub use compound::{
    ArithCommand, CaseCommand, CaseItem, CondCommand, ElifClause, ForCommand, Group, IfCommand,
    LoopCommand, Subshell,
};
pub use word::{
    ArithmeticExpansion, CommandSubstitution, ParameterExpansion, ProcessSubstitution, Word,
    WordPiece,
};

use crate::span::Span;
use crate::tree::{Node, SyntaxKind, Tree};

/// Define a typed wrapper around one node kind.
macro_rules! typed {
    ($(#[$doc:meta])* $name:ident => $kind:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name<'a>(pub(crate) &'a Node);

        impl<'a> $name<'a> {
            /// View `node` as this, if that is what it is.
            pub fn cast(node: &'a Node) -> Option<Self> {
                (node.kind() == SyntaxKind::$kind).then_some(Self(node))
            }

            /// The node underneath, for anything this view does not answer.
            pub const fn syntax(self) -> &'a Node {
                self.0
            }

            pub fn span(self) -> Span {
                self.0.span()
            }
        }
    };
}

pub(crate) use typed;

typed! {
    /// A whole script.
    Script => Script
}

typed! {
    /// A run of statements.
    CommandList => CommandList
}

typed! {
    /// One statement, and what terminated it.
    ListItem => ListItem
}

typed! {
    /// `a && b || c`.
    AndOrList => AndOrList
}

typed! {
    /// `! time a | b`.
    Pipeline => Pipeline
}

impl<'a> Script<'a> {
    /// The typed root of a parsed script.
    pub fn of(tree: &'a Tree) -> Self {
        Self(tree.root())
    }

    pub fn commands(self) -> Option<CommandList<'a>> {
        self.0.nodes().find_map(CommandList::cast)
    }

    /// Every statement in the script, in order.
    pub fn items(self) -> impl Iterator<Item = ListItem<'a>> {
        self.commands().into_iter().flat_map(CommandList::items)
    }
}

impl<'a> CommandList<'a> {
    pub fn items(self) -> impl Iterator<Item = ListItem<'a>> {
        self.0.nodes().filter_map(ListItem::cast)
    }
}

impl<'a> ListItem<'a> {
    /// What runs, whether or not it is wrapped in a pipeline or an and-or list.
    pub fn command(self) -> Option<Command<'a>> {
        self.0.nodes().find_map(Command::cast)
    }

    pub fn and_or(self) -> Option<AndOrList<'a>> {
        self.0.nodes().find_map(AndOrList::cast)
    }

    pub fn pipeline(self) -> Option<Pipeline<'a>> {
        self.0.nodes().find_map(Pipeline::cast)
    }

    /// `;`, `&` or a newline. Absent on the last statement of a file with no trailing separator.
    pub fn terminator(self) -> Option<SyntaxKind> {
        self.0.tokens().map(|token| token.kind()).find(|kind| {
            matches!(
                kind,
                SyntaxKind::Semi | SyntaxKind::Amp | SyntaxKind::Newline
            )
        })
    }

    /// Whether the statement was sent to the background with `&`.
    pub fn is_background(self) -> bool {
        self.terminator() == Some(SyntaxKind::Amp)
    }
}

impl<'a> AndOrList<'a> {
    /// Each pipeline in the chain, with the operator that reached it.
    ///
    /// The first has no operator. `a && b || c` gives `[(None, a), (Some(&&), b), (Some(||), c)]`.
    pub fn branches(self) -> Vec<(Option<SyntaxKind>, Command<'a>)> {
        let mut out = Vec::new();
        let mut operator = None;
        for child in self.0.children() {
            match child {
                crate::tree::Element::Token(token)
                    if matches!(token.kind(), SyntaxKind::AndAnd | SyntaxKind::PipePipe) =>
                {
                    operator = Some(token.kind());
                }
                crate::tree::Element::Node(node) => {
                    if let Some(command) = Command::cast(node) {
                        out.push((operator.take(), command));
                    }
                }
                crate::tree::Element::Token(_) => {}
            }
        }
        out
    }
}

impl<'a> Pipeline<'a> {
    /// `! cmd` — the status is inverted.
    pub fn is_negated(self) -> bool {
        self.0.token(SyntaxKind::Bang).is_some()
    }

    /// `time cmd` — report how long the pipeline took.
    pub fn is_timed(self) -> bool {
        self.0.token(SyntaxKind::Time).is_some()
    }

    pub fn commands(self) -> impl Iterator<Item = Command<'a>> {
        self.0.nodes().filter_map(Command::cast)
    }
}

#[cfg(test)]
mod tests;
