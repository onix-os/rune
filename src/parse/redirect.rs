//! Redirections, and the file descriptor that can be glued to the front of one.

use super::Parser;
use crate::tree::SyntaxKind;

/// Whether `text` is `{name}` or `{name[subscript]}`: a variable a redirection keeps its
/// descriptor in.
fn is_fd_variable(text: &str) -> bool {
    let Some(inner) = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return false;
    };
    let name = match inner.split_once('[') {
        Some((name, subscript)) if subscript.ends_with(']') => name,
        Some(_) => return false,
        None => inner,
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

impl Parser<'_> {
    pub(super) fn at_redirect(&self) -> bool {
        self.peek().is_some_and(SyntaxKind::is_redirect_operator) || self.fd_prefix().is_some()
    }

    /// How many tokens name the descriptor in front of a redirection operator, if any do.
    ///
    /// `2>&1` — digits touching the operator name the descriptor being redirected. `{fd}>file`
    /// — a variable in braces touching it asks the shell to choose one and store it there, after a
    /// compound command as much as in a simple one: `} {fd}>log`.
    ///
    /// They have to be touching. `echo 2 >file` writes the word `2` and redirects; `echo 2>file`
    /// redirects descriptor 2 and writes nothing.
    fn fd_prefix(&self) -> Option<usize> {
        let first = self.significant(0)?;
        let mut text = String::new();
        for (count, token) in self.tokens[first..].iter().enumerate() {
            if token.kind.is_redirect_operator() {
                let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
                return (digits || is_fd_variable(&text)).then_some(count);
            }
            if token.kind != SyntaxKind::Text || (count > 0 && !text.starts_with('{')) {
                return None;
            }
            text.push_str(self.token_text(first + count));
        }
        None
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
        for _ in 0..self.fd_prefix().unwrap_or(0) {
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

#[cfg(test)]
mod tests {
    use super::is_fd_variable;

    #[test]
    fn a_descriptor_variable_is_a_name_in_braces() {
        for text in ["{fd}", "{_x1}", "{fd[1]}", "{a[$i]}"] {
            assert!(is_fd_variable(text), "{text}");
        }
        for text in ["{}", "{1}", "{a b}", "fd", "{fd", "{a[1}", "{a-b}"] {
            assert!(!is_fd_variable(text), "{text}");
        }
    }
}
