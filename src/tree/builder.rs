//! Building a tree, one token at a time.
//!
//! The builder owns the cursor into the source. A caller says what kind a token is and how long it
//! is, never where it starts, so tokens cannot overlap and cannot leave gaps between them. The
//! losslessness of the tree is a property of this type rather than something the parser has to
//! remember to preserve.

use super::kind::SyntaxKind;
use super::{Element, Node, Token, Tree};
use crate::source::Source;
use crate::span::Span;

/// A position in the tree being built, to wrap children that turned out to belong together.
///
/// Shell needs this. `a && b` parses `a` as a pipeline before the `&&` reveals that it was the
/// first branch of an and-or list, and the node has to be slipped in underneath what is already
/// built rather than around what is about to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    depth: usize,
    index: usize,
    offset: u32,
}

struct Frame {
    kind: SyntaxKind,
    start: u32,
    children: Vec<Element>,
}

pub struct Builder {
    source: Source,
    offset: u32,
    stack: Vec<Frame>,
}

impl Builder {
    pub fn new(source: Source, root: SyntaxKind) -> Self {
        let stack = vec![Frame {
            kind: root,
            start: 0,
            children: Vec::new(),
        }];
        Self {
            source,
            offset: 0,
            stack,
        }
    }

    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// How far into the source the builder has been told about.
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    /// Open a node. Everything added until the matching [`Builder::finish`] goes inside it.
    pub fn start(&mut self, kind: SyntaxKind) {
        self.stack.push(Frame {
            kind,
            start: self.offset,
            children: Vec::new(),
        });
    }

    /// Add a token of `len` bytes at the cursor, and advance the cursor past it.
    ///
    /// A length that would run past the end of the source is clamped rather than trusted; the
    /// tree stays inside the text it describes whatever the caller believes.
    pub fn token(&mut self, kind: SyntaxKind, len: u32) {
        let start = self.offset;
        let end = start.saturating_add(len).min(self.source.len());
        self.offset = end;
        self.push(Element::Token(Token {
            kind,
            span: Span::new(start, end),
        }));
    }

    /// Close the innermost open node. The root cannot be closed this way.
    pub fn finish(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(frame) = self.stack.pop() else {
            return;
        };
        self.push(Element::Node(Node {
            kind: frame.kind,
            span: Span::new(frame.start, self.offset),
            children: frame.children,
        }));
    }

    /// Mark the current position so a node can later be opened as if it had started here.
    pub fn checkpoint(&mut self) -> Checkpoint {
        Checkpoint {
            depth: self.stack.len(),
            index: self.stack.last().map_or(0, |frame| frame.children.len()),
            offset: self.offset,
        }
    }

    /// Open a node retroactively, adopting everything added since the checkpoint.
    ///
    /// A checkpoint taken at a different depth than the one it is used at cannot be honoured; the
    /// node is opened at the cursor instead, which loses structure but never loses text.
    pub fn start_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        let honoured = checkpoint.depth == self.stack.len()
            && self
                .stack
                .last()
                .is_some_and(|frame| checkpoint.index <= frame.children.len());
        let Some(frame) = self.stack.last_mut().filter(|_| honoured) else {
            return self.start(kind);
        };
        let adopted = frame.children.split_off(checkpoint.index);
        self.stack.push(Frame {
            kind,
            start: checkpoint.offset,
            children: adopted,
        });
    }

    /// Close every open node and produce the tree.
    ///
    /// Any source the caller never accounted for is appended as an [`SyntaxKind::Error`] node, so
    /// that a parser which loses its place produces a tree that is still complete. Clean input
    /// must never reach that path, which is what the parser's own tests assert.
    pub fn build(mut self) -> Tree {
        while self.stack.len() > 1 {
            self.finish();
        }
        if self.offset < self.source.len() {
            let remaining = self.source.len() - self.offset;
            self.start(SyntaxKind::Error);
            self.token(SyntaxKind::Unknown, remaining);
            self.finish();
        }
        let root = match self.stack.pop() {
            Some(frame) => Node {
                kind: frame.kind,
                span: Span::new(frame.start, self.offset),
                children: frame.children,
            },
            None => Node {
                kind: SyntaxKind::Script,
                span: Span::empty(0),
                children: Vec::new(),
            },
        };
        Tree::new(self.source, root)
    }

    fn push(&mut self, element: Element) {
        if let Some(frame) = self.stack.last_mut() {
            frame.children.push(element);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_tile_the_source_without_gaps() {
        let mut builder = Builder::new(Source::new("a b"), SyntaxKind::Script);
        builder.token(SyntaxKind::Text, 1);
        builder.token(SyntaxKind::Whitespace, 1);
        builder.token(SyntaxKind::Text, 1);
        let tree = builder.build();
        assert_eq!(tree.reconstruct(), "a b");
        let spans: Vec<_> = tree.root().tokens().map(Token::span).collect();
        assert_eq!(spans, [Span::new(0, 1), Span::new(1, 2), Span::new(2, 3)]);
    }

    #[test]
    fn an_unclosed_node_is_closed_by_build() {
        let mut builder = Builder::new(Source::new("ab"), SyntaxKind::Script);
        builder.start(SyntaxKind::SimpleCommand);
        builder.token(SyntaxKind::Text, 2);
        let tree = builder.build();
        assert_eq!(tree.reconstruct(), "ab");
        assert!(tree.root().node(SyntaxKind::SimpleCommand).is_some());
    }

    #[test]
    fn source_left_unaccounted_for_becomes_an_error_node() {
        let mut builder = Builder::new(Source::new("echo hi"), SyntaxKind::Script);
        builder.token(SyntaxKind::Text, 4);
        let tree = builder.build();
        assert_eq!(tree.reconstruct(), "echo hi");
        assert!(tree.root().has_errors());
    }

    #[test]
    fn a_length_past_the_end_is_clamped() {
        let mut builder = Builder::new(Source::new("ab"), SyntaxKind::Script);
        builder.token(SyntaxKind::Text, 99);
        let tree = builder.build();
        assert_eq!(tree.reconstruct(), "ab");
        assert_eq!(tree.root().span(), Span::new(0, 2));
    }

    #[test]
    fn a_checkpoint_adopts_what_came_before_it() {
        let mut builder = Builder::new(Source::new("a&&b"), SyntaxKind::Script);
        let start = builder.checkpoint();
        builder.start(SyntaxKind::Pipeline);
        builder.token(SyntaxKind::Text, 1);
        builder.finish();
        builder.start_at(start, SyntaxKind::AndOrList);
        builder.token(SyntaxKind::AndAnd, 2);
        builder.start(SyntaxKind::Pipeline);
        builder.token(SyntaxKind::Text, 1);
        builder.finish();
        builder.finish();

        let tree = builder.build();
        assert_eq!(tree.reconstruct(), "a&&b");
        let list = tree
            .root()
            .node(SyntaxKind::AndOrList)
            .expect("the list was wrapped in");
        assert_eq!(list.span(), Span::new(0, 4));
        assert_eq!(list.nodes().count(), 2, "both pipelines moved inside");
    }

    #[test]
    fn an_empty_node_has_a_position_anyway() {
        let mut builder = Builder::new(Source::new("ab"), SyntaxKind::Script);
        builder.token(SyntaxKind::Text, 2);
        builder.start(SyntaxKind::Error);
        builder.finish();
        let tree = builder.build();
        let empty = tree.root().node(SyntaxKind::Error).expect("an empty node");
        assert_eq!(empty.span(), Span::empty(2));
    }

    #[test]
    fn finishing_more_than_was_started_leaves_the_root_alone() {
        let mut builder = Builder::new(Source::new("a"), SyntaxKind::Script);
        builder.token(SyntaxKind::Text, 1);
        builder.finish();
        builder.finish();
        let tree = builder.build();
        assert_eq!(tree.root().kind(), SyntaxKind::Script);
        assert_eq!(tree.reconstruct(), "a");
    }
}
