# splice

A static Rust binary that applies literal, count-checked edit scripts to text files, and a Claude Code plugin (a skill and a hook) that routes agents to it. `README.md` says what it is for; `splice --help`, generated from `src/report/mod.rs`, is the grammar.

The engineering style guide at `~/.claude/CLAUDE.md` governs. This file states only what is specific to this repository.

## Running things

`just check` is the gate and must stay green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` under the lint table in `Cargo.toml`, `cargo test`, and `claude plugin validate --strict` over the marketplace and the plugin. The Rust recipes run inside the flake's shell, entering `nix develop` themselves unless `SPLICE_SHELL` shows they already stand in it; the validator runs the `claude` on `PATH`. `just splice <arguments>` runs the binary from the tree.

The toolchain is pinned once, in `rust-toolchain.toml`, which both the flake and CI's rustup read.

## Shape

`src/lib.rs` is the functional core and does no IO: the script grammar (`script`), matching and splicing one file's text (`plan`), the diff (`diff`), the hook's verdict (`guard`), the argument grammar (`cli`), every message (`report`), and the exit codes (`exit_code`). `src/main.rs` is the shell: it reads the script and the files, writes atomically, runs the command under `try`, and maps outcomes to exit codes. A new decision goes in the library with a unit test; `main.rs` holds wiring, which `tests/cli.rs` covers against real files.

The lint table denies `unwrap`, `expect`, `panic!`, indexing, slicing, string slicing, `as` casts, and unchecked arithmetic. A programmer error crashes through `assert!` or `unreachable!` with a message; arithmetic goes through `checked_*` with a `let ... else { unreachable!(..) }`, and slicing through `.get(..)`.

## Tests

A test is a function named `when_<operation>_then_<result>` in a module named `given_<input>`, and each test file opens with its goal and method as a `//!` comment. Each case asserts on a returned value, a refusal's reason, or a file's contents; the `report` tests read messages because a message is what that module produces.

`tests/documents.rs` holds the plugin manifest's version equal to the crate's and parses every script example in the skill, so the skill cannot drift from the grammar.

The hook's verdict was checked against every Bash command in this machine's Claude Code transcripts; when `guard` changes, rerun that check rather than trusting the unit rows alone: feed each command through `splice guard` as hook JSON and read what it blocks.

## Releases

Raise `version` in `Cargo.toml` and in `claude-code/splice/.claude-plugin/plugin.json` together and merge to `main`. `.github/workflows/release.yml` publishes a release when the version has no release yet and builds nothing otherwise. Archives are named by target alone, which `install.sh` depends on.
