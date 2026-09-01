//! Redirections, and the file descriptor that can be glued to the front of one.

use super::Parser;
use crate::tree::SyntaxKind;

const OPERATORS: [SyntaxKind; 12] = [
    SyntaxKind::Less,
    SyntaxKind::Great,
    SyntaxKind::GreatGreat,
    SyntaxKind::LessGreat,
    SyntaxKind::LessAmp,
    SyntaxKind::GreatAmp,
    SyntaxKind::LessLess,
    SyntaxKind::LessLessDash,
    SyntaxKind::LessLessLess,
    SyntaxKind::GreatPipe,
    SyntaxKind::AmpGreat,
    SyntaxKind::AmpGreatGreat,
];

impl Parser<'_> {
    pub(super) fn at_redirect(&self) -> bool {
        self.peek().is_some_and(|kind| OPERATORS.contains(&kind)) || self.at_fd_prefix()
    }

    /// `2>&1` — digits touching a redirection operator name the descriptor being redirected.
    ///
    /// They have to be touching. `echo 2 >file` writes the word `2` and redirects; `echo 2>file`
    /// redirects descriptor 2 and writes nothing.
    fn at_fd_prefix(&self) -> bool {
        let Some(index) = self.significant(0) else {
            return false;
        };
        if self.tokens[index].kind != SyntaxKind::Text {
            return false;
        }
        let text = self.token_text(index);
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        self.tokens
            .get(index + 1)
            .is_some_and(|next| OPERATORS.contains(&next.kind))
    }

    pub(super) fn redirect(&mut self) {
        self.start(SyntaxKind::Redirect);
        if self.at_fd_prefix() {
            self.bump();
        }
        self.bump();
        if self.at_word() {
            self.word();
        } else {
            self.error("this redirection does not say what to redirect to");
        }
        self.finish_node();
    }
}
