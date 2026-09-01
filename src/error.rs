//! What went wrong, and where.
//!
//! An error here is a *record*, not a failure: parsing does not stop for one. The whole point of
//! the crate is that a script with nine mistakes in it reports nine, so these accumulate in a list
//! beside a tree that still covers the entire file.

use crate::span::Span;
use crate::tree::SyntaxKind;
use std::fmt;

/// How much a finding is worth interrupting someone for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Severity {
    /// The script will not run as written.
    #[default]
    Error,
    /// The script runs, but probably not as intended.
    Warning,
}

/// One thing the parser could not make sense of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// What to point at.
    ///
    /// For a construct that was never closed this is the **opener**, not the end of the file. A
    /// missing `fi` reported at the last line of a script says only that the script ended, which
    /// is the one thing the reader already knew; the `if` on line 2 is the thing to go and look at.
    pub span: Span,
    pub message: String,
    /// What would have been accepted here, if the parser knows.
    pub expected: Vec<SyntaxKind>,
    /// Where the construct that ran unfinished began.
    ///
    /// Kept for a report that wants a second label. It agrees with [`Error::span`] for the
    /// unclosed-construct errors, where the opener is also the place to point.
    pub opened_at: Option<Span>,
    /// Whether the input ran out while this was still open.
    ///
    /// **Recorded rather than inferred from the position.** An interactive prompt decides between
    /// reading another line and reporting a mistake on exactly this question, and it used to be
    /// answered by asking whether the error sat at the end of the input — which stopped being true
    /// the moment the errors started pointing at the construct instead.
    pub unfinished: bool,
    pub severity: Severity,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            expected: Vec::new(),
            opened_at: None,
            unfinished: false,
            severity: Severity::Error,
        }
    }

    /// Mark this as something another line of input would finish.
    pub fn unfinished(mut self) -> Self {
        self.unfinished = true;
        self
    }

    pub fn expecting(mut self, expected: impl IntoIterator<Item = SyntaxKind>) -> Self {
        self.expected.extend(expected);
        self
    }

    pub fn opened_at(mut self, span: Span) -> Self {
        self.opened_at = Some(span);
        self
    }

    pub fn warning(mut self) -> Self {
        self.severity = Severity::Warning;
        self
    }

    /// The expected kinds as they are written, for a message.
    pub fn expected_text(&self) -> Vec<&'static str> {
        self.expected
            .iter()
            .filter_map(|kind| kind.static_text())
            .collect()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Whether a fragment of shell is finished, and if not, why not.
///
/// An interactive prompt needs all three: keep reading, run it, or report it. The distinction
/// between the last two is the only reason a prompt can tell an unclosed `if` from a typo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    /// A whole program. Run it.
    Complete,
    /// Well-formed so far, but a construct is still open. Read another line.
    Unfinished,
    /// Not shell. Report it.
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_carries_what_it_wanted() {
        let error = Error::new(Span::new(9, 9), "this `if` was never closed")
            .expecting([SyntaxKind::Fi, SyntaxKind::Elif, SyntaxKind::Else])
            .opened_at(Span::new(0, 2));
        assert_eq!(error.expected_text(), ["fi", "elif", "else"]);
        assert_eq!(error.opened_at, Some(Span::new(0, 2)));
        assert_eq!(error.severity, Severity::Error);
        assert_eq!(error.to_string(), "this `if` was never closed");
    }

    #[test]
    fn kinds_without_a_fixed_spelling_are_left_out() {
        let error = Error::new(Span::empty(0), "x").expecting([SyntaxKind::Word, SyntaxKind::Do]);
        assert_eq!(error.expected_text(), ["do"]);
    }
}
