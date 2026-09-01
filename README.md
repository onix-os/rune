# rune

A shell parser that keeps going.

`rune` reads POSIX shell — and the parts of bash worth reading — and produces a tree. It does not
execute anything, expand anything, and it does not stop at the first mistake. A script with nine
errors in it reports nine.

Every byte of the input is in the tree: whitespace, comments, and the text of things that did not
parse. Concatenating the tree's tokens returns the input unchanged, which is the invariant the
whole crate rests on and the one the test suite asserts on everything it parses. Recovery, syntax
highlighting and a formatter are all built on it.

## What it does so far

Reads shell and gives back a tree, with a typed view over it and a list of what it could not make
sense of.

- **Tokenizer** — quoting (including `$'…'`, where a backslash protects the closing quote), every
  `$` form, process substitution, and here-documents, which begin on the line *after* the one that
  asked for them.
- **Grammar** — hand-written recursive descent. Pipelines, and-or lists, every compound command,
  functions, assignments and redirections. Reserved words are recognised where they are reserved:
  `if` opens a conditional, `echo if` prints a word.
- **Recovery** — one mistake, one message. A stray `done` used to produce 133 errors in one file;
  it now produces two. Each report names the construct, points at where it opened, and says what
  would have closed it.
- **Typed view** — `Script::of(tree)` and accessors down from there. Nothing owns anything, so the
  view cannot drift from the tree.
- **`Parsed::completeness`** — `Complete`, `Unfinished` or `Invalid`, which is what an interactive
  prompt needs to decide between running a line, reading another, and complaining.

Against oslo's corpus of 432 real scripts: every one reconstructs byte for byte, and 424 parse with
nothing to report. Seven of the other eight are fixtures written to be broken; the last is a
genuine unterminated quote.

Not done: the arithmetic sub-grammar, and single-token repair for typos. See `PLAN.md`.

```sh
# parse a script and show the tree
cargo run --example main -- script.sh

# check the parser against a directory of real scripts
RUNE_CORPUS=/path/to/scripts cargo test --test against_a_corpus -- --ignored --nocapture
```

## Commands

The build is `.make.lua`, read by [oslo](https://github.com/termworks/oslo). At an oslo prompt in
this directory `make` is enough; anywhere else it is `oslo make`.

```sh
make                      # the recipes, with what each of them says it does
make build
make run --args='--help'
make test
make verify
make release --type patch
```

The directory environment is `.env.lua`, loaded when you `cd` here and unloaded when you leave. It
brings up the flake's dev shell and defines `_b`, `_r`, `_t`, `_v` and `_i` for the commands above.

## Requirements

`.make.lua` and `.env.lua` are read by [oslo](https://github.com/termworks/oslo), which provides
both the `make` task runner and the directory environment. Without it, `make` is whatever is on
your `$PATH` and `.env.lua` is never loaded.

```sh
# at an oslo prompt in this directory
make build

# anywhere else
oslo make build
```
