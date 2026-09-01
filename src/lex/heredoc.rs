//! Here-documents, which is where a line-at-a-time lexer usually comes apart.
//!
//! `<<EOF` says nothing about the text next to it. The body begins on the *following* line, so the
//! lexer has to note the delimiter, carry on to the end of the current line, and only then change
//! what it is reading. More than one can be queued at once — `cat <<A <<B` takes two bodies, in
//! the order the operators appeared.

use super::{Lexed, Lexer};
use crate::tree::SyntaxKind;

/// A body the lexer owes, once it reaches the end of the line that asked for it.
pub(super) struct Heredoc {
    /// The delimiter with its quoting removed, which is what the terminator line is compared to.
    delimiter: String,
    /// `<<-`: leading tabs come off every line, the terminator included.
    strip_tabs: bool,
}

/// Strip the quoting from a delimiter word.
///
/// `<<'EOF'`, `<<"EOF"` and `<<\EOF` all end at a line saying `EOF`. The quoting is not part of the
/// delimiter; what it decides is whether the body expands, which is the parser's to record.
fn unquote(raw: &str) -> (String, bool) {
    let mut out = String::with_capacity(raw.len());
    let mut quoted = false;
    for ch in raw.chars() {
        match ch {
            '\'' | '"' | '\\' => quoted = true,
            _ => out.push(ch),
        }
    }
    (out, quoted)
}

impl<'a> Lexer<'a> {
    /// Note that the word coming up names a here-document delimiter.
    pub(super) fn expect_delimiter(&mut self, strip_tabs: bool) {
        self.awaiting_delimiter = Some(strip_tabs);
        self.delimiter_text.clear();
    }

    /// Feed an emitted token to the delimiter that is being collected, if one is.
    ///
    /// The delimiter is a word like any other and can be built from several pieces, which is why
    /// this watches the token stream rather than reading ahead: `<<"E"OF` is one word spelling
    /// `EOF`, and only the pieces know where it ends.
    pub(super) fn collect_delimiter(&mut self, kind: SyntaxKind, text: &'a str) {
        if self.awaiting_delimiter.is_none() {
            return;
        }
        if super::word::continues_a_word(kind) {
            self.delimiter_text.push_str(text);
            return;
        }
        // Whitespace before the delimiter is allowed: `cat << EOF` is the same as `cat <<EOF`.
        if kind == SyntaxKind::Whitespace && self.delimiter_text.is_empty() {
            return;
        }
        self.finish_delimiter();
    }

    pub(super) fn finish_delimiter(&mut self) {
        let Some(strip_tabs) = self.awaiting_delimiter.take() else {
            return;
        };
        if self.delimiter_text.is_empty() {
            return;
        }
        let (delimiter, _quoted) = unquote(&self.delimiter_text);
        self.delimiter_text.clear();
        self.heredocs.push(Heredoc {
            delimiter,
            strip_tabs,
        });
    }

    /// Read every body owed, now that the line asking for them has ended.
    pub(super) fn take_heredoc_bodies(&mut self) {
        while !self.heredocs.is_empty() {
            let doc = self.heredocs.remove(0);
            self.read_body(&doc);
        }
    }

    fn read_body(&mut self, doc: &Heredoc) {
        let body_start = self.cursor.offset();
        loop {
            if self.cursor.is_eof() {
                // The file ended before the delimiter did. Everything left is body; the parser
                // is what says so.
                self.close_body(body_start, self.cursor.offset(), None);
                return;
            }
            let line = self.cursor.line();
            let content = line.strip_suffix('\n').unwrap_or(line);
            let compared = if doc.strip_tabs {
                content.trim_start_matches('\t')
            } else {
                content
            };
            if compared == doc.delimiter {
                let end = self.cursor.offset();
                self.cursor.advance(line.len());
                self.close_body(body_start, end, Some(self.cursor.offset()));
                return;
            }
            self.cursor.advance(line.len());
        }
    }

    /// Emit the body, and the line that closed it if there was one.
    fn close_body(&mut self, body_start: u32, body_end: u32, terminator_end: Option<u32>) {
        if body_end > body_start {
            self.push_raw(SyntaxKind::HeredocText, body_end - body_start);
        }
        if let Some(end) = terminator_end {
            self.push_raw(SyntaxKind::HeredocEnd, end - body_end);
        }
    }

    /// Add a token without letting it take part in word or delimiter bookkeeping.
    fn push_raw(&mut self, kind: SyntaxKind, len: u32) {
        if len == 0 {
            return;
        }
        self.at_word_start = true;
        self.out.push(Lexed { kind, len });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_comes_off_the_delimiter() {
        assert_eq!(unquote("EOF"), ("EOF".to_string(), false));
        assert_eq!(unquote("'EOF'"), ("EOF".to_string(), true));
        assert_eq!(unquote("\"EOF\""), ("EOF".to_string(), true));
        assert_eq!(unquote("\\EOF"), ("EOF".to_string(), true));
        assert_eq!(unquote("\"E\"OF"), ("EOF".to_string(), true));
    }
}
