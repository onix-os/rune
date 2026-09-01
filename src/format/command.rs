//! Commands that are a line: a simple command, a pipeline, an `&&` list, a redirection.

use super::{Fmt, Layout};
use crate::tree::{Element, Node, SyntaxKind as K};

impl Fmt<'_> {
    /// `a && b`, `a || b`.
    ///
    /// **A break the author wrote is a break that stays.** A chain of four commands joined by `&&`
    /// is a paragraph, and whether it was one line or four is a decision about reading it that no
    /// rule here is in a position to overturn. What this settles is the *indentation* of the
    /// continuation, which nothing but a formatter ever gets right by hand.
    pub(super) fn and_or(&mut self, node: &Node) {
        self.joined(node, |kind| matches!(kind, K::AndAnd | K::PipePipe));
    }

    /// `a | b`, and the `!` or `time` in front of it.
    pub(super) fn pipeline(&mut self, node: &Node) {
        self.joined(node, |kind| matches!(kind, K::Pipe | K::PipeAmp));
    }

    /// The shape `and_or` and `pipeline` share: commands with an operator between them.
    fn joined(&mut self, node: &Node, is_operator: fn(K) -> bool) {
        let children: Vec<&Element> = node.children().collect();
        let mut indented = false;
        for (at, child) in children.iter().enumerate() {
            let kind = child.kind();
            match kind {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                K::Bang | K::Time | K::Coproc => {
                    self.out.space();
                    self.token(child.span());
                    self.out.space();
                }
                _ if is_operator(kind) => {
                    self.out.space();
                    self.token(child.span());
                    if broken_after(&children, at + 1) {
                        if !indented {
                            self.out.indent();
                            indented = true;
                        }
                        self.out.line();
                    } else {
                        self.out.space();
                    }
                }
                _ => match child {
                    Element::Node(command) => self.command(command),
                    token => {
                        self.out.space();
                        self.verbatim(token);
                    }
                },
            }
        }
        if indented {
            self.out.dedent();
        }
    }

    /// `cmd arg arg >file`.
    ///
    /// A backslash-newline is kept for the same reason a break in an `&&` chain is: somebody split
    /// a long invocation on purpose, and the split is the readable part. The indentation under it
    /// is this walk's to decide.
    pub(super) fn simple_command(&mut self, node: &Node) {
        let mut indented = false;
        for child in node.children() {
            match child.kind() {
                K::Whitespace | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                K::LineContinuation => {
                    self.out.space();
                    self.out.push("\\");
                    if !indented {
                        self.out.indent();
                        indented = true;
                    }
                    self.out.line();
                }
                _ => {
                    self.out.space();
                    match child {
                        Element::Node(part) if part.kind() == K::Redirect => {
                            let written = self.redirect(part);
                            self.out.push(&written);
                            self.last_line = self.line_of_end(part);
                        }
                        // A word, an assignment, an array value: text this may not touch.
                        Element::Node(part) => self.token(part.span()),
                        token => self.verbatim(token),
                    }
                }
            }
        }
        if indented {
            self.out.dedent();
        }
    }

    /// `2>&1`, `>file`, `<<EOF` — written as one piece.
    ///
    /// **Glued, because the space is where the meaning goes.** `2 >&1` is a command taking `2` as an
    /// argument and sending its output to fd 1, which is not what `2>&1` says at all. Writing the
    /// whole redirection with no spaces in it makes that class of mistake unreachable.
    ///
    /// The one place a space goes back in is before a target that would otherwise weld itself to the
    /// operator: `> >(tee log)` glued is `>>(tee log)`, an append to a file whose name starts with a
    /// bracket. The test is on the target's first character, so it covers every operator at once.
    pub(super) fn redirect(&mut self, node: &Node) -> String {
        let mut head = String::new();
        let mut target = String::new();
        let mut past_operator = false;
        for child in node.children() {
            if matches!(
                child.kind(),
                K::Whitespace | K::LineContinuation | K::Newline | K::Comment
            ) {
                continue;
            }
            let text = self.source_text(child.span());
            if past_operator {
                target.push_str(text);
            } else {
                head.push_str(text);
                past_operator = child.kind().is_redirect_operator();
            }
        }
        match target.starts_with(['<', '>', '(', '&', '|']) {
            true => format!("{head} {target}"),
            false => format!("{head}{target}"),
        }
    }

    /// `{ ... }` and `( ... )`, and whether they stay on one line.
    pub(super) fn bracketed(&mut self, node: &Node, open: K, close: K) {
        let inline = self.on_one_line(node);
        for child in node.children() {
            let kind = child.kind();
            match kind {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                _ if kind == open => {
                    self.token(child.span());
                    // **Spaced, even though `(a && b)` reads better.** Nested subshells are the
                    // reason: `( ( echo x ) )` welded is `((echo x))`, which is not two subshells
                    // at all — `((` opens an arithmetic command, and the program stops meaning what
                    // it said. One space costs nothing and makes that unreachable.
                    match inline {
                        true => self.out.space(),
                        false => {
                            self.out.indent();
                            self.out.line();
                        }
                    }
                }
                _ if kind == close => {
                    match inline {
                        // **A `}` needs something before it.** `{ a }` is not a group at all —
                        // `}` there is an argument to `a` — so the terminator the inline layout
                        // dropped has to come back before the brace.
                        true if close == K::RBrace => {
                            self.out.push(";");
                            self.out.space();
                        }
                        true => self.out.space(),
                        false => {
                            self.out.dedent();
                            self.out.line();
                        }
                    }
                    self.token(child.span());
                }
                _ => match child {
                    Element::Node(body) if body.kind() == K::CommandList => {
                        self.command_list(
                            body,
                            if inline {
                                Layout::Inline
                            } else {
                                Layout::Block
                            },
                        );
                    }
                    Element::Node(part) if part.kind() == K::Redirect => {
                        let written = self.redirect(part);
                        self.out.space();
                        self.out.push(&written);
                        self.last_line = self.line_of_end(part);
                    }
                    other => {
                        self.out.space();
                        self.verbatim(other);
                    }
                },
            }
        }
    }

    /// `f() { ... }` and `function f { ... }`.
    pub(super) fn function_def(&mut self, node: &Node) {
        for child in node.children() {
            match child.kind() {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                // Welded to the name, because `f ()` and `f()` are the same definition and only one
                // of them looks like one.
                K::LParen | K::RParen => self.token(child.span()),
                K::Function => {
                    self.token(child.span());
                    self.out.space();
                }
                _ => match child {
                    Element::Node(name) if name.kind() == K::Word => self.token(name.span()),
                    Element::Node(body) => {
                        self.out.space();
                        self.command(body);
                    }
                    token => {
                        self.out.space();
                        self.verbatim(token);
                    }
                },
            }
        }
    }
}

/// Whether a line break comes before the next thing worth writing.
fn broken_after(children: &[&Element], from: usize) -> bool {
    children[from..]
        .iter()
        .find(|child| {
            !matches!(
                child.kind(),
                K::Whitespace | K::LineContinuation | K::Comment
            )
        })
        .is_some_and(|child| child.kind() == K::Newline)
}
