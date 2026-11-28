//! Scripts bash accepts, each read the way bash reads it.
//!
//! Every case here came out of real scripts that parsed in bash and not in rune. Each one is held
//! to three things: it parses with nothing to report, the tree reproduces it byte for byte, and
//! the structure is the one bash builds.

use super::parse;
use crate::ast::{Command, Script};
use crate::tree::{Node, SyntaxKind};

/// Parse `text`, insisting it is clean and lossless.
fn clean(text: &str) -> crate::parse::Parsed {
    let parsed = parse(text);
    assert!(parsed.is_clean(), "{text:?} reported {:?}", parsed.errors());
    assert_eq!(parsed.tree().reconstruct(), text, "{text:?} lost text");
    parsed
}

/// Every node of `kind` beneath `node`.
fn find<'a>(node: &'a Node, kind: SyntaxKind, out: &mut Vec<&'a Node>) {
    for child in node.nodes() {
        if child.kind() == kind {
            out.push(child);
        }
        find(child, kind, out);
    }
}

/// The text of every node of `kind` in `text`'s tree.
fn texts(text: &str, kind: SyntaxKind) -> Vec<String> {
    let parsed = clean(text);
    let mut nodes = Vec::new();
    find(parsed.tree().root(), kind, &mut nodes);
    nodes
        .iter()
        .map(|node| parsed.tree().source().slice(node.span()).to_string())
        .collect()
}

#[test]
fn a_test_can_span_lines_where_bash_skips_newlines() {
    for text in [
        "[[ a = a\n && -n x ]]",
        "[[\n a = a ]]",
        "[[ -n x\n]]",
        "[[ a = b ||\n a = a ]]",
        "[[ a = a\n || b = c\n ]]",
        "[[ ( a = a\n ) ]]",
        "[[ !\n a = b ]]",
        "[[ $x =~ a\n]]",
    ] {
        clean(text);
    }
    // After a bare word bash wants an operator, and a newline is not one.
    assert!(!parse("[[ a\n]]").is_clean());
    assert!(!parse("[[ x\n== x ]]").is_clean());
}

#[test]
fn a_regex_group_keeps_its_spaces() {
    for (text, operand) in [
        ("[[ \"a=b c\" =~ a=([^ ]*) ]]", "a=([^ ]*)"),
        ("[[ $s =~ (^| )$c( |$) ]]", "(^| )$c( |$)"),
        ("[[ x =~ (a[ ]b) ]]", "(a[ ]b)"),
        ("[[ x =~ a(;)b ]]", "a(;)b"),
        ("[[ x =~ (a&b) ]]", "(a&b)"),
        ("[[ x =~ a(\n)b ]]", "a(\n)b"),
        ("[[ x =~ (#) ]]", "(#)"),
        ("[[ ab =~ a(b)# ]]", "a(b)#"),
        ("[[ ab =~ a|b ]]", "a|b"),
    ] {
        let words = texts(text, SyntaxKind::CondWord);
        assert_eq!(words.last().map(String::as_str), Some(operand), "{text:?}");
    }
    // Outside a group the operand ends where a word does, and bash refuses what follows.
    for text in ["[[ \"a b\" =~ a[ ]b ]]", "[[ a =~ a&b ]]", "[[ a =~ a>b ]]"] {
        assert!(!parse(text).is_clean(), "{text:?}");
    }
}

#[test]
fn a_regex_test_holds_inside_a_substitution() {
    clean("x=$( [[ a =~ (a| b) ]] && echo y ); echo $x");
    clean("until [[ a =~ (a| b) ]]; do :; done");
    clean("[[ a =~ ^(a)$ && b =~ (b| c) ]]");
}

#[test]
fn a_group_closes_on_its_own_parenthesis() {
    for (text, groups) in [
        ("[[ x && (a == a) ]]", 1),
        ("[[ (abc =~ b) ]]", 1),
        ("[[ (a == a) && (b == b) ]]", 2),
        ("[[ ! (x == y) ]]", 1),
        ("[[ !(a == a) ]]", 1),
    ] {
        assert_eq!(texts(text, SyntaxKind::CondGroup).len(), groups, "{text:?}");
    }
    // A comparison's pattern is still a pattern.
    assert!(texts("[[ ab == !(x) ]]", SyntaxKind::CondGroup).is_empty());
}

#[test]
fn an_arithmetic_command_closes_on_its_own_parentheses() {
    for (text, expression) in [
        ("((1 || (1 && 1)))", "1 || (1 && 1)"),
        ("((x = 1 + (2)))", "x = 1 + (2)"),
        ("((($(echo 7) & 3) == 3))", "($(echo 7) & 3) == 3"),
        ("(( $(echo 3) > 5 ))", " $(echo 3) > 5 "),
        ("(($(echo 1)))", "$(echo 1)"),
        ("((x = 1 << 2))", "x = 1 << 2"),
        ("(( i++ )) 2>/dev/null", " i++ "),
    ] {
        let parsed = clean(text);
        let item = Script::of(parsed.tree())
            .items()
            .next()
            .expect("a statement");
        let Some(Command::Arithmetic(command)) = item.command() else {
            panic!("{text:?} is not an arithmetic command");
        };
        assert_eq!(command.expression(parsed.tree().source()), expression);
    }
    clean("if (( $(echo 3) > 5 )); then :; fi");
    clean("while (( $(echo $i) < 2 )); do :; done");
    clean("((x = 1 << 2)); echo $x\necho next\n");
}

#[test]
fn an_arithmetic_for_header_is_read_by_its_parentheses() {
    let text = "for ((i = (1); i < 3; i += (1))); do :; done";
    let parsed = clean(text);
    let item = Script::of(parsed.tree())
        .items()
        .next()
        .expect("a statement");
    let Some(Command::For(looping)) = item.command() else {
        panic!("not a loop");
    };
    assert_eq!(
        looping.arithmetic(parsed.tree().source()),
        Some("i = (1); i < 3; i += (1)")
    );
    clean("for ((i=0; i<1<<1; i++)); do echo $i; done");
}

#[test]
fn a_hash_after_a_closing_parenthesis_is_part_of_the_word() {
    for (text, word) in [
        ("echo $((1))#x", "$((1))#x"),
        ("x=$(echo 1)#frag", "$(echo 1)#frag"),
        ("cat <(echo a)#b", "<(echo a)#b"),
    ] {
        assert!(
            texts(text, SyntaxKind::Word).iter().any(|w| w == word),
            "{text:?}"
        );
    }
    // After an arithmetic *command*, a new word begins and `#` is a comment.
    clean("((1)) # done");
}

#[test]
fn a_substitution_begins_where_a_comment_can() {
    let text = "x=$(# comment with ) inside\necho hi); echo \"$x\"";
    assert_eq!(texts(text, SyntaxKind::CommandSubstitution).len(), 1);
    clean("cat <(# a ) note\necho a)");
}

#[test]
fn a_heredoc_delimiter_loses_only_its_quoting() {
    clean("cat <<$'EOF'\nbody\nEOF\n");
    clean("cat <<'E\"OF'\nbody\nE\"OF\n");
    clean("cat <<\"E\\\"F\"\nbody\nE\"F\n");
    clean("cat <<$\"EOF\"\nbody\nEOF\n");
}

#[test]
fn a_heredoc_inside_a_substitution_hides_its_closers() {
    for text in [
        "x=$(cat <<EOF\nDon't\nEOF\n)",
        "x=$(cat <<EOF\na \"b\nEOF\n)",
        "x=$(cat <<'EOF'\na `b\nEOF\n)",
        "x=\"$(cat <<EOF\nDon't ) \"\nEOF\n)\"",
    ] {
        assert_eq!(
            texts(text, SyntaxKind::CommandSubstitution).len(),
            1,
            "{text:?}"
        );
    }
}

#[test]
fn an_unterminated_heredoc_is_a_warning_bash_would_give() {
    let parsed = parse("cat <<EOF\nunterminated\n");
    assert_eq!(parsed.errors().len(), 1);
    let error = &parsed.errors()[0];
    assert_eq!(error.severity, crate::error::Severity::Warning);
    assert!(error.unfinished, "a prompt still reads another line");
    assert_eq!(parsed.tree().reconstruct(), "cat <<EOF\nunterminated\n");
}

#[test]
fn negation_can_repeat() {
    for (text, negated) in [("! ! false", false), ("! ! ! true", true), ("! true", true)] {
        let parsed = clean(text);
        let item = Script::of(parsed.tree())
            .items()
            .next()
            .expect("a statement");
        assert_eq!(
            item.pipeline().map(|p| p.is_negated()),
            Some(negated),
            "{text}"
        );
    }
}

#[test]
fn a_loop_body_can_be_a_brace_group() {
    for text in [
        "for i in 1 2; { echo $i; }",
        "for x\n{ echo $x; }",
        "for x in a b\n{ echo $x; }",
        "for x in; { echo none; }",
        "for ((i=0;i<2;i++)); { echo $i; }",
        "for ((i=0;i<2;i++))\n{ echo $i; }",
        "for ((i=0;i<2;i++)) { echo $i; }",
        "for x in a; { echo $x; } > out",
        "select x in a; { echo $x; }",
        "for x\nin a b; do echo $x; done",
    ] {
        let parsed = clean(text);
        let item = Script::of(parsed.tree())
            .items()
            .next()
            .expect("a statement");
        let Some(Command::For(looping)) = item.command() else {
            panic!("{text:?} is not a loop");
        };
        assert!(looping.body().is_some(), "{text:?} has no body");
    }
    // Without a separator the `{` is not reserved, and bash refuses the line.
    assert!(!parse("for x { echo $x; }").is_clean());
}

/// The name and the subscript text of the last assignment in `text`.
fn element(text: &str) -> (String, String) {
    let parsed = clean(text);
    let mut nodes = Vec::new();
    find(parsed.tree().root(), SyntaxKind::Assignment, &mut nodes);
    let last = nodes.last().copied().expect("an assignment");
    let assignment = crate::ast::Assignment::cast(last).expect("an assignment");
    let source = parsed.tree().source();
    let subscript = assignment
        .subscript()
        .map(|node| source.slice(node.span()).to_string())
        .unwrap_or_default();
    (assignment.name(source).to_string(), subscript)
}

#[test]
fn a_subscript_with_an_expansion_still_assigns() {
    for (text, subscript) in [
        ("a[$i]=Y", "[$i]"),
        ("a[${#a[@]}]=y", "[${#a[@]}]"),
        ("a[$((1))]=", "[$((1))]"),
        ("a[\"$k\"]=w", "[\"$k\"]"),
        ("a[$i]+=x", "[$i]"),
        ("a[$i+1]=q", "[$i+1]"),
        ("a[ $i ]=s", "[ $i ]"),
        ("a[$(echo 3)]=t", "[$(echo 3)]"),
        ("a[i<2]=x", "[i<2]"),
        ("x=1 a[ $i ]=s", "[ $i ]"),
        ("x=$(echo 1) a[$j]=s", "[$j]"),
        ("b[3]=z", "[3]"),
    ] {
        let name = text
            .rsplit(' ')
            .find(|word| word.contains('['))
            .unwrap_or(text);
        let name = name.split('[').next().unwrap_or("");
        assert_eq!(
            element(text),
            (name.to_string(), subscript.to_string()),
            "{text:?}"
        );
    }
    // Where an assignment cannot stand, the same characters are ordinary words.
    let words = texts("declare a[ $i ]=x", SyntaxKind::Word);
    assert_eq!(words, ["declare", "a[", "$i", "]=x"]);
    let words = texts("echo a[ 1 ]=x", SyntaxKind::Word);
    assert_eq!(words, ["echo", "a[", "1", "]=x"]);
}

/// The coprocess in `text`: its name, if one was written, and the kind of its command.
fn coprocess(text: &str) -> (Option<String>, SyntaxKind) {
    let parsed = clean(text);
    let mut nodes = Vec::new();
    find(parsed.tree().root(), SyntaxKind::CoprocCommand, &mut nodes);
    assert_eq!(nodes.len(), 1, "{text:?} holds one coprocess");
    let coproc = crate::ast::CoprocCommand::cast(nodes[0]).expect("a coprocess");
    let source = parsed.tree().source();
    let name = coproc
        .name()
        .map(|word| source.slice(word.span()).to_string());
    let command = coproc.command().expect("a command").syntax().kind();
    (name, command)
}

#[test]
fn a_coprocess_is_named_only_before_a_compound_command() {
    for (text, name, command) in [
        ("coproc cat file", None, SyntaxKind::SimpleCommand),
        ("coproc NAME cat", None, SyntaxKind::SimpleCommand),
        ("coproc { cat; }", None, SyntaxKind::Group),
        ("coproc NAME { cat; }", Some("NAME"), SyntaxKind::Group),
        ("coproc W (cat)", Some("W"), SyntaxKind::Subshell),
        (
            "coproc W while read l; do :; done",
            Some("W"),
            SyntaxKind::WhileCommand,
        ),
        ("coproc [[ -n x ]]", None, SyntaxKind::CondCommand),
        ("coproc cat > out 2>&1", None, SyntaxKind::SimpleCommand),
    ] {
        assert_eq!(
            coprocess(text),
            (name.map(str::to_string), command),
            "{text:?}"
        );
    }
    // Redirections after a compound command are that command's.
    let parsed = clean("coproc N { cat; } >log 2>&1");
    let mut groups = Vec::new();
    find(parsed.tree().root(), SyntaxKind::Group, &mut groups);
    assert_eq!(
        groups[0]
            .nodes()
            .filter(|n| n.kind() == SyntaxKind::Redirect)
            .count(),
        2
    );
    // In a pipeline, and not a keyword anywhere a command cannot start.
    clean("coproc cat | wc -l");
    assert!(texts("echo coproc x", SyntaxKind::CoprocCommand).is_empty());
    assert!(!parse("coproc").is_clean());
}

#[test]
fn a_coprocess_formats_as_it_reads() {
    for text in [
        "coproc cat file\n",
        "coproc NAME { cat; }\n",
        "coproc ( cat ) >log\n",
    ] {
        assert_eq!(crate::format(text).as_deref(), Ok(text), "{text:?}");
    }
}

/// The variable of every redirection in `text`.
fn variables(text: &str) -> Vec<String> {
    let parsed = clean(text);
    let mut nodes = Vec::new();
    find(parsed.tree().root(), SyntaxKind::Redirect, &mut nodes);
    nodes
        .iter()
        .filter_map(|node| crate::ast::Redirect::cast(node))
        .filter_map(|redirect| redirect.variable(parsed.tree().source()))
        .map(str::to_string)
        .collect()
}

#[test]
fn a_descriptor_variable_belongs_to_its_redirection() {
    assert_eq!(variables("exec {fd}>f"), ["fd"]);
    assert_eq!(variables("{ echo; } {fd}>f"), ["fd"]);
    assert_eq!(variables("while :; do :; done {fd}<f"), ["fd"]);
    assert_eq!(variables("cat {a[1]}<f {b}>&-"), ["a[1]", "b"]);
    assert_eq!(variables("{fd}>f"), ["fd"]);
    // Not touching, or not a name: an ordinary word.
    assert!(variables("echo {fd} >f").is_empty());
    assert!(variables("echo {1}>f").is_empty());
    assert!(texts("echo {fd} >f", SyntaxKind::Word).contains(&"{fd}".to_string()));
    assert_eq!(
        crate::format("{ echo; } {fd}>f\n").as_deref(),
        Ok("{ echo; } {fd}>f\n")
    );
}

/// A subshell whose first command is an arithmetic command: `(((1/0)); echo x)`.
///
/// The outer `((` closes apart, so it is a `(` and then a `((` that does close together.
#[test]
fn a_subshell_can_open_with_an_arithmetic_command() {
    for text in [
        "(((1/0)); echo x)",
        "(((x = 1)) && echo y)",
        "( ((1)); echo x)",
        "(((1)) )",
    ] {
        assert_eq!(texts(text, SyntaxKind::Subshell).len(), 1, "{text}");
        assert_eq!(texts(text, SyntaxKind::ArithCommand).len(), 1, "{text}");
    }
}

/// An array value the input ends inside is unfinished, so a reader takes the next line for it.
#[test]
fn an_open_array_value_is_unfinished() {
    for text in ["a=(1 \\\n", "a=(1\n", "declare -A m=([x]=1 \\\n"] {
        assert_eq!(
            parse(text).completeness(),
            crate::Completeness::Unfinished,
            "{text:?}"
        );
    }
    clean("a=(1 \\\n 2)\n");
}
