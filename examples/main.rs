//! Build a small tree by hand and show it. There is no tokenizer yet.

use rune::{Builder, Source, SyntaxKind};

fn main() {
    let mut builder = Builder::new(Source::new("echo hi\n"), SyntaxKind::Script);
    builder.start(SyntaxKind::SimpleCommand);
    builder.start(SyntaxKind::Word);
    builder.token(SyntaxKind::Text, 4);
    builder.finish();
    builder.token(SyntaxKind::Whitespace, 1);
    builder.start(SyntaxKind::Word);
    builder.token(SyntaxKind::Text, 2);
    builder.finish();
    builder.finish();
    builder.token(SyntaxKind::Newline, 1);

    let tree = builder.build();
    print!("{}", tree.dump());
    println!(
        "reconstructs: {}",
        tree.reconstruct() == tree.source().text()
    );
}
