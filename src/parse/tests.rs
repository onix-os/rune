use super::parse;

/// The tree, indented, with the outer `Script` line dropped so the tests read as what is inside it.
fn tree(text: &str) -> String {
    let parsed = parse(text);
    let dump = parsed.tree().dump();
    dump.lines()
        .skip(1)
        .map(|line| line.strip_prefix("  ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Just the node kinds, in order, for tests about shape rather than text.
fn shape(text: &str) -> Vec<String> {
    parse(text)
        .tree()
        .dump()
        .lines()
        .filter(|line| !line.contains('"'))
        .map(|line| {
            let trimmed = line.trim_start();
            let depth = (line.len() - trimmed.len()) / 2;
            let name = trimmed.split('@').next().unwrap_or("").to_string();
            format!("{}{name}", "  ".repeat(depth))
        })
        .collect()
}

fn errors(text: &str) -> Vec<String> {
    parse(text)
        .errors()
        .iter()
        .map(|error| error.message.clone())
        .collect()
}

#[test]
fn a_simple_command_is_a_run_of_words() {
    assert_eq!(
        tree("echo hi"),
        "CommandList@0..7\n  \
           ListItem@0..7\n    \
             SimpleCommand@0..7\n      \
               Word@0..4\n        \
                 Text@0..4 \"echo\"\n      \
               Whitespace@4..5 \" \"\n      \
               Word@5..7\n        \
                 Text@5..7 \"hi\""
    );
}

#[test]
fn adjacent_pieces_are_one_word() {
    assert_eq!(
        shape("echo pre\"$mid\"post"),
        [
            "Script",
            "  CommandList",
            "    ListItem",
            "      SimpleCommand",
            "        Word",
            "        Word",
        ]
    );
}

#[test]
fn a_reserved_word_is_only_reserved_in_command_position() {
    let conditional = shape("if true; then echo x; fi");
    assert!(
        conditional.iter().any(|line| line.trim() == "IfCommand"),
        "{conditional:?}"
    );
    // As an argument it is an ordinary word, and no `IfCommand` appears.
    assert!(
        !shape("echo if")
            .iter()
            .any(|line| line.contains("IfCommand"))
    );
    // Glued to something else it is not the keyword either.
    assert!(!shape("iffy").iter().any(|line| line.contains("IfCommand")));
}

#[test]
fn pipelines_and_and_or_lists_nest_the_way_they_bind() {
    let nested = shape("a | b && c");
    assert!(
        nested.iter().any(|line| line.trim() == "AndOrList"),
        "{nested:?}"
    );
    assert!(
        nested.iter().any(|line| line.trim() == "Pipeline"),
        "{nested:?}"
    );
}

#[test]
fn a_lone_command_needs_no_wrapper() {
    let lone = shape("a");
    assert!(!lone.iter().any(|line| line.trim() == "Pipeline"));
    assert!(!lone.iter().any(|line| line.trim() == "AndOrList"));
}

#[test]
fn an_assignment_is_split_into_its_parts() {
    assert_eq!(
        tree("x=1"),
        "CommandList@0..3\n  \
           ListItem@0..3\n    \
             SimpleCommand@0..3\n      \
               Assignment@0..3\n        \
                 Text@0..1 \"x\"\n        \
                 Equal@1..2 \"=\"\n        \
                 Word@2..3\n          \
                   Text@2..3 \"1\""
    );
}

#[test]
fn an_assignment_can_be_empty_or_an_array() {
    assert!(shape("x=").iter().any(|line| line.trim() == "Assignment"));
    assert!(
        shape("arr=(a b c)")
            .iter()
            .any(|line| line.trim() == "ArrayValue")
    );
    // A word that is not a name is a command, not an assignment.
    assert!(!shape("2=x").iter().any(|line| line.trim() == "Assignment"));
}

#[test]
fn a_redirection_keeps_its_descriptor() {
    assert_eq!(
        tree("cmd 2>&1"),
        "CommandList@0..8\n  \
           ListItem@0..8\n    \
             SimpleCommand@0..8\n      \
               Word@0..3\n        \
                 Text@0..3 \"cmd\"\n      \
               Whitespace@3..4 \" \"\n      \
               Redirect@4..8\n        \
                 Text@4..5 \"2\"\n        \
                 GreatAmp@5..7 \">&\"\n        \
                 Word@7..8\n          \
                   Text@7..8 \"1\""
    );
}

#[test]
fn a_space_before_a_redirection_makes_the_digits_an_argument() {
    let words = shape("echo 2 >file");
    assert_eq!(words.iter().filter(|line| line.trim() == "Word").count(), 3);
}

#[test]
fn a_command_substitution_holds_a_list_of_its_own() {
    let substitution = shape("echo $(ls | wc)");
    assert!(
        substitution
            .iter()
            .any(|line| line.trim() == "CommandSubstitution"),
        "{substitution:?}"
    );
    assert!(
        substitution.iter().any(|line| line.trim() == "Pipeline"),
        "{substitution:?}"
    );
}

#[test]
fn every_compound_command_is_recognised() {
    for (source, node) in [
        ("if a; then b; fi", "IfCommand"),
        ("while a; do b; done", "WhileCommand"),
        ("until a; do b; done", "UntilCommand"),
        ("for i in 1 2; do b; done", "ForCommand"),
        ("for ((i=0;i<3;i++)); do b; done", "ArithForCommand"),
        ("select x in a b; do c; done", "SelectCommand"),
        ("case $x in a) b;; esac", "CaseCommand"),
        ("(a; b)", "Subshell"),
        ("{ a; b; }", "Group"),
        ("((x + 1))", "ArithCommand"),
        ("[[ -f x ]]", "CondCommand"),
        ("f() { a; }", "FunctionDef"),
        ("function f { a; }", "FunctionDef"),
    ] {
        let found = shape(source);
        assert!(
            found.iter().any(|line| line.trim() == node),
            "{source:?} did not produce a {node}: {found:?}"
        );
        assert!(
            parse(source).is_clean(),
            "{source:?} reported {:?}",
            errors(source)
        );
    }
}

#[test]
fn an_if_names_the_word_it_wanted() {
    assert_eq!(
        errors("if true; then echo x"),
        ["this `if` was never closed"]
    );
    let parsed = parse("if true; then echo x");
    let error = &parsed.errors()[0];
    assert_eq!(error.expected_text(), ["fi"]);
    // The report can point at the `if`, not just at the end of the file.
    assert_eq!(error.opened_at.map(|span| span.start), Some(0));
}

#[test]
fn a_parse_error_does_not_stop_the_rest_of_the_file() {
    let parsed = parse("if a; then b\necho after\nwhile c; do d\n");
    assert_eq!(
        parsed.errors().len(),
        2,
        "{:?}",
        errors("if a; then b\necho after\nwhile c; do d\n")
    );
    assert!(
        parsed.tree().dump().contains("\"after\""),
        "the rest of the file is still parsed"
    );
}

/// Each of these was wrong until a corpus of real scripts said so.
#[test]
fn substitutions_hold_shell_wherever_they_appear() {
    for source in [
        // Quoting stops at `$(` and starts again after `)`, so the closer is not quoted text.
        "echo \"$(ls)\"",
        "echo \"${x:-$(y)}\"",
        "if [ -n \"$(x)\" ]; then :; fi",
        // The backtick that closes is a word piece, like the one that opened.
        "echo `ls`",
        "echo \"`ls`\"",
        "echo `echo a | wc`",
        // An arithmetic expansion inside a command substitution must not look like its end.
        "echo $(echo $((1 + 2)))",
        // Process substitution is a word, not a redirection.
        "diff <(a) <(b)",
        "tee >(cat) < in",
    ] {
        let parsed = parse(source);
        assert!(
            parsed.is_clean(),
            "{source:?} reported {:?}",
            errors(source)
        );
        assert_eq!(parsed.tree().reconstruct(), source);
    }
}

#[test]
fn a_declaration_builtin_reads_its_arguments_as_assignments() {
    let declared = shape("declare -a w=(if then fi)");
    assert!(
        declared.iter().any(|line| line.trim() == "ArrayValue"),
        "{declared:?}"
    );
    assert!(parse("local -a m=(do done)").is_clean());
    // Reserved words inside the parentheses stay ordinary words.
    assert!(!declared.iter().any(|line| line.contains("IfCommand")));
}

#[test]
fn one_mistake_is_one_message() {
    // A stray `done` used to produce an error on every token after it.
    let parsed = parse("echo a\ndone\necho b\necho c\n");
    assert_eq!(parsed.errors().len(), 1, "{:?}", parsed.errors());
    assert!(
        parsed.tree().dump().contains("\"c\""),
        "the rest still parses"
    );
}

/// Nothing asks for trivia that trails the last command, so it has to be swept up deliberately.
///
/// The builder makes up for whatever the parser leaves behind, which meant these still
/// reconstructed their source exactly while the text sat in an error node outside the tree.
#[test]
fn trailing_trivia_reaches_the_tree_rather_than_an_error_node() {
    for source in [
        "echo hi # a trailing comment",
        "echo hi\n# a comment on its own\n",
        "cat <<EOF\nbody\nEOF\n",
        "cat <<EOF\nbody\nEOF",
        "echo hi   ",
        "echo hi\n\n\n",
    ] {
        let parsed = parse(source);
        assert!(
            parsed.is_clean(),
            "{source:?} reported {:?}",
            errors(source)
        );
        assert!(
            !parsed.tree().root().has_errors(),
            "{source:?} left an error node:\n{}",
            parsed.tree().dump()
        );
        assert_eq!(parsed.tree().reconstruct(), source);
    }
}

#[test]
fn a_here_document_body_hangs_off_the_tree_not_the_end_of_it() {
    let parsed = parse("cat <<EOF\nbody\nEOF\necho after\n");
    assert!(parsed.is_clean());
    assert!(!parsed.tree().root().has_errors());
    let dump = parsed.tree().dump();
    assert!(dump.contains("HeredocText"), "{dump}");
    assert!(dump.contains("\"after\""), "{dump}");
}

/// A redirection after a compound command belongs to it, not to the statement after it.
///
/// `{ …; } 2>&1 | sort` reads stderr into the pipe. Left as a statement of its own, the
/// redirection did nothing and the stream went to the terminal instead.
#[test]
fn a_compound_command_takes_its_trailing_redirections() {
    for source in [
        "{ echo a; } 2>&1",
        "( echo a ) > log",
        "if a; then b; fi > log",
        "while a; do b; done < in",
        "for i in a; do b; done >> log",
        "case x in a) b;; esac 2> err",
        "[[ -f x ]] > log",
        "(( 1 + 1 )) > log",
    ] {
        let parsed = parse(source);
        assert!(
            parsed.is_clean(),
            "{source:?} reported {:?}",
            errors(source)
        );
        let dump = parsed.tree().dump();
        // One *top-level* statement, with the redirection inside the construct rather than
        // beside it. Statements nested in the body do not count, so the depth is what is read.
        let statements = dump
            .lines()
            .filter(|line| line.starts_with("    ListItem@"))
            .count();
        assert_eq!(
            statements, 1,
            "{source:?} became more than one statement:\n{dump}"
        );
        assert!(dump.contains("Redirect@"), "{source:?}:\n{dump}");
    }
}

/// An unclosed construct is reported *at the construct*, not at the end of the file.
///
/// The parser finds out at the end, because that is where it runs out of input looking for the
/// closer — but "the script ended" is the one thing the reader can already see. The `if` on line 2
/// is what they have to go and look at.
#[test]
fn an_unclosed_construct_is_reported_where_it_opened() {
    let source = "#!/bin/sh\nif [ -f x ]; then\n  echo found\n\nfor f in *; do\n  echo \"$f\"\n\necho 'unterminated\n\necho done\n";
    let parsed = parse(source);
    let at = |error: &crate::error::Error| parsed.tree().source().line_col(error.span.start);

    let found: Vec<_> = parsed
        .errors()
        .iter()
        .map(|error| (at(error).0, error.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (2, "this `if` was never closed"),
            (5, "this `for` was never closed"),
            (8, "this `'` was never closed"),
        ],
        "each error belongs on the line that opened it"
    );

    // And every one of them is something another line would finish.
    assert!(parsed.errors().iter().all(|error| error.unfinished));
}

/// A mistake that no amount of further input would fix keeps `unfinished` off.
#[test]
fn a_stray_word_is_not_something_another_line_finishes() {
    let parsed = parse("echo one\ndone\n");
    assert_eq!(parsed.errors().len(), 1);
    assert!(!parsed.errors()[0].unfinished);
    // Reported where it is, because that is where the mistake is.
    assert_eq!(
        parsed
            .tree()
            .source()
            .line_col(parsed.errors()[0].span.start)
            .0,
        2
    );
}

#[test]
fn a_prompt_can_tell_unfinished_from_wrong() {
    use crate::error::Completeness::{Complete, Invalid, Unfinished};

    for (source, expected) in [
        ("", Complete),
        ("echo hi", Complete),
        ("if a; then b; fi", Complete),
        ("cat <<EOF\nbody\nEOF\n", Complete),
        // Something is open and the next line will close it.
        ("if a; then b", Unfinished),
        ("while a; do b", Unfinished),
        ("echo \"abc", Unfinished),
        ("echo 'abc", Unfinished),
        ("echo $(ls", Unfinished),
        ("echo ${x", Unfinished),
        ("echo `ls", Unfinished),
        ("cat <<EOF\nbody\n", Unfinished),
        ("a &&", Unfinished),
        ("a |", Unfinished),
        ("(a; b", Unfinished),
        // Nothing will fix these by adding to the end.
        ("done", Invalid),
        ("echo )", Invalid),
        ("fi", Invalid),
    ] {
        assert_eq!(
            parse(source).completeness(),
            expected,
            "{source:?} was read as {:?}, reporting {:?}",
            parse(source).completeness(),
            errors(source)
        );
    }
}

#[test]
fn an_unterminated_quote_is_reported_at_all() {
    // Nothing in the token stream shows it: the word simply runs to the end of the file.
    assert_eq!(errors("echo \"abc"), ["this `\"` was never closed"]);
    assert_eq!(errors("echo 'abc"), ["this `'` was never closed"]);
    let parsed = parse("echo 'abc");
    assert_eq!(parsed.errors()[0].opened_at.map(|span| span.start), Some(5));
}

#[test]
fn one_unclosed_construct_is_reported_once() {
    // The rule and the lexer both notice `$(`; only one of them should speak.
    assert_eq!(errors("echo $(ls"), ["this `$(` was never closed"]);
    assert_eq!(errors("echo ${x"), ["this `${` was never closed"]);
    // An `if` with no `then` used to be reported once for the `then` and again for the `fi`.
    // Recovery also used to swallow the `)`, so the substitution around it looked unclosed too and
    // one broken word became three messages about the whole file.
    assert_eq!(errors("echo $(if)"), ["this `if` was never closed"]);
    assert_eq!(errors("while a").len(), 1);
    assert_eq!(errors("for i in a").len(), 1);
    assert_eq!(errors("case $x").len(), 1);
}

#[test]
fn an_unclosed_expansion_is_reported() {
    assert_eq!(errors("echo $(ls"), ["this `$(` was never closed"]);
    assert_eq!(errors("echo ${x"), ["this `${` was never closed"]);
}
