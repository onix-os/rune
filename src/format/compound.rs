//! Commands that are a block: `if`, `while`, `for`, `select`, `case`, and their clauses.
//!
//! **One walk for all of them.** They differ in which words they use, not in what those words do:
//! something opens a header, something ends the header and opens a body, something closes the body.
//! Writing five nearly identical walks is how the fifth one ends up subtly different from the other
//! four, so the shape is written once and the keywords say which part they play.

use super::{Fmt, Layout};
use crate::tree::{Element, Node, SyntaxKind as K};

impl Fmt<'_> {
    /// A compound command, from its opening word to its closing one.
    pub(super) fn compound(&mut self, node: &Node) {
        let mut in_body = false;
        let mut started_body = false;
        for child in node.children() {
            let kind = child.kind();
            match kind {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                // The word that opens a header. Everything until `then`, `do` or `in` is the
                // condition, the variable and its list, or the word a `case` chooses on.
                K::If | K::Elif | K::While | K::Until | K::For | K::Select | K::Case => {
                    self.token(child.span());
                    self.out.space();
                }
                // `in` is two different words. In a `for` it is part of the header; in a `case` it
                // is the word that opens the body, which is why this cannot be decided by the token.
                K::In => {
                    self.out.space();
                    self.token(child.span());
                    match node.kind() == K::CaseCommand {
                        true => {
                            self.out.indent();
                            self.out.line();
                            in_body = true;
                        }
                        false => self.out.space(),
                    }
                }
                // **The `;` is written here rather than kept.** Whatever ended the header — a
                // semicolon, a newline, nothing at all because `then` was on its own line — comes
                // out the same way, and the header keeps its own line.
                K::Then | K::Do => {
                    self.out.push(";");
                    self.out.space();
                    self.token(child.span());
                    self.out.indent();
                    self.out.line();
                    in_body = true;
                }
                K::Fi | K::Done | K::Esac => {
                    self.out.dedent();
                    self.out.line();
                    self.token(child.span());
                    in_body = false;
                }
                K::Else => {
                    self.token(child.span());
                    self.out.indent();
                    self.out.line();
                    in_body = true;
                }
                // Already said by the layout; see the `Then` arm.
                K::Semi => {}
                _ => {
                    match child {
                        // A clause of its own. It leaves the indentation one level in, which is
                        // where its own body belongs and where the closing word will find it.
                        Element::Node(clause)
                            if matches!(clause.kind(), K::ElifClause | K::ElseClause) =>
                        {
                            self.out.dedent();
                            self.out.line();
                            self.compound(clause);
                        }
                        Element::Node(arm) if arm.kind() == K::CaseItem => {
                            if started_body {
                                self.break_before(self.line_of_start(arm));
                            }
                            started_body = true;
                            self.case_item(arm);
                        }
                        Element::Node(list) if list.kind() == K::CommandList => {
                            let layout = match in_body {
                                true => Layout::Block,
                                false => Layout::Inline,
                            };
                            self.command_list(list, layout);
                        }
                        Element::Node(part) if part.kind() == K::Redirect => {
                            let written = self.redirect(part);
                            self.out.space();
                            self.out.push(&written);
                            self.last_line = self.line_of_end(part);
                        }
                        // A word of the header, an arithmetic `(( ))`, anything unanticipated.
                        Element::Node(part) => {
                            self.out.space();
                            self.token(part.span());
                        }
                        token => {
                            self.out.space();
                            self.verbatim(token);
                        }
                    }
                }
            }
        }
    }

    /// One arm of a `case`.
    ///
    /// **One line or several, as it was written.** A table of short arms is a table, and expanding
    /// every one of them to three lines turns something readable into something to scroll past.
    /// The author already decided; this only lines it up.
    pub(super) fn case_item(&mut self, node: &Node) {
        let inline = self.on_one_line(node);
        let mut indented = false;
        for child in node.children() {
            match child.kind() {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                // Welded to the patterns on either side: `a|b)` is one thing to read.
                K::Pipe | K::LParen => self.token(child.span()),
                K::RParen => {
                    self.token(child.span());
                    match inline {
                        true => self.out.space(),
                        false => {
                            self.out.indent();
                            indented = true;
                            self.out.line();
                        }
                    }
                }
                K::SemiSemi | K::SemiAmp | K::SemiSemiAmp => {
                    match inline {
                        true => self.out.space(),
                        false => self.out.line(),
                    }
                    self.token(child.span());
                    if indented {
                        self.out.dedent();
                        indented = false;
                    }
                }
                _ => match child {
                    Element::Node(pattern) if pattern.kind() == K::CasePattern => {
                        self.token(pattern.span());
                    }
                    Element::Node(body) if body.kind() == K::CommandList => {
                        let layout = match inline {
                            true => Layout::Inline,
                            false => Layout::Block,
                        };
                        self.command_list(body, layout);
                    }
                    other => {
                        self.out.space();
                        self.verbatim(other);
                    }
                },
            }
        }
        // The last arm may leave its `;;` off, and the indentation still has to come back.
        if indented {
            self.out.dedent();
        }
    }
}
