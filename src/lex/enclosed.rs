//! Where a substitution or expansion that opens at the start of some text closes.
//!
//! A shell that keeps a word as text and reads it again later — to expand it, or to run what is
//! inside a `$( … )` — has to find that end exactly where this lexer did, or the two disagree
//! about what the word was. Here-documents, `case` patterns, comments, `$'…'` and regex groups all
//! hide a `)` that does not close anything, and this is the one place that already knows them all.

use super::Lexer;
use crate::tree::SyntaxKind;

/// The construct that opens at the very start of `text`, and how many bytes it covers.
///
/// `text` begins with `$(`, `$((`, `${`, `` ` ``, `<(` or `>(`. The answer is the kind of the
/// opening token and the length up to and including the closer, so `$((1))x` is
/// `(DollarParenParen, 6)` and `$(a) b` is `(DollarParen, 4)`. `None` when `text` opens none of
/// them, or when it is never closed.
///
/// Only as much of `text` is read as the construct takes, so asking about every `$(` in a long
/// here-document body costs the length of the substitutions, not of the body.
pub fn enclosed(text: &str) -> Option<(SyntaxKind, usize)> {
    let mut lexer = Lexer::new(text);
    let first = lexer.step();
    if lexer.modes.len() < 2 {
        return None;
    }
    let mut last = first;
    while lexer.modes.len() > 1 {
        if lexer.cursor.is_eof() {
            return None;
        }
        last = lexer.step();
    }
    // A `$((` that ended on a single `)` was never finished as arithmetic.
    if first == SyntaxKind::DollarParenParen && last != SyntaxKind::RParenRParen {
        return None;
    }
    Some((first, lexer.cursor.offset() as usize))
}

#[cfg(test)]
mod tests {
    use super::enclosed;
    use crate::tree::SyntaxKind;

    fn length(text: &str) -> Option<usize> {
        enclosed(text).map(|(_, len)| len)
    }

    #[test]
    fn each_opener_closes_where_its_closer_is() {
        assert_eq!(enclosed("$(a) b"), Some((SyntaxKind::DollarParen, 4)));
        assert_eq!(enclosed("$((1))x"), Some((SyntaxKind::DollarParenParen, 6)));
        assert_eq!(enclosed("${x:-y}z"), Some((SyntaxKind::DollarBrace, 7)));
        assert_eq!(enclosed("`a` b"), Some((SyntaxKind::Backtick, 3)));
        assert_eq!(enclosed("<(a) b"), Some((SyntaxKind::ProcSubIn, 4)));
        assert_eq!(
            enclosed("$((cd /) ; pwd)"),
            Some((SyntaxKind::DollarParen, 15))
        );
    }

    #[test]
    fn a_closer_inside_something_else_does_not_count() {
        for text in [
            "$(cat <<EOF\nDon't ) \"\nEOF\n)",
            "$(echo $'it\\'s )')",
            "$(# a ) in a comment\necho hi)",
            "$(case a in a) echo;; esac)",
            "$(echo \"a)b\")",
            "${y:-$'it\\'s'}",
            "$([[ a =~ (b| c) ]])",
        ] {
            assert_eq!(length(text), Some(text.len()), "{text:?}");
        }
    }

    #[test]
    fn nothing_opened_or_never_closed_is_none() {
        assert_eq!(enclosed("abc"), None);
        assert_eq!(enclosed("$x"), None);
        assert_eq!(enclosed("$(echo"), None);
        assert_eq!(enclosed("$((1+2)"), None);
        assert_eq!(enclosed("${x"), None);
    }
}
