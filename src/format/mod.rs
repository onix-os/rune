//! Formatting a script, by rewriting only the space between its tokens.
//!
//! # Why this is a walk and not a printer
//!
//! A formatter usually has to be a second implementation of the language: it takes an abstract tree
//! that threw the source away and prints a program back out, and every construct it forgets is a
//! construct it silently deletes. **The lossless tree makes that unnecessary.** Every byte of the
//! input is in it, so the significant text can be copied out verbatim and the only thing this
//! decides is what goes *between* — indentation, spaces, line breaks, blank lines.
//!
//! What follows from that is the safety argument. A construct this walk has never heard of is
//! emitted as its own source text (see [`Fmt::verbatim`]), which is exactly what it was. There is no
//! shape of program that comes out changed because it was not anticipated; the worst that can happen
//! to one is that it comes out unimproved.
//!
//! # The two invariants
//!
//! * **Idempotence.** `format(format(x)) == format(x)`.
//! * **Meaning is preserved.** `parse(format(x))` has the same *node* tree as `parse(x)`, and the
//!   same sequence of significant token texts. Only trivia and separators may differ.
//!
//! Both are asserted over the corpus in `tests/`, which is what makes them properties rather than
//! intentions.
//!
//! # What it will not touch
//!
//! The text of a word, a quoted string, a comment, an expansion, a `[[ ]]` test, or a here-document
//! body. A here-document body is the sharpest case: those bytes are *data*, they must begin in
//! column zero, and a formatter that indented them has changed what the program does. See
//! [`out::Out::verbatim_block`].
//!
//! A script that does not parse is **refused rather than formatted** — [`format`] hands back the
//! errors. Reformatting around a mistake is how a formatter turns a typo into a lost afternoon.

mod command;
mod compound;
mod out;

use crate::error::Error;
use crate::source::Source;
use crate::span::Span;
use crate::tree::{Element, Node, SyntaxKind as K, Tree};
use out::Out;

/// How the output should look. Everything here is a matter of taste rather than of meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// One level of indentation, written out. Four spaces unless something says otherwise.
    pub indent: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            indent: "    ".to_string(),
        }
    }
}

/// Format a script, or hand back the reasons it could not be parsed.
///
/// **Refusing is the point.** A script with a missing `fi` has no tree worth reformatting, and the
/// output would be a second mistake laid over the first.
pub fn format(text: &str) -> Result<String, Vec<Error>> {
    format_with(text, &Options::default())
}

/// [`format`], with the layout said out loud.
pub fn format_with(text: &str, options: &Options) -> Result<String, Vec<Error>> {
    let parsed = crate::parse(text);
    if !parsed.is_clean() {
        return Err(parsed.errors().to_vec());
    }
    Ok(format_tree(parsed.tree(), options))
}

/// Format a tree that has already been parsed.
///
/// No refusal here: a caller holding a tree has already decided what to do about its errors, and an
/// error node formats as the text it covers.
pub fn format_tree(tree: &Tree, options: &Options) -> String {
    let mut fmt = Fmt {
        source: tree.source(),
        out: Out::new(options.indent.clone()),
        last_line: 0,
    };
    fmt.script(tree.root());
    fmt.out.finish()
}

/// How a list of commands is being laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    /// One command per line, indented — a body.
    Block,
    /// All on one line, separated by `; ` — the condition of an `if`, the body of a `case` arm that
    /// was written on one line.
    Inline,
}

/// The walk itself.
pub(super) struct Fmt<'a> {
    source: &'a Source,
    out: Out,
    /// The source line the last thing written came from.
    ///
    /// The one piece of the *input's* layout this keeps, and it exists for comments: a comment
    /// after a command belongs on that command's line, and a comment on a line of its own belongs
    /// on a line of its own. Nothing else can tell those apart.
    last_line: u32,
}

impl<'a> Fmt<'a> {
    fn slice(&self, span: Span) -> &'a str {
        self.source.slice(span)
    }

    /// The source text a span covers.
    pub(super) fn source_text(&self, span: Span) -> &'a str {
        self.source.slice(span)
    }

    /// The line a node's last byte is on.
    pub(super) fn line_of_end(&self, node: &Node) -> u32 {
        self.source.line_of(node.span().end.saturating_sub(1))
    }

    /// The line a node begins on.
    pub(super) fn line_of_start(&self, node: &Node) -> u32 {
        self.source.line_of(node.span().start)
    }

    /// A here-document body or its closing line, exactly as it was.
    ///
    /// **It can turn up beside anything.** The body belongs to the line that opened it, not to the
    /// command — so in `if a; then cat <<E … fi` the two tokens are children of the `if`, and in a
    /// plain script they are children of the list. Every walk that iterates children has to know
    /// about them, which is why this is one call rather than a rule in one place.
    pub(super) fn heredoc(&mut self, span: Span) {
        self.out.verbatim_block(self.slice(span));
        self.last_line = self.source.line_of(span.end.saturating_sub(1));
    }

    /// Start a new line for something that begins on `line` in the source, keeping a blank line if
    /// there was one.
    ///
    /// **Counted in source lines rather than in newline tokens.** A `&` ends its command *and*
    /// leaves the newline after it in the list, where a `;` does not — so counting tokens made
    /// `a &` gain a blank line under it every time the file was formatted. Lines cannot disagree
    /// with themselves that way.
    pub(super) fn break_before(&mut self, line: u32) {
        match line > self.last_line + 1 {
            true => self.out.blank(),
            false => self.out.line(),
        }
    }

    /// Write a span's own text, and remember which line it came from.
    pub(super) fn token(&mut self, span: Span) {
        self.out.push(self.slice(span));
        self.last_line = self.source.line_of(span.end.saturating_sub(1));
    }

    /// Write an element exactly as it was written.
    ///
    /// The fallback for everything this walk does not lay out itself — a `[[ ]]` test, an
    /// arithmetic command, a region that did not parse. It cannot be wrong, only unimproved.
    pub(super) fn verbatim(&mut self, element: &Element) {
        self.token(element.span());
    }

    fn script(&mut self, root: &Node) {
        for child in root.children() {
            match child.kind() {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                _ => match child {
                    Element::Node(node) if node.kind() == K::CommandList => {
                        self.command_list(node, Layout::Block);
                    }
                    other => self.verbatim(other),
                },
            }
        }
    }

    /// A comment, on the line it was written on.
    ///
    /// **Trailing or its own line, and never the other one.** `echo a # why` says something about
    /// that command; a comment on a line of its own says something about what follows. Moving one
    /// to where the other goes changes what it appears to be about, which is the one thing a
    /// formatter must not do to prose.
    pub(super) fn comment(&mut self, span: Span) {
        let line = self.source.line_of(span.start);
        if line == self.last_line && !self.out.breaking() {
            self.out.space();
        } else {
            self.out.line();
        }
        self.out.push(self.slice(span).trim_end());
        self.out.line();
        self.last_line = line;
    }

    /// A list of commands.
    pub(super) fn command_list(&mut self, node: &Node, layout: Layout) {
        let mut started = false;
        for child in node.children() {
            match child.kind() {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                K::ListItem => {
                    if started {
                        match layout {
                            Layout::Block => {
                                self.break_before(self.source.line_of(child.span().start));
                            }
                            Layout::Inline => {
                                self.out.push(";");
                                self.out.space();
                            }
                        }
                    }
                    started = true;
                    if let Some(item) = child.as_node() {
                        self.list_item(item, layout);
                    }
                }
                _ => self.verbatim(child),
            }
        }
    }

    /// One command and whatever ended it.
    ///
    /// The terminator is the interesting part. `;` before a line break says nothing the line break
    /// does not, so it goes; `&` says the command runs in the background, so it stays wherever it
    /// was written.
    fn list_item(&mut self, node: &Node, layout: Layout) {
        for child in node.children() {
            match child.kind() {
                K::Whitespace | K::LineContinuation | K::Newline => {}
                K::Comment => self.comment(child.span()),
                K::HeredocText | K::HeredocEnd => self.heredoc(child.span()),
                // Dropped in both layouts: `command_list` writes the separator it wants.
                K::Semi => {}
                K::Amp | K::SemiAmp | K::SemiSemiAmp => {
                    self.out.space();
                    self.token(child.span());
                }
                _ => match child {
                    Element::Node(command) => self.command(command),
                    token => self.verbatim(token),
                },
            }
        }
        let _ = layout;
    }

    /// One command, of whatever shape.
    pub(super) fn command(&mut self, node: &Node) {
        match node.kind() {
            K::AndOrList => self.and_or(node),
            K::Pipeline => self.pipeline(node),
            K::SimpleCommand => self.simple_command(node),
            K::IfCommand
            | K::ElifClause
            | K::WhileCommand
            | K::UntilCommand
            | K::ForCommand
            | K::SelectCommand
            | K::CaseCommand
            | K::ElseClause => self.compound(node),
            K::FunctionDef => self.function_def(node),
            K::Group => self.bracketed(node, K::LBrace, K::RBrace),
            K::Subshell => self.bracketed(node, K::LParen, K::RParen),
            // `[[ ]]`, `(( ))`, an arithmetic `for`, an error node: written as they were.
            _ => self.token(node.span()),
        }
    }

    /// Whether this can be written on one line, and was.
    ///
    /// Used wherever the author's own choice between one line and several is worth keeping — a
    /// `case` arm, a `{ ...; }` group. A formatter that expanded every one of those would rewrite
    /// half of every script to say the same thing.
    ///
    /// **Was written on one line is not enough.** `(while true; do echo x; done)` is one line, but
    /// a `while` inside it puts its body on lines of its own whatever the bracket around it wanted —
    /// so the bracket asked for one line, got several, and formatting the result chose differently
    /// the second time. The layout has to agree with what the contents will actually do.
    pub(super) fn on_one_line(&self, node: &Node) -> bool {
        let span = node.span();
        self.source.line_of(span.start) == self.source.line_of(span.end.saturating_sub(1))
            && !forces_lines(node)
    }
}

/// Whether something inside this will take lines of its own no matter what is asked of it.
///
/// The compound commands, which always put their body on its own lines. Words and the things
/// written verbatim are not looked into: what is inside them is text, and text has no layout here.
fn forces_lines(node: &Node) -> bool {
    node.nodes().any(|child| match child.kind() {
        K::IfCommand
        | K::ElifClause
        | K::ElseClause
        | K::WhileCommand
        | K::UntilCommand
        | K::ForCommand
        | K::ArithForCommand
        | K::SelectCommand
        | K::CaseCommand
        | K::FunctionDef => true,
        K::Word | K::Assignment | K::ArrayValue | K::CondCommand | K::ArithCommand => false,
        _ => forces_lines(child),
    })
}

#[cfg(test)]
mod tests;
