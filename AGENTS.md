# splice

A static Rust binary that applies literal, count-checked edit scripts to text files, and a Claude Code plugin (a skill and a hook) that routes agents to it. `README.md` says what it is for; `splice --help`, generated from `src/report/mod.rs`, is the grammar.

The engineering style guide at `~/.claude/CLAUDE.md` governs. This file states only what is specific to this repository.

## Running things

`just check` is the gate and must stay green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` under the lint table in `Cargo.toml`, `cargo test`, and `claude plugin validate --strict` over the marketplace and the plugin. The Rust recipes run inside the flake's shell, entering `nix develop` themselves unless `SPLICE_SHELL` shows they already stand in it; the validator runs the `claude` on `PATH`. `just splice <arguments>` runs the binary from the tree.

`just bench` runs the criterion benchmarks in `benches/`, which stand outside the gate because wall-clock timings vary between runs; `cargo clippy --all-targets` compiles them, so the gate still holds them to the lint table. Each case checks the outcome it declares before it is timed, and the cases that measure scaling count throughput in lines, so linear work holds its lines per second as the input grows. To compare a change, record `just bench -- --save-baseline before` on the parent commit and run `just bench -- --baseline before` on the change. The recipe fixes glibc's malloc thresholds, so an in-process case times its own work rather than page faults that come and go with the allocator's history; a bare `cargo bench` leaves them adjusting, and its numbers do not compare with the recipe's. `benches/process.rs` spawns the built binary under the allocator's defaults, so its timings include starting the process, piping stdin, and the edit's fsync, which the in-process benches leave out.

`just profile <bench> <filter>` records the cases the filter matches under samply, built with the `profiling` profile so frames carry names, and `just profile-view <bench>` opens the recording in the browser. On Linux samply needs `kernel.perf_event_paranoid` at 1 or lower, and on a host of many CPUs `kernel.perf_event_mlock_kb` at 1028 or more, because it maps a 1,028 KiB ring buffer per CPU and the kernel counts what exceeds that setting against the user's mlock limit; short of the first it refuses to record, and short of the second it fails with `mmap failed`.

The toolchain is pinned once, in `rust-toolchain.toml`, which both the flake and CI's rustup read.

CI also builds the flake's package on Linux and macOS. It compiles from the file set `flake.nix` lists, so a file the manifest names outside `src/`, such as a declared bench, joins that set.

## Shape

`src/lib.rs` is the functional core and does no IO: the script grammar (`script`), matching and splicing one file's text (`plan`), the diff (`diff`), the hook's verdict (`guard`), the argument grammar (`cli`), every message (`report`), and the exit codes (`exit_code`). `src/main.rs` is the shell: it reads the script and the files, writes atomically, runs the command under `try`, and maps outcomes to exit codes. A new decision goes in the library with a unit test; `main.rs` holds wiring, which `tests/cli.rs` covers against real files.

The lint table denies `unwrap`, `expect`, `panic!`, indexing, slicing, string slicing, `as` casts, and unchecked arithmetic. A programmer error crashes through `assert!` or `unreachable!` with a message; arithmetic goes through `checked_*` with a `let ... else { unreachable!(..) }`, and slicing through `.get(..)`.

## Tests

A test is a function named `when_<operation>_then_<result>` in a module named `given_<input>`, and each test file opens with its goal and method as a `//!` comment. Each case asserts on a returned value, a refusal's reason, or a file's contents; the `report` tests read messages because a message is what that module produces.

`tests/documents.rs` holds the plugin manifest's version equal to the crate's and parses every script example in the skill, so the skill cannot drift from the grammar.

The hook's verdict was checked against every Bash command in this machine's Claude Code transcripts; when `guard` changes, rerun that check rather than trusting the unit rows alone: feed each command through `splice guard` as hook JSON and read what it blocks.

## Releases

Raise `version` in `Cargo.toml` and in `claude-code/splice/.claude-plugin/plugin.json` together and merge to `main`. `.github/workflows/release.yml` publishes a release when the version has no release yet and builds nothing otherwise. Archives are named by target alone, which `install.sh` depends on.
