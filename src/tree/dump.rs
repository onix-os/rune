//! An indented rendering of a tree, for tests and for looking at one.
//!
//! Every line is `Kind@start..end`, and tokens carry their text quoted after it. Tests compare
//! against this rather than against nested constructors, so a test reads like the tree it asserts.

use super::{Element, Node, Tree};
use crate::source::Source;

pub(super) fn dump(tree: &Tree) -> String {
    let mut out = String::new();
    node(&mut out, tree.root(), tree.source(), 0);
    out
}

fn node(out: &mut String, node: &Node, source: &Source, depth: usize) {
    indent(out, depth);
    out.push_str(&format!("{:?}@{}\n", node.kind(), node.span()));
    for child in node.children() {
        match child {
            Element::Node(inner) => self::node(out, inner, source, depth + 1),
            Element::Token(token) => {
                indent(out, depth + 1);
                let text = token.text(source);
                out.push_str(&format!("{:?}@{} {:?}\n", token.kind(), token.span(), text));
            }
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use crate::source::Source;
    use crate::tree::{Builder, SyntaxKind};

    #[test]
    fn renders_nesting_and_text() {
        let mut builder = Builder::new(Source::new("echo hi"), SyntaxKind::Script);
        builder.start(SyntaxKind::SimpleCommand);
        builder.start(SyntaxKind::Word);
        builder.token(SyntaxKind::Text, 4);
        builder.finish();
        builder.token(SyntaxKind::Whitespace, 1);
        builder.finish();
        builder.token(SyntaxKind::Text, 2);

        assert_eq!(
            builder.build().dump(),
            "Script@0..7\n  \
               SimpleCommand@0..5\n    \
                 Word@0..4\n      \
                   Text@0..4 \"echo\"\n    \
                 Whitespace@4..5 \" \"\n  \
               Text@5..7 \"hi\"\n"
        );
    }
}
