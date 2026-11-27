//! Where in a command the lexer is: at its start, at a `case` pattern, or at an assignment.
//!
//! None of this is grammar. It is the little the lexer has to know about position to read the
//! characters right — a `!(` that is a subshell or a pattern, a `)` that closes a substitution or a
//! `case` pattern, a `[` that opens a subscript or is just a character.

use super::Lexer;
use crate::tree::SyntaxKind;

/// Whether a word starting with `text` could be an assignment: a name, then `=`, `+=` or `[`.
fn starts_assignment(text: &str) -> bool {
    let mut chars = text.char_indices();
    if !chars
        .next()
        .is_some_and(|(_, ch)| ch.is_alphabetic() || ch == '_')
    {
        return false;
    }
    for (at, ch) in chars {
        match ch {
            '=' | '[' => return true,
            '+' => return text.get(at + 1..).is_some_and(|rest| rest.starts_with('=')),
            _ if ch.is_alphanumeric() || ch == '_' => {}
            _ => return false,
        }
    }
    false
}

impl Lexer<'_> {
    /// Whether a token of this kind and text leaves a place where a command can begin after it.
    ///
    /// Only ever an approximation, and only used to decide whether a `case` is the keyword — see
    /// [`Self::note_case_word`] — and how to read `!(`, `[[` and an assignment's `[`.
    ///
    /// **The reserved words are checked by text, because to the lexer they are not reserved.** `{`,
    /// `do` and `then` all arrive as `Text` — the tree shows `LBrace` because the *parser* decides
    /// that, long after this runs — so a rule written on kinds alone saw `f() { case …` as a `case`
    /// in the middle of a command and did not count it.
    fn ends_a_command(kind: SyntaxKind, text: &str) -> bool {
        matches!(
            kind,
            SyntaxKind::Newline
                | SyntaxKind::Semi
                | SyntaxKind::SemiSemi
                | SyntaxKind::SemiAmp
                | SyntaxKind::SemiSemiAmp
                | SyntaxKind::Amp
                | SyntaxKind::AndAnd
                | SyntaxKind::PipePipe
                | SyntaxKind::Pipe
                | SyntaxKind::PipeAmp
                | SyntaxKind::LParen
                // A `case` *pattern* ends with one, and the item's body begins after it — which is
                // how a `case` nested directly inside another one is counted.
                | SyntaxKind::RParen
                | SyntaxKind::DollarParen
                | SyntaxKind::Backtick
        ) || matches!(
            text,
            "{" | "do"
                | "then"
                | "else"
                | "elif"
                | "!"
                | "if"
                | "while"
                | "until"
                | "time"
                | "coproc"
        )
    }

    /// Keep [`Self::command_can_start`] and [`Self::pattern_next`] true to the token just read.
    ///
    /// `word_start` is whether the token began a word, read before it was emitted.
    pub(super) fn note_position(&mut self, kind: SyntaxKind, text: &str, word_start: bool) {
        match kind {
            _ if kind.is_trivia() => {}
            SyntaxKind::Newline => self.command_can_start = true,
            // A closing backtick ends a substitution inside a word, and the word goes on.
            SyntaxKind::Backtick if self.mode() != super::Mode::Backtick => {
                self.command_can_start = false;
            }
            SyntaxKind::SemiSemi | SyntaxKind::SemiAmp | SyntaxKind::SemiSemiAmp => {
                self.command_can_start = true;
                self.pattern_next = true;
            }
            // `case x in (a) …` — the optional opening parenthesis still leads to a pattern.
            SyntaxKind::LParen if self.pattern_next => self.command_can_start = true,
            _ => {
                self.command_can_start = Self::ends_a_command(kind, text);
                self.pattern_next = kind == SyntaxKind::Text && text == "in";
            }
        }
        self.note_assignment(kind, text, word_start);
    }

    /// Keep [`Self::assign_ok`] true: a word may assign at a command start and after another one.
    ///
    /// bash reads `a[ $i ]=x` as one word only where an assignment can stand, so `x=1 a[ $i ]=2`
    /// assigns twice and `declare a[ $i ]=x` is three arguments.
    fn note_assignment(&mut self, kind: SyntaxKind, text: &str, word_start: bool) {
        if kind.is_word_piece() {
            if word_start {
                self.word_assigns =
                    self.assign_ok && (self.subscript_next || starts_assignment(text));
            }
            return;
        }
        if kind == SyntaxKind::LineContinuation {
            return;
        }
        let after_assignment = kind == SyntaxKind::Whitespace && self.word_assigns;
        self.assign_ok = (self.command_can_start && !self.pattern_next) || after_assignment;
        self.word_assigns = false;
    }

    /// Count a `case` that opened or closed inside a command substitution.
    ///
    /// **Position matters for `case` and not for `esac`.** `"$(echo case)"` must still close where
    /// it closes, so the opener counts only where a command can begin; `esac` is asked for only
    /// when a `case` is already open, so a word of that spelling anywhere else cannot take one
    /// away. Neither is a reserved word to the lexer — the parser decides that — so this is a
    /// count kept for the sole purpose of knowing whether a `)` closes a pattern or a
    /// substitution.
    pub(super) fn note_case_word(&mut self, text: &str, at_command_start: bool) {
        match text {
            "case" if at_command_start => self.bump_sub_cases(1),
            "esac" => self.bump_sub_cases(-1),
            _ => {}
        }
    }

    fn bump_sub_cases(&mut self, by: i32) {
        if let Some((super::Mode::CommandSub { cases, .. }, _)) = self.modes.last_mut() {
            *cases = cases.saturating_add(by).max(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::starts_assignment;

    #[test]
    fn an_assignment_is_told_from_its_first_piece() {
        for text in ["a=", "a+=x", "a[", "_b1[2]=x", "PATH=/bin"] {
            assert!(starts_assignment(text), "{text}");
        }
        for text in ["a", "1a=", "a-b=", "=x", "", "a+b=c"] {
            assert!(!starts_assignment(text), "{text}");
        }
    }
}
