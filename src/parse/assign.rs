//! Assignments, and the function definitions that look a little like them.
//!
//! The lexer has no reason to split `x=1` — it is one run of ordinary characters — so the grammar
//! does it here, cutting the token into a name, an operator and a value.

use super::Parser;
use crate::tree::SyntaxKind;

/// Where a word stops being a name and starts being an assignment operator.
///
/// Returns the length of the name, the length of the operator, and which operator it is. Anything
/// that is not a name followed by `=` or `+=` is an ordinary word: `2=x` is a command called `2=x`.
fn split_assignment(text: &str) -> Option<(u32, u32, SyntaxKind)> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    // `a[i]=x` assigns to one element, and the subscript can hold anything arithmetic — so what is
    // between the brackets is skipped rather than checked. Without this the `[` ended the name and
    // `b[3]=z` was read as the name of a command to run.
    let mut subscript = 0u32;
    for (at, ch) in chars {
        match ch {
            '[' => subscript += 1,
            ']' if subscript > 0 => subscript -= 1,
            _ if subscript > 0 => {}
            '=' => return Some((at as u32, 1, SyntaxKind::Equal)),
            '+' if text.get(at + 1..).is_some_and(|rest| rest.starts_with('=')) => {
                return Some((at as u32, 2, SyntaxKind::PlusEqual));
            }
            _ if ch.is_alphanumeric() || ch == '_' => {}
            _ => return None,
        }
    }
    None
}

impl Parser<'_> {
    pub(super) fn at_assignment(&self) -> bool {
        let Some(index) = self.significant(0) else {
            return false;
        };
        self.tokens[index].kind == SyntaxKind::Text
            && split_assignment(self.token_text(index)).is_some()
    }

    pub(super) fn assignment(&mut self) {
        let Some(index) = self.significant(0) else {
            return;
        };
        let Some((name_len, op_len, op_kind)) = split_assignment(self.token_text(index)) else {
            return;
        };
        let rest = self.tokens[index].len.saturating_sub(name_len + op_len);

        self.start(SyntaxKind::Assignment);
        self.bump_slice(SyntaxKind::Text, name_len);
        self.bump_slice(op_kind, op_len);
        if rest > 0 {
            // The value began inside the same token, so the word has to be opened around it.
            self.open(SyntaxKind::Word);
            self.bump_slice(SyntaxKind::Text, rest);
            self.end_token();
            self.word_rest();
            self.finish_node();
        } else {
            self.end_token();
            if self.raw() == Some(SyntaxKind::LParen) {
                self.array_value();
            } else if self.raw().is_some_and(SyntaxKind::is_word_piece) {
                self.word();
            } else {
                // `x=` assigns the empty string, and the empty word is where that is recorded.
                self.open(SyntaxKind::Word);
                self.finish_node();
            }
        }
        self.finish_node();
    }

    /// `(a b c)` on the right of an assignment.
    fn array_value(&mut self) {
        self.start(SyntaxKind::ArrayValue);
        self.bump();
        loop {
            self.skip_newlines();
            if self.at_end() || self.at(SyntaxKind::RParen) {
                break;
            }
            let before = self.progress();
            if self.at_word() {
                self.word();
            } else {
                self.bump();
            }
            if self.progress() == before {
                break;
            }
        }
        if !self.eat(SyntaxKind::RParen) {
            self.error("this array value was never closed");
        }
        self.finish_node();
    }

    pub(super) fn at_function_definition(&self) -> bool {
        if self.at_keyword(SyntaxKind::Function) {
            return true;
        }
        let Some(index) = self.significant(0) else {
            return false;
        };
        if self.tokens[index].kind != SyntaxKind::Text {
            return false;
        }
        let name = self.token_text(index);
        // `x=()` is an empty array, not a function called `x=`.
        if name.is_empty() || name.contains('=') {
            return false;
        }
        self.peek_nth(1) == Some(SyntaxKind::LParen) && self.peek_nth(2) == Some(SyntaxKind::RParen)
    }

    pub(super) fn function_definition(&mut self) {
        self.start(SyntaxKind::FunctionDef);
        if self.eat_keyword(SyntaxKind::Function) {
            if self.at_word() {
                self.word();
            } else {
                self.error("`function` needs a name");
            }
            if self.eat(SyntaxKind::LParen) {
                self.eat(SyntaxKind::RParen);
            }
        } else {
            self.word();
            self.eat(SyntaxKind::LParen);
            if !self.eat(SyntaxKind::RParen) {
                self.error("a function definition is written `name() { ... }`");
            }
        }
        self.skip_newlines();
        self.command();
        self.finish_node();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_and_an_operator_are_told_from_an_ordinary_word() {
        assert_eq!(split_assignment("x=1"), Some((1, 1, SyntaxKind::Equal)));
        assert_eq!(split_assignment("PATH="), Some((4, 1, SyntaxKind::Equal)));
        assert_eq!(
            split_assignment("x+=1"),
            Some((1, 2, SyntaxKind::PlusEqual))
        );
        assert_eq!(split_assignment("_a1=y"), Some((3, 1, SyntaxKind::Equal)));
    }

    #[test]
    fn an_element_assignment_keeps_its_subscript_in_the_name() {
        assert_eq!(split_assignment("b[3]=z"), Some((4, 1, SyntaxKind::Equal)));
        assert_eq!(
            split_assignment("b[i+1]=z"),
            Some((6, 1, SyntaxKind::Equal))
        );
        assert_eq!(
            split_assignment("b[@]+=z"),
            Some((4, 2, SyntaxKind::PlusEqual))
        );
        // A subscript is not a licence to be anything: the name still has to be one.
        assert_eq!(split_assignment("1[0]=z"), None);
    }

    #[test]
    fn anything_that_is_not_a_name_is_a_word() {
        assert_eq!(split_assignment("2=x"), None);
        assert_eq!(split_assignment("echo"), None);
        assert_eq!(split_assignment("a-b=c"), None);
        assert_eq!(split_assignment(""), None);
        assert_eq!(split_assignment("=x"), None);
    }
}
