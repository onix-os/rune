use super::{Command, Script, WordPiece};
use crate::parse::parse;
use crate::tree::SyntaxKind;

#[test]
fn a_script_walks_to_its_commands() {
    let parsed = parse("echo one\nls -l\n");
    let script = Script::of(parsed.tree());
    let source = parsed.tree().source();

    let names: Vec<_> = script
        .items()
        .filter_map(|item| match item.command()? {
            Command::Simple(command) => command.name()?.literal(source),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["echo", "ls"]);
}

#[test]
fn a_word_says_whether_it_expands() {
    let parsed = parse("echo plain \"$var\" 'quoted' a\\ b");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Simple(command)) = item.command() else {
        panic!("expected a simple command");
    };
    let words: Vec<_> = command.arguments().collect();

    assert_eq!(words[0].literal(source).as_deref(), Some("plain"));
    assert!(
        words[1].expands(),
        "\"$var\" is not knowable before it runs"
    );
    assert_eq!(words[1].literal(source), None);
    assert_eq!(words[2].literal(source).as_deref(), Some("quoted"));
    assert_eq!(words[3].literal(source).as_deref(), Some("a b"));
}

#[test]
fn an_assignment_reads_back_its_parts() {
    let parsed = parse("PATH+=/usr/bin cmd");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Simple(command)) = item.command() else {
        panic!("expected a simple command");
    };
    let assignment = command.assignments().next().expect("an assignment");

    assert_eq!(assignment.name(source), "PATH");
    assert!(assignment.is_appending());
    assert_eq!(
        assignment.value().and_then(|word| word.literal(source)),
        Some("/usr/bin".to_string())
    );
}

#[test]
fn an_array_assignment_lists_its_elements() {
    let parsed = parse("arr=(a b c)");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Simple(command)) = item.command() else {
        panic!("expected a simple command");
    };
    let assignment = command.assignments().next().expect("an assignment");
    let elements: Vec<_> = assignment
        .array()
        .expect("an array value")
        .filter_map(|word| word.literal(source))
        .collect();
    assert_eq!(elements, ["a", "b", "c"]);
}

#[test]
fn a_redirection_reads_back_its_descriptor_and_target() {
    let parsed = parse("cmd 2>&1 >out");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Simple(command)) = item.command() else {
        panic!("expected a simple command");
    };
    let redirects: Vec<_> = command.redirects().collect();

    assert_eq!(redirects[0].descriptor(source), Some(2));
    assert_eq!(redirects[0].operator(), Some(SyntaxKind::GreatAmp));
    assert_eq!(redirects[1].descriptor(source), None);
    assert_eq!(
        redirects[1].target().and_then(|word| word.literal(source)),
        Some("out".to_string())
    );
}

#[test]
fn a_conditional_reaches_all_three_branches() {
    let parsed = parse("if a; then b; elif c; then d; else e; fi");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::If(conditional)) = item.command() else {
        panic!("expected an if");
    };

    let first = |list: Option<super::CommandList<'_>>| -> Option<String> {
        let item = list?.items().next()?;
        match item.command()? {
            Command::Simple(command) => command.name()?.literal(source),
            _ => None,
        }
    };
    assert_eq!(first(conditional.condition()).as_deref(), Some("a"));
    assert_eq!(first(conditional.then_branch()).as_deref(), Some("b"));
    assert_eq!(first(conditional.else_branch()).as_deref(), Some("e"));

    let elif = conditional.elif_clauses().next().expect("an elif");
    assert_eq!(first(elif.condition()).as_deref(), Some("c"));
    assert_eq!(first(elif.then_branch()).as_deref(), Some("d"));
}

#[test]
fn a_case_keeps_each_branchs_terminator() {
    let parsed = parse("case $x in a|b) one;; c) two;& *) three;;& esac");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Case(case)) = item.command() else {
        panic!("expected a case");
    };
    let items: Vec<_> = case.items().collect();

    let patterns: Vec<_> = items[0]
        .patterns()
        .filter_map(|word| word.literal(source))
        .collect();
    assert_eq!(patterns, ["a", "b"]);
    assert_eq!(items[0].terminator(), Some(SyntaxKind::SemiSemi));
    assert_eq!(items[1].terminator(), Some(SyntaxKind::SemiAmp));
    assert_eq!(items[2].terminator(), Some(SyntaxKind::SemiSemiAmp));
}

#[test]
fn an_and_or_list_pairs_each_branch_with_its_operator() {
    let parsed = parse("a && b || c");
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let branches = item.and_or().expect("an and-or list").branches();

    assert_eq!(branches.len(), 3);
    assert_eq!(branches[0].0, None);
    assert_eq!(branches[1].0, Some(SyntaxKind::AndAnd));
    assert_eq!(branches[2].0, Some(SyntaxKind::PipePipe));
}

#[test]
fn a_pipeline_reports_its_prefixes() {
    let parsed = parse("! time a | b");
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let pipeline = item.pipeline().expect("a pipeline");

    assert!(pipeline.is_negated());
    assert!(pipeline.is_timed());
    assert_eq!(pipeline.commands().count(), 2);
}

#[test]
fn a_loop_knows_which_way_round_its_test_is() {
    let parsed = parse("until a; do b; done");
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Loop(looping)) = item.command() else {
        panic!("expected a loop");
    };
    assert!(looping.is_until());
    assert!(looping.body().is_some());
}

#[test]
fn a_for_loop_separates_its_variable_from_its_items() {
    let parsed = parse("for i in a b c; do echo; done");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::For(loop_over)) = item.command() else {
        panic!("expected a for");
    };

    assert_eq!(
        loop_over.variable().and_then(|word| word.literal(source)),
        Some("i".to_string())
    );
    let items: Vec<_> = loop_over
        .items()
        .filter_map(|word| word.literal(source))
        .collect();
    assert_eq!(items, ["a", "b", "c"]);
}

#[test]
fn expansions_read_back_their_parts() {
    let parsed = parse("echo ${name:-fallback} $((1 + 2)) $(ls) <(gen)");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Simple(command)) = item.command() else {
        panic!("expected a simple command");
    };
    let pieces: Vec<_> = command
        .arguments()
        .filter_map(|word| word.pieces().next())
        .collect();

    match pieces[0] {
        WordPiece::Expansion(expansion) => {
            assert_eq!(expansion.name(source), "name");
            assert_eq!(expansion.operator(source), Some(":-"));
        }
        other => panic!("expected a parameter expansion, got {other:?}"),
    }
    match pieces[1] {
        WordPiece::Arithmetic(arithmetic) => assert_eq!(arithmetic.expression(source), "1 + 2"),
        other => panic!("expected an arithmetic expansion, got {other:?}"),
    }
    match pieces[2] {
        WordPiece::Command(substitution) => {
            assert!(!substitution.is_backticked());
            assert!(substitution.body().is_some());
        }
        other => panic!("expected a command substitution, got {other:?}"),
    }
    match pieces[3] {
        WordPiece::Process(substitution) => assert!(substitution.reads_from_command()),
        other => panic!("expected a process substitution, got {other:?}"),
    }
}

#[test]
fn a_function_definition_reaches_its_name_and_body() {
    let parsed = parse("greet() { echo hi; }");
    let source = parsed.tree().source();
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::Function(function)) = item.command() else {
        panic!("expected a function definition");
    };

    assert_eq!(
        function.name().and_then(|word| word.literal(source)),
        Some("greet".to_string())
    );
    assert!(matches!(function.body(), Some(Command::Group(_))));
}
