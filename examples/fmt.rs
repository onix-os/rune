//! Format a script.
//!
//! ```sh
//! cargo run --example fmt -- script.sh
//! ```

use std::io::Read;

fn main() {
    let mut text = String::new();
    match std::env::args().nth(1) {
        Some(path) => text = std::fs::read_to_string(&path).expect("readable"),
        None => {
            std::io::stdin().read_to_string(&mut text).expect("stdin");
        }
    }
    match rune::format(&text) {
        Ok(formatted) => print!("{formatted}"),
        Err(errors) => {
            for error in errors {
                eprintln!("{}", error.message);
            }
            std::process::exit(1);
        }
    }
}
