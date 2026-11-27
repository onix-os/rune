//! Here-documents, which is where a line-at-a-time lexer usually comes apart.
//!
//! `<<EOF` says nothing about the text next to it. The body begins on the *following* line, so the
//! lexer has to note the delimiter, carry on to the end of the current line, and only then change
//! what it is reading. More than one can be queued at once — `cat <<A <<B` takes two bodies, in
//! the order the operators appeared.

use super::unquote::unquote;
use super::{Lexed, Lexer};
use crate::tree::SyntaxKind;
use std::borrow::Cow;

/// A body the lexer owes, once it reaches the end of the line that asked for it.
pub(super) struct Heredoc {
    /// The delimiter with its quoting removed, which is what the terminator line is compared to.
    delimiter: String,
    /// `<<-`: leading tabs come off every line, the terminator included.
    strip_tabs: bool,
    /// Whether any of the delimiter was quoted, which leaves the body literal.
    quoted: bool,
    /// Where the `<<` is, so a body that never ends can point back at what asked for it.
    at: u32,
}

/// Whether a line ends in a backslash that is not itself escaped.
fn continues(line: &str) -> bool {
    line.bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 1
}

impl<'a> Lexer<'a> {
    /// Note that the word coming up names a here-document delimiter.
    pub(super) fn expect_delimiter(&mut self, strip_tabs: bool) {
        self.awaiting_delimiter = Some((strip_tabs, self.token_start()));
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
        if kind.is_word_piece() {
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
        let Some((strip_tabs, at)) = self.awaiting_delimiter.take() else {
            return;
        };
        if self.delimiter_text.is_empty() {
            return;
        }
        let (delimiter, quoted) = unquote(&self.delimiter_text);
        self.delimiter_text.clear();
        self.heredocs.push(Heredoc {
            delimiter,
            strip_tabs,
            quoted,
            at,
        });
    }

    /// Read every body owed, now that the line asking for them has ended.
    pub(super) fn take_heredoc_bodies(&mut self) {
        while !self.heredocs.is_empty() {
            let doc = self.heredocs.remove(0);
            self.read_body(&doc);
        }
    }

    /// The next line of a body as the shell reads it, and how many bytes of source it takes.
    ///
    /// **A body whose delimiter is unquoted joins a line ending in `\` to the next one**, before
    /// it is compared with the delimiter: in `abc\` followed by `EOF`, the line read is `abcEOF`
    /// and the body goes on. A quoted delimiter leaves every backslash where it is.
    fn body_line(&self, doc: &Heredoc) -> (usize, Cow<'a, str>) {
        let rest = self.cursor.rest();
        let mut taken = 0;
        let mut joined: Option<String> = None;
        loop {
            let tail = rest.get(taken..).unwrap_or("");
            let line = tail.find('\n').map_or(tail, |at| &tail[..=at]);
            let content = line.strip_suffix('\n').unwrap_or(line);
            taken += line.len();
            let more = !doc.quoted && line.ends_with('\n') && continues(content);
            if !more {
                return match joined {
                    Some(mut text) => {
                        text.push_str(content);
                        (taken, Cow::Owned(text))
                    }
                    None => (taken, Cow::Borrowed(content)),
                };
            }
            let text = joined.get_or_insert_with(String::new);
            text.push_str(&content[..content.len() - 1]);
            if taken >= rest.len() {
                return (taken, Cow::Owned(text.clone()));
            }
        }
    }

    fn read_body(&mut self, doc: &Heredoc) {
        let body_start = self.cursor.offset();
        loop {
            if self.cursor.is_eof() {
                // The file ended before the delimiter did. bash warns and uses what it read.
                self.note_unclosed("<<", doc.at);
                self.close_body(body_start, self.cursor.offset(), None);
                return;
            }
            let (length, content) = self.body_line(doc);
            let compared = if doc.strip_tabs {
                content.trim_start_matches('\t')
            } else {
                &content
            };
            if compared == doc.delimiter {
                let end = self.cursor.offset();
                self.cursor.advance(length);
                self.close_body(body_start, end, Some(self.cursor.offset()));
                return;
            }
            // **Inside `` `…` ``, the closing backquote may share the delimiter's line.** The old
            // form is read by finding its matching backquote and parsing what lies between, so
            // ``x=`cat <<E … E` `` is a here-document whose terminator is `E` and a backquote that
            // belongs to the substitution around it. Compared strictly the line spells ``E` ``,
            // nothing ever matched, and the body ran to the end of the file. Only the delimiter is
            // taken here; the backquote is left to be lexed as what it is.
            if self.inside_backtick()
                && let Some(after) = compared.strip_prefix(doc.delimiter.as_str())
                && after.starts_with('`')
            {
                let indent = content.len() - compared.len();
                let end = self.cursor.offset();
                self.cursor.advance(indent + doc.delimiter.len());
                self.close_body(body_start, end, Some(self.cursor.offset()));
                return;
            }
            self.cursor.advance(length);
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
    use super::continues;
    use crate::lex::lex;
    use crate::tree::SyntaxKind;

    #[test]
    fn only_an_unescaped_backslash_continues_a_line() {
        assert!(continues("abc\\"));
        assert!(continues("abc\\\\\\"));
        assert!(!continues("abc\\\\"));
        assert!(!continues("abc"));
    }

    /// The text of each body and terminator, in order.
    fn bodies(text: &str) -> Vec<(SyntaxKind, String)> {
        let mut at = 0;
        let mut out = Vec::new();
        for token in lex(text).tokens {
            let end = at + token.len as usize;
            if matches!(token.kind, SyntaxKind::HeredocText | SyntaxKind::HeredocEnd) {
                out.push((token.kind, text[at..end].to_string()));
            }
            at = end;
        }
        out
    }

    #[test]
    fn a_continued_line_is_compared_whole() {
        let text = "cat <<EOF\nabc\\\nEOF\nEOF\n";
        assert_eq!(
            bodies(text),
            [
                (SyntaxKind::HeredocText, "abc\\\nEOF\n".to_string()),
                (SyntaxKind::HeredocEnd, "EOF\n".to_string()),
            ]
        );
        // Quoted, the backslash is data and the first `EOF` ends the body.
        let text = "cat <<'EOF'\nabc\\\nEOF\nEOF\n";
        assert_eq!(
            bodies(text)[1],
            (SyntaxKind::HeredocEnd, "EOF\n".to_string())
        );
        assert_eq!(bodies(text)[0].1, "abc\\\n");
    }

    #[test]
    fn a_quoted_delimiter_is_compared_without_its_quoting() {
        for text in [
            "cat <<$'EOF'\nbody\nEOF\n",
            "cat <<'E\"OF'\nbody\nE\"OF\n",
            "cat <<\"E\\\"F\"\nbody\nE\"F\n",
        ] {
            assert!(lex(text).unclosed.is_empty(), "{text:?}");
            assert_eq!(bodies(text).len(), 2, "{text:?}");
        }
    }
}
