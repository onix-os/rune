use super::*;
use crate::tree::Token;

fn fmt(text: &str) -> String {
    format(text).expect("the sample parses")
}

/// The two invariants, on one sample. Every test below leans on this.
fn holds(before: &str, after: &str) {
    let once = fmt(before);
    assert_eq!(once, after);
    assert_eq!(fmt(&once), once, "formatting is not idempotent");
    let was = crate::parse(before);
    let now = crate::parse(&once);
    assert_eq!(
        shape(was.tree().root(), was.tree().source()),
        shape(now.tree().root(), now.tree().source()),
        "the tree changed shape"
    );
}

/// Everything that must survive formatting: the node tree, and every token that is not a
/// separator.
///
/// Trivia is what the formatter rewrites, so trivia is what this ignores. A `;` and a newline sit
/// in the same position — one layout writes that separator as one and one as the other — so both
/// are left out, and everything else has to come out the same, in the same order, under the same
/// nodes.
fn shape(node: &Node, source: &Source) -> (Vec<K>, Vec<String>) {
    let mut kinds = Vec::new();
    let mut texts = Vec::new();
    walk(node, source, &mut kinds, &mut texts);
    (kinds, texts)
}

fn walk(node: &Node, source: &Source, kinds: &mut Vec<K>, texts: &mut Vec<String>) {
    kinds.push(node.kind());
    for child in node.children() {
        match child {
            Element::Node(inner) => walk(inner, source, kinds, texts),
            Element::Token(token) if !is_separator(token) => {
                texts.push(token.text(source).to_string());
            }
            Element::Token(_) => {}
        }
    }
}

fn is_separator(token: &Token) -> bool {
    matches!(
        token.kind(),
        K::Whitespace | K::Comment | K::LineContinuation | K::Newline | K::Semi
    )
}

#[test]
fn an_empty_script_formats_to_nothing() {
    assert_eq!(fmt(""), "");
    assert_eq!(fmt("\n\n\n"), "");
    assert_eq!(fmt("   "), "");
}

#[test]
fn a_command_loses_the_spaces_it_did_not_need() {
    holds("echo    a     b", "echo a b\n");
    holds("   echo a  ", "echo a\n");
    holds("echo a;", "echo a\n");
}

/// A body is indented; a condition is not a body.
#[test]
fn an_if_puts_then_on_the_header_line() {
    holds("if a\nthen\nb\nfi", "if a; then\n    b\nfi\n");
    holds(
        "if a; then b; else c; fi",
        "if a; then\n    b\nelse\n    c\nfi\n",
    );
    holds(
        "if a; then b; elif c; then d; fi",
        "if a; then\n    b\nelif c; then\n    d\nfi\n",
    );
}

#[test]
fn a_loop_puts_do_on_the_header_line() {
    holds("while a\ndo\nb\ndone", "while a; do\n    b\ndone\n");
    holds("until a; do b; done", "until a; do\n    b\ndone\n");
    holds(
        "for i in 1 2; do echo $i; done",
        "for i in 1 2; do\n    echo $i\ndone\n",
    );
}

/// One line or several, as the author left it — see `case_item`.
#[test]
fn a_case_arm_keeps_the_layout_it_was_written_with() {
    holds(
        "case $x in\na|b) one;;\n*) two;;\nesac",
        "case $x in\n    a|b) one ;;\n    *) two ;;\nesac\n",
    );
    holds(
        "case $x in\na)\none\ntwo\n;;\nesac",
        "case $x in\n    a)\n        one\n        two\n        ;;\nesac\n",
    );
}

/// The last arm may leave its `;;` off, and `esac` still has to land in column zero.
#[test]
fn a_case_arm_without_its_terminator_still_closes() {
    holds(
        "case $x in\na)\none\nesac",
        "case $x in\n    a)\n        one\nesac\n",
    );
}

#[test]
fn a_function_body_is_a_block_unless_it_was_a_line() {
    holds("f()   {\nx=1\n}", "f() {\n    x=1\n}\n");
    holds("f() { a; }", "f() { a; }\n");
    holds("f ()\n{\na\n}", "f() {\n    a\n}\n");
}

/// `{ a }` is not a group. The terminator the inline layout drops has to come back.
#[test]
fn an_inline_group_keeps_something_before_its_brace() {
    holds("{ a; b; }", "{ a; b; }\n");
    holds("{\na\nb\n}", "{\n    a\n    b\n}\n");
    holds("(a && b)", "( a && b )\n");
}

/// **`((` is not two subshells.** It opens an arithmetic command, so welding nested brackets
/// together does not tidy a program — it changes which one it is.
#[test]
fn nested_subshells_keep_their_brackets_apart() {
    holds("( ( ( echo x ) ) )", "( ( ( echo x ) ) )\n");
    holds("(  (  echo x  )  )", "( ( echo x ) )\n");
    // And the adjacent spelling is left alone, because it is already the other thing.
    holds("((1 + 1))", "((1 + 1))\n");
}

/// A bracket that was on one line still opens out when what is inside it cannot stay on one.
#[test]
fn a_bracket_gives_way_to_the_compound_inside_it() {
    holds(
        "(while a; do b; done) | c",
        "(\n    while a; do\n        b\n    done\n) | c\n",
    );
}

/// The space is where the meaning goes: `2 >&1` is a different command from `2>&1`.
#[test]
fn a_redirection_is_written_as_one_piece() {
    holds("echo a > out.txt", "echo a >out.txt\n");
    holds("echo a 2>&1", "echo a 2>&1\n");
    holds("echo a  >>  log", "echo a >>log\n");
    holds("read x < in", "read x <in\n");
    // The one place a space goes back in, or `>>(` becomes an append.
    holds("echo a > >(tee log)", "echo a > >(tee log)\n");
}

/// The bytes between the delimiters are data, and they start in column zero.
#[test]
fn a_here_document_body_is_left_exactly_as_it_was() {
    holds(
        "if a; then\ncat <<E\n  body\nE\nfi",
        "if a; then\n    cat <<E\n  body\nE\nfi\n",
    );
    holds("cat <<E\nx\nE\necho after", "cat <<E\nx\nE\necho after\n");
}

/// A break somebody wrote is a break that stays; the indentation under it is not theirs to keep.
#[test]
fn a_split_line_stays_split_and_gets_lined_up() {
    holds("a && b ||\nc", "a && b ||\n    c\n");
    holds("a &&\nb", "a &&\n    b\n");
    holds("a | b", "a | b\n");
    holds("long \\\n  --flag one", "long \\\n    --flag one\n");
}

/// Where a comment sits says what it is about.
#[test]
fn a_comment_stays_on_the_line_it_was_written_on() {
    holds("echo a # why", "echo a # why\n");
    holds("# why\necho a", "# why\necho a\n");
    holds("echo a  #  why  ", "echo a #  why\n");
}

/// One blank line, however many were there — and none at either end.
#[test]
fn blank_lines_are_kept_but_only_one() {
    holds("a\n\n\n\nb", "a\n\nb\n");
    holds("\n\na\nb\n\n\n", "a\nb\n");
}

/// A background command is not a command with a redundant `;` after it.
#[test]
fn an_ampersand_is_not_a_separator_to_drop() {
    holds("a &", "a &\n");
    holds("a & b", "a &\nb\n");
}

/// Never touched, because nothing here understands what is inside them.
#[test]
fn quoting_and_tests_and_expansions_come_out_byte_for_byte() {
    holds("echo '  a  b  '", "echo '  a  b  '\n");
    holds("echo \"a   $b\"", "echo \"a   $b\"\n");
    holds("[[ -f  x  ]]", "[[ -f  x  ]]\n");
    holds("x=$(  a  )", "x=$(  a  )\n");
}

/// A script with a mistake in it has no tree worth reformatting.
#[test]
fn a_script_that_will_not_parse_is_refused() {
    assert!(format("if a; then").is_err());
    assert!(format("case $x in").is_err());
}

/// The layout is a setting, not a fact about shell.
#[test]
fn the_indentation_can_be_said_out_loud() {
    let options = Options {
        indent: "\t".to_string(),
    };
    assert_eq!(
        format_with("if a; then b; fi", &options).unwrap(),
        "if a; then\n\tb\nfi\n"
    );
}
