//! The lossless syntax tree.
//!
//! Every byte of the input is in here: whitespace, comments, and the text of things that did not
//! parse. Nothing stores text — a token is a kind and a [`Span`], and the text is recovered by
//! slicing the [`Source`]. That is what makes [`Tree::reconstruct`] the identity function, and
//! it is the invariant everything else in the crate is allowed to assume.

mod builder;
mod dump;
mod kind;

pub use builder::{Builder, Checkpoint};
pub use kind::SyntaxKind;

use crate::source::Source;
use crate::span::Span;

/// A leaf: a run of source text with a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    kind: SyntaxKind,
    span: Span,
}

impl Token {
    pub const fn kind(&self) -> SyntaxKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn text<'a>(&self, source: &'a Source) -> &'a str {
        source.slice(self.span)
    }
}

/// A branch: a kind, the range it covers, and what is inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    kind: SyntaxKind,
    span: Span,
    children: Vec<Element>,
}

impl Node {
    pub const fn kind(&self) -> SyntaxKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn children(&self) -> impl Iterator<Item = &Element> {
        self.children.iter()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(Element::as_node)
    }

    pub fn tokens(&self) -> impl Iterator<Item = &Token> {
        self.children.iter().filter_map(Element::as_token)
    }

    /// The first direct child node of a kind. What most typed accessors are made of.
    pub fn node(&self, kind: SyntaxKind) -> Option<&Node> {
        self.nodes().find(|node| node.kind == kind)
    }

    /// The first direct child token of a kind.
    pub fn token(&self, kind: SyntaxKind) -> Option<&Token> {
        self.tokens().find(|token| token.kind == kind)
    }

    pub fn text<'a>(&self, source: &'a Source) -> &'a str {
        source.slice(self.span)
    }

    /// Every token beneath this node, in source order.
    pub fn visit_tokens(&self, visit: &mut impl FnMut(&Token)) {
        for child in &self.children {
            match child {
                Element::Token(token) => visit(token),
                Element::Node(node) => node.visit_tokens(visit),
            }
        }
    }

    /// Whether anything beneath this node failed to parse.
    pub fn has_errors(&self) -> bool {
        self.kind == SyntaxKind::Error
            || self.nodes().any(Node::has_errors)
            || self.tokens().any(|token| token.kind == SyntaxKind::Unknown)
    }
}

/// A child of a node: another node, or a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    Node(Node),
    Token(Token),
}

impl Element {
    pub const fn as_node(&self) -> Option<&Node> {
        match self {
            Self::Node(node) => Some(node),
            Self::Token(_) => None,
        }
    }

    pub const fn as_token(&self) -> Option<&Token> {
        match self {
            Self::Token(token) => Some(token),
            Self::Node(_) => None,
        }
    }

    pub const fn kind(&self) -> SyntaxKind {
        match self {
            Self::Node(node) => node.kind,
            Self::Token(token) => token.kind,
        }
    }

    pub const fn span(&self) -> Span {
        match self {
            Self::Node(node) => node.span,
            Self::Token(token) => token.span,
        }
    }
}

/// A parsed script: the text, and the tree over it.
#[derive(Debug, Clone)]
pub struct Tree {
    source: Source,
    root: Node,
}

impl Tree {
    pub(crate) const fn new(source: Source, root: Node) -> Self {
        Self { source, root }
    }

    pub const fn source(&self) -> &Source {
        &self.source
    }

    pub const fn root(&self) -> &Node {
        &self.root
    }

    /// Concatenate every token's text.
    ///
    /// This returns the input, byte for byte, for every tree this crate can build. It is the
    /// definition of lossless, and the test suite asserts it on everything it parses.
    pub fn reconstruct(&self) -> String {
        let mut out = String::with_capacity(self.source.text().len());
        self.root
            .visit_tokens(&mut |token| out.push_str(token.text(&self.source)));
        out
    }

    /// An indented view of the tree, for tests and for looking at it.
    pub fn dump(&self) -> String {
        dump::dump(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `echo hi`, built by hand. Phase 0 has no tokenizer; this is the toy tree.
    fn toy() -> Tree {
        let mut builder = Builder::new(Source::new("echo hi"), SyntaxKind::Script);
        builder.start(SyntaxKind::SimpleCommand);
        builder.start(SyntaxKind::Word);
        builder.token(SyntaxKind::Text, 4);
        builder.finish();
        builder.token(SyntaxKind::Whitespace, 1);
        builder.start(SyntaxKind::Word);
        builder.token(SyntaxKind::Text, 2);
        builder.finish();
        builder.finish();
        builder.build()
    }

    #[test]
    fn a_tree_reconstructs_its_source() {
        let tree = toy();
        assert_eq!(tree.reconstruct(), "echo hi");
    }

    #[test]
    fn spans_run_from_the_first_child_to_the_last() {
        let tree = toy();
        assert_eq!(tree.root().span(), Span::new(0, 7));
        let command = tree
            .root()
            .node(SyntaxKind::SimpleCommand)
            .expect("a command");
        assert_eq!(command.span(), Span::new(0, 7));
        let word = command.node(SyntaxKind::Word).expect("a word");
        assert_eq!(word.span(), Span::new(0, 4));
        assert_eq!(word.text(tree.source()), "echo");
    }

    #[test]
    fn a_clean_tree_reports_no_errors() {
        assert!(!toy().root().has_errors());
    }

    #[test]
    fn an_error_node_is_visible_from_the_root() {
        let mut builder = Builder::new(Source::new("`"), SyntaxKind::Script);
        builder.start(SyntaxKind::SimpleCommand);
        builder.start(SyntaxKind::Error);
        builder.token(SyntaxKind::Unknown, 1);
        builder.finish();
        builder.finish();
        let tree = builder.build();
        assert!(tree.root().has_errors());
        assert_eq!(tree.reconstruct(), "`");
    }
}
