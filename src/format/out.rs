//! The buffer a formatter writes into.
//!
//! Nothing here knows about shell. It knows that a space between two things is not a space at the
//! start of a line, that two blank lines are one blank line, and that a here-document body has to
//! begin in column zero whatever the indentation around it was — which are the three rules every
//! line of the walk would otherwise have to remember for itself.
//!
//! **Whitespace is requested, not written.** [`Out::space`] and [`Out::line`] record what *would*
//! separate the next thing from the last, and the separator is only settled when something actually
//! arrives. That is what makes a trailing space impossible to write and a run of blank lines at the
//! end of a file impossible to keep.

/// A growing formatted script.
pub(super) struct Out {
    text: String,
    /// One level of indentation, already expanded.
    unit: String,
    depth: usize,
    /// Line breaks waiting to be written: 0, 1, or 2 — two being one blank line.
    breaks: usize,
    /// A space waiting to be written, if anything comes to write it before.
    space: bool,
    /// Whether anything at all has been written. Leading blank lines are dropped rather than kept.
    started: bool,
    /// Whether the current line is still empty, which is when a space is not a separator.
    at_start: bool,
}

impl Out {
    pub(super) fn new(unit: String) -> Self {
        Self {
            text: String::new(),
            unit,
            depth: 0,
            breaks: 0,
            space: false,
            started: false,
            at_start: true,
        }
    }

    pub(super) fn indent(&mut self) {
        self.depth += 1;
    }

    pub(super) fn dedent(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// A space before the next thing, if the next thing does not begin a line.
    pub(super) fn space(&mut self) {
        self.space = true;
    }

    /// End the line.
    ///
    /// **Nothing to end is nothing to do.** A here-document body leaves the cursor in column zero
    /// of a line nothing has been written on, and a break asked for there would have opened a blank
    /// line under every here-document in the file.
    pub(super) fn line(&mut self) {
        if self.started && !self.at_start {
            self.breaks = self.breaks.max(1);
        }
        self.space = false;
    }

    /// End the line and leave one blank line after it — never more, and never at the top.
    pub(super) fn blank(&mut self) {
        if self.started {
            self.breaks = if self.at_start { 1 } else { 2 };
        }
        self.space = false;
    }

    /// Whether a line break is already waiting, which is how the walk tells that the thing it is
    /// about to write will start a line of its own.
    pub(super) fn breaking(&self) -> bool {
        self.breaks > 0 || self.at_start
    }

    /// Write something, settling whatever separator was owed first.
    pub(super) fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.started {
            self.started = true;
            self.breaks = 0;
            self.space = false;
        }
        if self.breaks > 0 {
            for _ in 0..self.breaks {
                self.text.push('\n');
            }
            self.breaks = 0;
            self.at_start = true;
            self.space = false;
        }
        if self.at_start {
            for _ in 0..self.depth {
                self.text.push_str(&self.unit);
            }
            self.at_start = false;
        } else if self.space {
            self.text.push(' ');
        }
        self.space = false;
        self.text.push_str(text);
    }

    /// Write text that must keep every byte it has, starting in column zero.
    ///
    /// A here-document body and its delimiter line: the bytes between `<<EOF` and `EOF` are data,
    /// and a formatter that indented them would have changed the program. The text carries its own
    /// newlines, so nothing here adds any.
    pub(super) fn verbatim_block(&mut self, text: &str) {
        if !self.started {
            self.started = true;
        } else if !self.at_start || self.breaks > 0 {
            self.text.push('\n');
        }
        self.breaks = 0;
        self.space = false;
        self.text.push_str(text);
        self.at_start = self.text.ends_with('\n');
    }

    /// The script, ending in exactly one newline — or empty, if nothing was written.
    pub(super) fn finish(mut self) -> String {
        if !self.started {
            return String::new();
        }
        if !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out() -> Out {
        Out::new("    ".to_string())
    }

    /// A space that nothing follows is never written, which is what makes a trailing space
    /// unrepresentable rather than something to strip afterwards.
    #[test]
    fn an_unclaimed_space_is_never_written() {
        let mut out = out();
        out.push("echo");
        out.space();
        out.line();
        assert_eq!(out.finish(), "echo\n");
    }

    /// Indentation belongs to the first thing on a line, so an empty line has none.
    #[test]
    fn a_blank_line_carries_no_indentation() {
        let mut out = out();
        out.push("a");
        out.indent();
        out.blank();
        out.push("b");
        assert_eq!(out.finish(), "a\n\n    b\n");
    }

    #[test]
    fn blank_lines_never_pile_up_or_lead() {
        let mut out = out();
        out.blank();
        out.blank();
        out.push("a");
        out.blank();
        out.blank();
        out.push("b");
        out.blank();
        assert_eq!(out.finish(), "a\n\nb\n");
    }

    /// The body starts in column zero whatever the indentation was, or it is not the same body.
    #[test]
    fn a_verbatim_block_ignores_the_indentation_around_it() {
        let mut out = out();
        out.indent();
        out.push("cat <<EOF");
        out.line();
        out.verbatim_block("  body\nEOF\n");
        out.push("done");
        assert_eq!(out.finish(), "    cat <<EOF\n  body\nEOF\n    done\n");
    }
}
