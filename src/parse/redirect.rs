//! Redirections, and the file descriptor that can be glued to the front of one.

use super::Parser;
use crate::tree::SyntaxKind;

impl Parser<'_> {
    pub(super) fn at_redirect(&self) -> bool {
        self.peek().is_some_and(SyntaxKind::is_redirect_operator) || self.at_fd_prefix()
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
            .is_some_and(|next| next.kind.is_redirect_operator())
    }

    /// The redirections written after a compound command, as in `while …; done > log`.
    ///
    /// They belong to the construct they follow, so they are taken *inside* its node rather than
    /// left to look like a statement of their own — which is what `} 2>&1` would otherwise be, and
    /// then the stream it was redirecting went to the terminal instead of down the pipe.
    pub(super) fn trailing_redirects(&mut self) {
        while self.at_redirect() {
            let before = self.progress();
            self.redirect();
            if self.progress() == before {
                return;
            }
        }
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
