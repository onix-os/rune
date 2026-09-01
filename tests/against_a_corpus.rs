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
        for token in lex(&text) {
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
