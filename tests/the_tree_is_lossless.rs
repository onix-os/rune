//! The invariant the whole crate rests on: a tree reproduces its source, byte for byte.
//!
//! There is no tokenizer yet, so these trees are built by hand — deliberately badly, in the shapes
//! a recovering parser produces when it is having a hard time. Once a real parser exists this file
//! grows a second half that parses text instead of inventing it, and asserts the same thing.

use rune::{Builder, Source, SyntaxKind, Tree};

/// Chop `text` into single-byte tokens under a few nested nodes, ignoring what shell means.
///
/// The point is that the *shape* of the tree cannot affect what it reconstructs to.
fn shred(text: &str) -> Tree {
    let source = Source::new(text);
    let mut builder = Builder::new(source, SyntaxKind::Script);
    let mut depth = 0;
    for (index, ch) in text.char_indices() {
        if index % 3 == 0 && depth < 4 {
            builder.start(SyntaxKind::Word);
            depth += 1;
        }
        builder.token(SyntaxKind::Text, ch.len_utf8() as u32);
        if index % 5 == 0 && depth > 0 {
            builder.finish();
            depth -= 1;
        }
    }
    builder.build()
}

const SCRIPTS: &[&str] = &[
    "",
    "\n",
    "echo hi",
    "echo hi\n",
    "if true; then echo yes; fi\n",
    "for i in 1 2 3; do\n  echo \"$i\"\ndone\n",
    "cat <<'EOF'\nliteral $body\nEOF\n",
    "a && b || c | d & e; f\n",
    "# just a comment\n",
    "echo 'unclosed\n",
    "echo \"nested $(echo $(echo deep))\"\n",
    "x=1 y=2 env\n",
    "echo é ü 日本語\n",
    "echo a\\\n b\n",
    "((((((\n",
    "\t \r\n  \t\n",
    "esac done fi )",
];

#[test]
fn every_script_reconstructs_exactly() {
    for script in SCRIPTS {
        let tree = shred(script);
        assert_eq!(
            tree.reconstruct(),
            *script,
            "reconstruction differs from the source for {script:?}"
        );
    }
}

#[test]
fn the_root_covers_the_whole_source() {
    for script in SCRIPTS {
        let tree = shred(script);
        assert_eq!(
            tree.root().span().start,
            0,
            "root does not start at 0 for {script:?}"
        );
        assert_eq!(
            tree.root().span().end,
            tree.source().len(),
            "root does not reach the end for {script:?}"
        );
    }
}

#[test]
fn tokens_are_contiguous_and_in_order() {
    for script in SCRIPTS {
        let tree = shred(script);
        let mut next = 0;
        tree.root().visit_tokens(&mut |token| {
            assert_eq!(
                token.span().start,
                next,
                "a gap or an overlap before {:?} in {script:?}",
                token.span()
            );
            next = token.span().end;
        });
        assert_eq!(
            next,
            tree.source().len(),
            "tokens stop short of the end for {script:?}"
        );
    }
}

#[test]
fn a_parser_that_loses_its_place_still_produces_the_whole_text() {
    // Half the input accounted for; `build` has to take responsibility for the rest.
    let mut builder = Builder::new(Source::new("echo one two three"), SyntaxKind::Script);
    builder.token(SyntaxKind::Text, 4);
    let tree = builder.build();

    assert_eq!(tree.reconstruct(), "echo one two three");
    assert!(
        tree.root().has_errors(),
        "the abandoned tail is marked, not silently dropped"
    );
}

#[test]
fn every_span_lands_on_a_character_boundary() {
    for script in SCRIPTS {
        let tree = shred(script);
        let text = tree.source().text();
        tree.root().visit_tokens(&mut |token| {
            let span = token.span();
            assert!(
                text.is_char_boundary(span.start as usize)
                    && text.is_char_boundary(span.end as usize),
                "{span:?} splits a character in {script:?}"
            );
        });
    }
}
