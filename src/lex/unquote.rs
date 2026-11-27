//! Quote removal for a here-document delimiter.
//!
//! The delimiter is compared with the terminator line after quote removal, and quote removal is
//! the shell's, not a filter over three characters: `<<'E"OF'` ends at a line saying `E"OF`,
//! `<<"E\"F"` at `E"F`, and `<<$'E\tF'` at `E`, a tab and `F`.

use std::iter::Peekable;
use std::str::Chars;

/// The delimiter `raw` spells once its quoting is removed, and whether any of it was quoted.
///
/// Whether it was quoted is what decides that the body is literal; `<<E"O"F` and `<<\EOF` count.
pub(super) fn unquote(raw: &str) -> (String, bool) {
    let mut out = String::with_capacity(raw.len());
    let mut quoted = false;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                quoted = true;
                out.extend(chars.next());
            }
            '\'' => {
                quoted = true;
                out.extend(chars.by_ref().take_while(|inner| *inner != '\''));
            }
            '"' => {
                quoted = true;
                double_quoted(&mut chars, &mut out);
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                quoted = true;
                ansi_c(&mut chars, &mut out);
            }
            '$' if chars.peek() == Some(&'"') => {
                chars.next();
                quoted = true;
                double_quoted(&mut chars, &mut out);
            }
            _ => out.push(ch),
        }
    }
    (out, quoted)
}

/// The inside of `"…"`, where a backslash escapes only `$`, `` ` ``, `"`, `\` and a newline.
fn double_quoted(chars: &mut Peekable<Chars<'_>>, out: &mut String) {
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return,
            '\\' => match chars.peek() {
                Some('\n') => {
                    chars.next();
                }
                Some('$' | '`' | '"' | '\\') => out.extend(chars.next()),
                _ => out.push('\\'),
            },
            _ => out.push(ch),
        }
    }
}

/// The inside of `$'…'`, decoding the escapes a delimiter could plausibly spell.
fn ansi_c(chars: &mut Peekable<Chars<'_>>, out: &mut String) {
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            return;
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            out.push('\\');
            return;
        };
        let decoded = match escaped {
            'a' => '\x07',
            'b' => '\x08',
            'e' | 'E' => '\x1b',
            'f' => '\x0c',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\x0b',
            '\\' | '\'' | '"' | '?' => escaped,
            'x' => number(chars, 16, 2, 0).unwrap_or('x'),
            '0'..='7' => number(chars, 8, 2, escaped.to_digit(8).unwrap_or(0)).unwrap_or(escaped),
            other => {
                out.push('\\');
                other
            }
        };
        out.push(decoded);
    }
}

/// Up to `most` more digits in `radix` after `value`, as the character they number.
///
/// `None` for a hexadecimal escape with no digit after it, which stays the letter it was.
fn number(chars: &mut Peekable<Chars<'_>>, radix: u32, most: usize, value: u32) -> Option<char> {
    let mut value = value;
    let mut read = 0;
    while read < most
        && let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(radix))
    {
        value = value * radix + digit;
        chars.next();
        read += 1;
    }
    (read > 0 || radix == 8)
        .then(|| char::from_u32(value))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::unquote;

    #[test]
    fn quoting_comes_off_the_delimiter() {
        assert_eq!(unquote("EOF"), ("EOF".to_string(), false));
        assert_eq!(unquote("'EOF'"), ("EOF".to_string(), true));
        assert_eq!(unquote("\"EOF\""), ("EOF".to_string(), true));
        assert_eq!(unquote("\\EOF"), ("EOF".to_string(), true));
        assert_eq!(unquote("\"E\"OF"), ("EOF".to_string(), true));
    }

    #[test]
    fn only_the_quoting_comes_off() {
        assert_eq!(unquote("'E\"OF'"), ("E\"OF".to_string(), true));
        assert_eq!(unquote("\"E\\\"F\""), ("E\"F".to_string(), true));
        assert_eq!(unquote("'A'B"), ("AB".to_string(), true));
        assert_eq!(unquote("\"$x\""), ("$x".to_string(), true));
    }

    #[test]
    fn ansi_c_and_locale_quoting_are_quoting_too() {
        assert_eq!(unquote("$'EOF'"), ("EOF".to_string(), true));
        assert_eq!(unquote("$'E\\tF'"), ("E\tF".to_string(), true));
        assert_eq!(unquote("$'\\x41\\101'"), ("AA".to_string(), true));
        assert_eq!(unquote("$\"EOF\""), ("EOF".to_string(), true));
    }
}
