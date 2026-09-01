//! Run the lexer over a directory of real scripts.
//!
//! Invented input only proves the lexer survives what its author thought of. This points it at
//! whatever shell is lying around and checks the one thing that must hold everywhere: the tokens
//! account for every byte, exactly once.
//!
//! Ignored by default, because it needs a corpus to point at:
//!
//! ```sh
//! RUNE_CORPUS=/path/to/scripts cargo test --test against_a_corpus -- --ignored --nocapture
//! ```

use rune::{SyntaxKind, lex};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn shell_scripts(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            shell_scripts(&path, found);
        } else if path
            .extension()
            .is_some_and(|ext| ext == "sh" || ext == "bash")
        {
            found.push(path);
        }
    }
}

#[test]
#[ignore = "needs RUNE_CORPUS pointing at a directory of shell scripts"]
fn the_lexer_accounts_for_every_byte_of_real_shell() {
    let root = std::env::var("RUNE_CORPUS").expect("set RUNE_CORPUS to a directory of scripts");
    let mut scripts = Vec::new();
    shell_scripts(Path::new(&root), &mut scripts);
    scripts.sort();
    assert!(!scripts.is_empty(), "no .sh or .bash files under {root}");

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut bytes = 0;
    let mut unknown_in = Vec::new();

    for path in &scripts {
        let Ok(text) = fs::read_to_string(path) else {
            continue; // not UTF-8; the lexer takes &str, so there is nothing to check here
        };
        bytes += text.len();

        let mut at = 0;
        for token in lex(&text).tokens {
            assert_ne!(
                token.len,
                0,
                "a zero-length {:?} in {}",
                token.kind,
                path.display()
            );
            at += token.len as usize;
            assert!(
                text.is_char_boundary(at),
                "a token boundary splits a character in {}",
                path.display()
            );
            *counts.entry(format!("{:?}", token.kind)).or_default() += 1;
            if token.kind == SyntaxKind::Unknown {
                unknown_in.push(path.display().to_string());
            }
        }
        assert_eq!(at, text.len(), "the tokens do not cover {}", path.display());
    }

    println!("\n{} scripts, {bytes} bytes", scripts.len());
    for (kind, count) in &counts {
        println!("{count:>8}  {kind}");
    }
    if !unknown_in.is_empty() {
        unknown_in.dedup();
        println!("\nunknown tokens in {} files:", unknown_in.len());
        for path in unknown_in.iter().take(20) {
            println!("  {path}");
        }
    }
}

#[test]
#[ignore = "needs RUNE_CORPUS pointing at a directory of shell scripts"]
fn the_parser_covers_every_byte_and_says_what_it_could_not_read() {
    let root = std::env::var("RUNE_CORPUS").expect("set RUNE_CORPUS to a directory of scripts");
    let mut scripts = Vec::new();
    shell_scripts(Path::new(&root), &mut scripts);
    scripts.sort();
    assert!(!scripts.is_empty(), "no .sh or .bash files under {root}");

    let mut clean = 0;
    let mut complained = Vec::new();
    let mut messages: BTreeMap<String, usize> = BTreeMap::new();

    for path in &scripts {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let parsed = rune::parse(&text);

        // The invariant, on real shell: whatever the parser made of it, nothing was lost.
        assert_eq!(
            parsed.tree().reconstruct(),
            text,
            "the tree does not reproduce {}",
            path.display()
        );

        // Reconstruction alone is too weak a check. The builder makes up for anything the parser
        // failed to account for by sweeping it into an error node, so a script whose tokens were
        // silently abandoned still reproduces its source perfectly — which is how a here-document
        // body that ended a file went unnoticed while landing outside the tree entirely.
        if parsed.is_clean() {
            assert!(
                !parsed.tree().root().has_errors(),
                "{} parsed with nothing to report but left an error node in the tree",
                path.display()
            );
        }

        if parsed.is_clean() {
            clean += 1;
        } else {
            complained.push((path.display().to_string(), parsed.errors().len()));
            for error in parsed.errors() {
                *messages.entry(error.message.clone()).or_default() += 1;
            }
        }
    }

    println!(
        "\n{clean} of {} scripts parsed with nothing to report",
        scripts.len()
    );
    if !messages.is_empty() {
        println!("\nwhat the rest said:");
        let mut sorted: Vec<_> = messages.iter().collect();
        sorted.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        for (message, count) in sorted.iter().take(15) {
            println!("{count:>8}  {message}");
        }
        println!("\nfiles, worst first:");
        complained.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        for (path, count) in complained.iter().take(15) {
            println!("{count:>8}  {path}");
        }
    }
}

/// The two things a formatter must never get wrong, over real shell.
///
/// **Idempotence** — formatting twice is formatting once — and **meaning** — the node tree and the
/// significant tokens are the same before and after. Invented samples prove a formatter handles
/// what its author thought of; 400 scripts nobody wrote for it prove the rest.
#[test]
#[ignore = "needs RUNE_CORPUS pointing at a directory of shell scripts"]
fn formatting_is_idempotent_and_keeps_the_tree() {
    let root = std::env::var("RUNE_CORPUS").expect("set RUNE_CORPUS to a directory of scripts");
    let mut scripts = Vec::new();
    shell_scripts(Path::new(&root), &mut scripts);
    scripts.sort();
    assert!(!scripts.is_empty(), "no .sh or .bash files under {root}");

    let mut formatted = 0;
    let mut refused = 0;
    let mut unchanged = 0;

    for path in &scripts {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(once) = rune::format(&text) else {
            refused += 1;
            continue;
        };
        formatted += 1;
        if once == text {
            unchanged += 1;
        }

        // What the formatter wrote, the parser has to be able to read. The sharpest failure there
        // is, so it names the file and shows what came out.
        let twice = match rune::format(&once) {
            Ok(twice) => twice,
            Err(errors) => panic!(
                "formatting {} produced something that will not parse: {}\n---\n{once}---",
                path.display(),
                errors
                    .iter()
                    .map(|error| error.message.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        };
        assert_eq!(twice, once, "formatting {} twice moved it", path.display());
        assert_eq!(
            shape(&text),
            shape(&once),
            "formatting changed the tree of {}",
            path.display()
        );
    }

    println!(
        "\n{formatted} of {} scripts formatted ({unchanged} already were), {refused} refused",
        scripts.len()
    );
}

/// The node tree and every token that is not a separator.
///
/// Trivia is what a formatter rewrites, so trivia is what this ignores; `;` and a newline are one
/// separator written two ways, so they go with it. Anything else that differs is a program that
/// changed.
fn shape(text: &str) -> (Vec<String>, Vec<String>) {
    let parsed = rune::parse(text);
    let mut kinds = Vec::new();
    let mut texts = Vec::new();
    walk(
        parsed.tree().root(),
        parsed.tree().source(),
        &mut kinds,
        &mut texts,
    );
    (kinds, texts)
}

fn walk(
    node: &rune::Node,
    source: &rune::Source,
    kinds: &mut Vec<String>,
    texts: &mut Vec<String>,
) {
    kinds.push(format!("{:?}", node.kind()));
    for child in node.children() {
        match child {
            rune::Element::Node(inner) => walk(inner, source, kinds, texts),
            rune::Element::Token(token) => {
                if !matches!(
                    token.kind(),
                    SyntaxKind::Whitespace
                        | SyntaxKind::Comment
                        | SyntaxKind::LineContinuation
                        | SyntaxKind::Newline
                        | SyntaxKind::Semi
                ) {
                    texts.push(token.text(source).to_string());
                }
            }
        }
    }
}
