//! A shell parser that keeps going.
//!
//! `rune` reads POSIX shell — and the parts of bash worth reading — and produces a lossless tree.
//! It does not execute anything, expand anything, or stop at the first mistake.
//!
//! The tree holds every byte of the input, including whitespace, comments, and the text of things
//! that did not parse, so [`Tree::reconstruct`] returns the input unchanged. That property is what
//! error recovery, syntax highlighting, and a formatter are all built on, and it is asserted on
//! everything the test suite parses.
//!
//! # Where things are
//!
//! - [`Source`] is the script, and the index from byte offset to line and column.
//! - [`Span`] is a byte range; nothing in the tree stores text.
//! - [`tree`] is the tree itself, and [`Builder`] is the only way to make one.
//! - [`Error`] is one thing that went wrong. Parsing collects them rather than returning them.

pub mod ast;
pub mod error;
pub mod lex;
pub mod parse;
pub mod source;
pub mod span;
pub mod tree;

pub use ast::Script;
pub use error::{Completeness, Error, Severity};
pub use lex::{Lexed, Lexing, Unclosed, lex};
pub use parse::{Parsed, parse};
pub use source::Source;
pub use span::Span;
pub use tree::{Builder, Element, Node, SyntaxKind, Token, Tree};
