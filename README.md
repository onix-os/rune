# rune

A shell parser that keeps going.

`rune` reads POSIX shell — and the parts of bash worth reading — and produces a tree. It does not
execute anything, expand anything, and it does not stop at the first mistake. A script with nine
errors in it reports nine.

Every byte of the input is in the tree: whitespace, comments, and the text of things that did not
parse. Concatenating the tree's tokens returns the input unchanged, which is the invariant the
whole crate rests on and the one the test suite asserts on everything it parses. Recovery, syntax
highlighting and a formatter are all built on it.

## State

Early. The tree and its builder, spans, the source index, and the tokenizer exist and are tested.
The tokenizer handles quoting, escapes, every `$` form, and here-documents, and it accounts for
every byte of oslo's 432-script corpus with nothing left unrecognised. There is no grammar yet, so
nothing produces a tree from shell — see `PLAN.md` for the order the rest arrives in.

```sh
# lex a directory of real scripts and report what turns up
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
