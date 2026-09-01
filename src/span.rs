//! Byte ranges into the source text.

use std::fmt;
use std::ops::Range;

/// A half-open byte range, `start..end`.
///
/// Offsets are `u32`, which caps a script at 4 GiB and halves the size of every node in the tree.
/// Nothing in the tree stores text; a token is a kind and one of these, and the text is recovered
/// by slicing. That is what makes losslessness structural rather than something to be maintained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// A zero-width span, for a construct that is missing rather than wrong.
    pub const fn empty(at: u32) -> Self {
        Self { start: at, end: at }
    }

    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.end <= self.start
    }

    pub const fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// The smallest span containing both.
    pub const fn cover(self, other: Self) -> Self {
        let start = if self.start < other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end > other.end {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }

    /// The text this span covers, or `""` if it does not land on character boundaries.
    ///
    /// Slicing is fallible on purpose. A parser that panics is worse than one that is wrong, and
    /// the one input guaranteed to be strange is the one that made the parser reach for a span in
    /// the first place.
    pub fn of(self, text: &str) -> &str {
        text.get(self.start as usize..self.end as usize)
            .unwrap_or("")
    }
}

impl From<Span> for Range<usize> {
    fn from(span: Span) -> Self {
        span.start as usize..span.end as usize
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_both_ends() {
        let joined = Span::new(4, 6).cover(Span::new(1, 5));
        assert_eq!(joined, Span::new(1, 6));
    }

    #[test]
    fn an_empty_span_still_has_a_position() {
        let span = Span::empty(7);
        assert!(span.is_empty());
        assert_eq!(span.start, 7);
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn slices_the_text() {
        assert_eq!(Span::new(5, 10).of("echo hello"), "hello");
    }

    #[test]
    fn a_span_past_the_end_yields_nothing() {
        assert_eq!(Span::new(3, 99).of("echo").len(), 0);
    }

    #[test]
    fn a_span_inside_a_character_yields_nothing() {
        // "é" is two bytes; cutting between them must not panic.
        assert_eq!(Span::new(0, 1).of("é"), "");
    }

    #[test]
    fn contains_is_half_open() {
        let span = Span::new(2, 4);
        assert!(span.contains(2));
        assert!(span.contains(3));
        assert!(!span.contains(4));
    }
}
