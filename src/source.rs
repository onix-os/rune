//! The script being parsed, and the index that turns an offset back into a place.

use crate::span::Span;

/// A script, with the byte offset of every line start precomputed.
///
/// The tree stores offsets; a person reads line and column. This is the only thing that knows how
/// to get from one to the other, and it is built once rather than by scanning for newlines at
/// every diagnostic.
#[derive(Debug, Clone)]
pub struct Source {
    text: String,
    /// Byte offset of the start of each line. Always begins with `0`.
    ///
    /// A trailing newline opens a final, empty line: `"a\n"` has starts `[0, 2]`. That is what
    /// makes an offset at end-of-input land somewhere sensible rather than off the end.
    starts: Vec<u32>,
}

impl Source {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(at, _)| (at as u32).saturating_add(1)),
        );
        Self { text, starts }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> u32 {
        self.text.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn slice(&self, span: Span) -> &str {
        span.of(&self.text)
    }

    /// How many lines the index holds.
    ///
    /// A trailing newline opens a final, empty line, so `"a\n"` counts as two.
    pub fn line_count(&self) -> u32 {
        self.starts.len() as u32
    }

    /// The 1-based line and column an offset falls on.
    ///
    /// The column counts characters, not bytes, because that is what someone counting along a
    /// line arrives at.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = self.line_of(offset);
        let start = self.line_start(line);
        let column = self
            .text
            .get(start as usize..offset as usize)
            .map_or(0, |run| run.chars().count()) as u32;
        (line, column.saturating_add(1))
    }

    /// The 1-based line an offset falls on.
    pub fn line_of(&self, offset: u32) -> u32 {
        self.starts.partition_point(|start| *start <= offset).max(1) as u32
    }

    /// Byte offset where a 1-based line begins; the end of the text if the line is past the end.
    pub fn line_start(&self, line: u32) -> u32 {
        let index = line.saturating_sub(1) as usize;
        self.starts
            .get(index)
            .copied()
            .unwrap_or_else(|| self.len())
    }

    /// The span of a 1-based line, with its line terminator left out.
    pub fn line_span(&self, line: u32) -> Span {
        let start = self.line_start(line);
        let end = self
            .starts
            .get(line as usize)
            .copied()
            .unwrap_or_else(|| self.len());
        let text = self.text.get(start as usize..end as usize).unwrap_or("");
        let trimmed = text.trim_end_matches('\n').trim_end_matches('\r');
        Span::new(start, start.saturating_add(trimmed.len() as u32))
    }

    /// The text of a 1-based line, without its line terminator.
    pub fn line_text(&self, line: u32) -> &str {
        self.slice(self.line_span(line))
    }
}

impl From<&str> for Source {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_and_columns_from_one() {
        let source = Source::new("echo a\necho b\n");
        assert_eq!(source.line_col(0), (1, 1));
        assert_eq!(source.line_col(5), (1, 6));
        assert_eq!(source.line_col(7), (2, 1));
    }

    #[test]
    fn a_column_counts_characters_not_bytes() {
        // Three two-byte characters, then the offset of the fourth character.
        let source = Source::new("ééé x");
        assert_eq!(source.line_col(6), (1, 4));
    }

    #[test]
    fn a_line_leaves_out_its_terminator() {
        let source = Source::new("echo a\necho b\n");
        assert_eq!(source.line_text(1), "echo a");
        assert_eq!(source.line_text(2), "echo b");
    }

    #[test]
    fn a_carriage_return_is_a_terminator_too() {
        assert_eq!(Source::new("echo a\r\nb").line_text(1), "echo a");
    }

    #[test]
    fn a_trailing_newline_opens_a_final_empty_line() {
        let source = Source::new("a\n");
        assert_eq!(source.line_count(), 2);
        assert_eq!(source.line_col(2), (2, 1));
        assert_eq!(source.line_text(2), "");
    }

    #[test]
    fn the_end_of_the_last_line_is_on_that_line() {
        let source = Source::new("echo");
        assert_eq!(source.line_col(4), (1, 5));
    }

    #[test]
    fn a_line_past_the_end_is_empty_rather_than_a_panic() {
        let source = Source::new("a\n");
        assert_eq!(source.line_text(99), "");
        assert_eq!(source.line_start(99), 2);
    }

    #[test]
    fn an_empty_source_still_has_a_first_line() {
        let source = Source::new("");
        assert_eq!(source.line_count(), 1);
        assert_eq!(source.line_col(0), (1, 1));
        assert_eq!(source.line_text(1), "");
    }
}
