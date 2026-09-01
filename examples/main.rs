//! Parse a script and show what came of it.
//!
//! ```sh
//! cargo run --example main -- script.sh
//! echo 'a | b' | cargo run --example main
//! ```

use std::io::Read;

fn main() {
    let mut text = String::new();
    match std::env::args().nth(1) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(contents) => text = contents,
            Err(error) => {
                eprintln!("{path}: {error}");
                std::process::exit(1);
            }
        },
        None => {
            if std::io::stdin().read_to_string(&mut text).is_err() {
                eprintln!("could not read stdin");
                std::process::exit(1);
            }
        }
    }

    let parsed = rune::parse(&text);
    print!("{}", parsed.tree().dump());

    if parsed.tree().reconstruct() != text {
        eprintln!("\nBUG: the tree does not reproduce its source");
        std::process::exit(2);
    }

    if parsed.is_clean() {
        println!("\nno errors");
        return;
    }
    println!("\n{} error(s):", parsed.errors().len());
    let source = parsed.tree().source();
    for error in parsed.errors() {
        let (line, column) = source.line_col(error.span.start);
        println!("  {line}:{column}: {}", error.message);
        if let Some(opened) = error.opened_at {
            let (line, column) = source.line_col(opened.start);
            println!("      opened at {line}:{column}");
        }
    }
}
