//! The subscript of an assignment, which runs to its matching `]` whatever is inside.
//!
//! Where an assignment can stand, bash reads `name[` as the start of a subscript and takes
//! everything up to the matching `]` into the word: `a[$i + 1]=x` and `a[ $i ]=x` are each one
//! word, one assignment. Anywhere else the same characters are ordinary: `declare a[ $i ]=x` is
//! three arguments, which is what bash makes of it too.
//!
//! A subscript the plain run already closes stays inside that run — `a[1]=x` is still one token.
//! Only one that is still open when the run ends gets the mode, and then its brackets come out
//! as [`SyntaxKind::LBracket`] and [`SyntaxKind::RBracket`] so the parser can find its end.

use super::{Lexer, Mode};
use crate::tree::SyntaxKind;

/// How long the name is, if `rest` starts with a name whose `[` the plain run would leave open.
fn open_subscript(rest: &str, plain: impl Fn(char) -> bool) -> Option<usize> {
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    let name = chars
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_'))
        .filter(|(_, ch)| *ch == '[')
        .map(|(at, _)| at)?;
    let mut depth = 0u32;
    for ch in rest.get(name..)?.chars() {
        if !plain(ch) {
            break;
        }
        match ch {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return None;
        }
    }
    Some(name)
}

impl Lexer<'_> {
    /// Read the name before an assignment's open subscript, if one starts here.
    ///
    /// Asked only at the start of a word, where an assignment can stand.
    pub(super) fn subscript_name(&mut self, plain: impl Fn(char) -> bool) -> bool {
        if !self.assign_ok {
            return false;
        }
        let Some(len) = open_subscript(self.cursor.rest(), plain) else {
            return false;
        };
        self.cursor.advance(len);
        self.subscript_next = true;
        true
    }

    /// The `[` after a name [`Self::subscript_name`] read, which opens the subscript.
    pub(super) fn open_subscript_bracket(&mut self) -> bool {
        if !std::mem::take(&mut self.subscript_next) || !self.cursor.eat_char('[') {
            return false;
        }
        self.push_mode(Mode::Subscript { depth: 0 });
        true
    }

    /// A piece of a subscript: a bracket, a quote or expansion, or a run of anything else.
    pub(super) fn subscript_piece(&mut self, depth: i32) -> SyntaxKind {
        match self.cursor.peek() {
            Some('[') => {
                self.cursor.bump();
                self.set_depth(depth + 1);
                SyntaxKind::LBracket
            }
            Some(']') => {
                self.cursor.bump();
                if depth == 0 {
                    self.pop_mode();
                } else {
                    self.set_depth(depth - 1);
                }
                SyntaxKind::RBracket
            }
            Some('\'' | '"' | '\\' | '`' | '$') => self.word_piece(),
            _ => {
                self.cursor
                    .eat_while(|ch| !matches!(ch, '[' | ']' | '\'' | '"' | '\\' | '`' | '$'));
                SyntaxKind::Text
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::open_subscript;

    fn plain(ch: char) -> bool {
        !matches!(ch, '$' | ' ' | '"' | '\'' | '(' | ')' | ';' | '\\' | '`')
    }

    #[test]
    fn only_a_subscript_the_run_leaves_open_is_one() {
        assert_eq!(open_subscript("a[$i]=x", plain), Some(1));
        assert_eq!(open_subscript("arr[ $i ]=x", plain), Some(3));
        assert_eq!(open_subscript("a[b[$i]]=x", plain), Some(1));
        assert_eq!(open_subscript("a[1]=x", plain), None);
        assert_eq!(open_subscript("a[i+1]=x", plain), None);
        assert_eq!(open_subscript("1a[$i]=x", plain), None);
        assert_eq!(open_subscript("a.b[$i]", plain), None);
        assert_eq!(open_subscript("abc", plain), None);
    }
}
