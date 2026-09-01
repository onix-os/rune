use super::lex;
use crate::tree::SyntaxKind;

/// Every token paired with the text it covers, which is what makes a failure readable.
fn tokens(text: &str) -> Vec<(SyntaxKind, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    for token in lex(text) {
        let end = at + token.len as usize;
        out.push((token.kind, text.get(at..end).unwrap_or("<split>")));
        at = end;
    }
    out
}

fn kinds(text: &str) -> Vec<SyntaxKind> {
    lex(text).into_iter().map(|token| token.kind).collect()
}

/// Shell that has given lexers trouble, plus the ordinary cases.
const SCRIPTS: &[&str] = &[
    "",
    "\n",
    "echo hi",
    "echo   hi\t\n",
    "# a comment",
    "echo a#b",
    "echo # a comment",
    "a && b || c | d & e; f",
    "a ;; b ;& c ;;& d",
    "cmd >out 2>&1 <in >>app <>rw >|clob &>both &>>more",
    "cat <<EOF\nbody\nEOF",
    "cat <<-EOF\n\tbody\nEOF",
    "cat <<<'here string'",
    "'single'",
    "'unterminated",
    "\"double\"",
    "\"unterminated",
    "\"a b\tc\nd\"",
    "\"escapes: \\$ \\` \\\" \\\\ and \\d\"",
    "a\\ b",
    "trailing\\",
    "$HOME $? $$ $1 $@ $* $# $! $- $_",
    "$",
    "5$",
    "${x}",
    "${x:-default}",
    "${x:=set} ${x:?msg} ${x:+alt}",
    "${#x} ${!x}",
    "${x#pre} ${x##pre} ${x%suf} ${x%%suf}",
    "${x/a/b} ${x//a/b} ${x^^} ${x,,}",
    "${a[0]} ${a[@]}",
    "${x:-$(echo nested)}",
    "$(echo hi)",
    "$(echo $(echo deep))",
    "`echo old`",
    "$((1 + 2))",
    "$(( (1 + 2) * 3 ))",
    "$(())",
    "~ ~/x ~user/x a~b",
    "echo a\\\nb",
    "\\\n",
    "for i in 1 2 3; do echo \"$i\"; done",
    "if [[ -f x ]]; then echo y; fi",
    "f() { echo hi; }",
    "case $x in a) echo a;; *) echo b;; esac",
    "x=1 y=2 env",
    "arr=(a b c)",
    "echo é ü 日本語",
    "((((((",
    "))))))",
    "$${}{$",
    "\t \r\n  \t\n",
];

#[test]
fn the_tokens_cover_the_whole_input() {
    for script in SCRIPTS {
        let total: usize = lex(script).iter().map(|token| token.len as usize).sum();
        assert_eq!(total, script.len(), "lengths do not add up for {script:?}");
    }
}

#[test]
fn no_token_is_empty() {
    for script in SCRIPTS {
        for token in lex(script) {
            assert_ne!(token.len, 0, "a zero-length {:?} in {script:?}", token.kind);
        }
    }
}

#[test]
fn every_token_lands_on_a_character_boundary() {
    for script in SCRIPTS {
        let mut at = 0;
        for token in lex(script) {
            at += token.len as usize;
            assert!(
                script.is_char_boundary(at),
                "a token boundary splits a character in {script:?}"
            );
        }
    }
}

#[test]
fn words_and_the_space_between_them() {
    assert_eq!(
        tokens("echo hi"),
        [
            (SyntaxKind::Text, "echo"),
            (SyntaxKind::Whitespace, " "),
            (SyntaxKind::Text, "hi"),
        ]
    );
}

#[test]
fn a_comment_needs_to_start_a_word() {
    assert_eq!(kinds("# c"), [SyntaxKind::Comment]);
    assert_eq!(
        kinds("echo # c"),
        [
            SyntaxKind::Text,
            SyntaxKind::Whitespace,
            SyntaxKind::Comment
        ]
    );
    assert_eq!(tokens("echo a#b").last(), Some(&(SyntaxKind::Text, "a#b")));
}

#[test]
fn operators_take_the_longest_match() {
    assert_eq!(kinds("&&"), [SyntaxKind::AndAnd]);
    assert_eq!(kinds(";;&"), [SyntaxKind::SemiSemiAmp]);
    assert_eq!(kinds(";;"), [SyntaxKind::SemiSemi]);
    assert_eq!(kinds("<<-"), [SyntaxKind::LessLessDash]);
    assert_eq!(kinds("<<<"), [SyntaxKind::LessLessLess]);
    assert_eq!(kinds("<<"), [SyntaxKind::LessLess]);
    assert_eq!(kinds("&>>"), [SyntaxKind::AmpGreatGreat]);
}

#[test]
fn quotes_hold_their_contents_together() {
    assert_eq!(tokens("'a b'"), [(SyntaxKind::SingleQuoted, "'a b'")]);
    assert_eq!(
        tokens("\"a b\""),
        [
            (SyntaxKind::DoubleQuote, "\""),
            (SyntaxKind::Text, "a b"),
            (SyntaxKind::DoubleQuote, "\""),
        ]
    );
}

#[test]
fn an_unterminated_quote_runs_to_the_end_rather_than_hanging() {
    assert_eq!(tokens("'abc"), [(SyntaxKind::SingleQuoted, "'abc")]);
    assert_eq!(
        tokens("\"abc"),
        [(SyntaxKind::DoubleQuote, "\""), (SyntaxKind::Text, "abc")]
    );
}

#[test]
fn a_backslash_escapes_only_some_things_inside_double_quotes() {
    assert_eq!(
        tokens("\"\\$ \\d\""),
        [
            (SyntaxKind::DoubleQuote, "\""),
            (SyntaxKind::Escaped, "\\$"),
            // A backslash before `d` is not an escape, so the run simply restarts after it.
            (SyntaxKind::Text, " "),
            (SyntaxKind::Text, "\\d"),
            (SyntaxKind::DoubleQuote, "\""),
        ]
    );
}

#[test]
fn a_line_continuation_does_not_break_the_word_it_is_in() {
    assert_eq!(
        tokens("ab\\\ncd"),
        [
            (SyntaxKind::Text, "ab"),
            (SyntaxKind::LineContinuation, "\\\n"),
            (SyntaxKind::Text, "cd"),
        ]
    );
    // Still one word, so the `#` is not a comment.
    assert_eq!(tokens("ab\\\n#c").last(), Some(&(SyntaxKind::Text, "#c")));
}

#[test]
fn a_parameter_expansion_names_its_parts() {
    assert_eq!(
        tokens("${x:-d}"),
        [
            (SyntaxKind::DollarBrace, "${"),
            (SyntaxKind::Text, "x"),
            (SyntaxKind::ParamOp, ":-"),
            (SyntaxKind::Text, "d"),
            (SyntaxKind::RBrace, "}"),
        ]
    );
}

#[test]
fn an_operator_before_the_name_is_told_from_one_after_it() {
    assert_eq!(
        tokens("${#x}"),
        [
            (SyntaxKind::DollarBrace, "${"),
            (SyntaxKind::ParamOp, "#"),
            (SyntaxKind::Text, "x"),
            (SyntaxKind::RBrace, "}"),
        ]
    );
    assert_eq!(
        tokens("${x#y}"),
        [
            (SyntaxKind::DollarBrace, "${"),
            (SyntaxKind::Text, "x"),
            (SyntaxKind::ParamOp, "#"),
            (SyntaxKind::Text, "y"),
            (SyntaxKind::RBrace, "}"),
        ]
    );
}

#[test]
fn a_subscript_is_its_own_pair() {
    assert_eq!(
        tokens("${a[0]}"),
        [
            (SyntaxKind::DollarBrace, "${"),
            (SyntaxKind::Text, "a"),
            (SyntaxKind::LBracket, "["),
            (SyntaxKind::Text, "0"),
            (SyntaxKind::RBracket, "]"),
            (SyntaxKind::RBrace, "}"),
        ]
    );
}

#[test]
fn a_command_substitution_holds_ordinary_shell() {
    assert_eq!(
        tokens("$(echo hi)"),
        [
            (SyntaxKind::DollarParen, "$("),
            (SyntaxKind::Text, "echo"),
            (SyntaxKind::Whitespace, " "),
            (SyntaxKind::Text, "hi"),
            (SyntaxKind::RParen, ")"),
        ]
    );
}

#[test]
fn arithmetic_finds_its_own_end() {
    assert_eq!(
        tokens("$((1 + 2))"),
        [
            (SyntaxKind::DollarParenParen, "$(("),
            (SyntaxKind::Text, "1 + 2"),
            (SyntaxKind::RParen, ")"),
            (SyntaxKind::RParen, ")"),
        ]
    );
}

#[test]
fn arithmetic_counts_its_nested_parentheses() {
    assert_eq!(
        tokens("$(( (1+2)*3 ))"),
        [
            (SyntaxKind::DollarParenParen, "$(("),
            (SyntaxKind::Text, " (1+2)*3 "),
            (SyntaxKind::RParen, ")"),
            (SyntaxKind::RParen, ")"),
        ]
    );
}

#[test]
fn empty_arithmetic_does_not_swallow_its_closer() {
    assert_eq!(
        kinds("$(())"),
        [
            SyntaxKind::DollarParenParen,
            SyntaxKind::RParen,
            SyntaxKind::RParen,
        ]
    );
}

#[test]
fn a_tilde_only_counts_at_the_start_of_a_word() {
    assert_eq!(
        tokens("~/x"),
        [(SyntaxKind::Tilde, "~"), (SyntaxKind::Text, "/x")]
    );
    assert_eq!(tokens("~user"), [(SyntaxKind::Tilde, "~user")]);
    assert_eq!(tokens("a~b"), [(SyntaxKind::Text, "a~b")]);
}

#[test]
fn a_lone_dollar_is_just_a_character() {
    assert_eq!(
        tokens("5$"),
        [(SyntaxKind::Text, "5"), (SyntaxKind::Dollar, "$")]
    );
}
