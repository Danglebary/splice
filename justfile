set positional-arguments

# The prefix that puts a command inside the flake's shell: nothing where the shell's
# marker `SPLICE_SHELL` is set, so a command already inside it runs in place, and
# `nix develop` everywhere else, since a plain shell resolves the machine's toolchain
# rather than the project's.
in_shell := if env("SPLICE_SHELL", "") == "1" { "" } else { "nix develop --command" }

# How long `just profile` runs the cases it records: long enough for thousands of samples
# at samply's default rate of one per millisecond.
profile_seconds := "5"

# glibc's malloc thresholds for the benches. glibc adjusts both from the allocations a
# process has made, which decides whether an iteration's large buffers come fresh from the
# kernel or reuse the heap, so a case can time page faults on one run and not the next.
# Setting them stops that adjustment: every chunk up to 32 MiB comes from the heap, and
# freed memory stays mapped until 256 MiB sits free at the heap's top, so an iteration
# times its own work. Other C libraries do not read the variable.
# https://sourceware.org/glibc/manual/latest/html_node/Memory-Allocation-Tunables.html
# — checked 2026-10-09
malloc_tunables := "glibc.malloc.mmap_threshold=33554432:glibc.malloc.trim_threshold=268435456"

# The `splice` binary built from the tree, its arguments passed through whole.
splice *ARGS:
    @{{in_shell}} cargo run --quiet -- "$@"

# The gate: the Rust half, then Claude Code's validator over the marketplace and the
# plugin.
check: check-rust check-claude-code

check-rust:
    {{in_shell}} cargo fmt --all --check
    {{in_shell}} cargo clippy --all-targets -- -D warnings
    {{in_shell}} cargo test

# The validator is the `claude` on `PATH`: Claude Code is unfree in nixpkgs, so the
# flake carries none.
check-claude-code:
    claude plugin validate --strict .
    claude plugin validate --strict claude-code/splice

# The benchmarks, which stand outside the gate because wall-clock timings vary between
# runs and machines. Arguments pass to `cargo bench`, and those after `--` to criterion,
# such as a filter or `--save-baseline <name>`.
bench *ARGS:
    @{{in_shell}} env "GLIBC_TUNABLES={{malloc_tunables}}" cargo bench "$@"

# Records one bench under samply: `just profile plan elision-to-absent-line/120001`. The
# bench is built under the profiling profile, so its frames carry names, and cargo runs it
# through samply, which records it and every process it starts. criterion's
# `--profile-time` runs the cases the filter matches without analysis, under the malloc
# thresholds `bench` times them with, and the recording lands at
# `target/profiling/<bench>.json`. On Linux samply reads perf events, which an
# unprivileged user may open only while `kernel.perf_event_paranoid` is 1 or lower, and
# maps a 1,028 KiB ring buffer per CPU, which can fail with `mmap failed` on a host of
# many CPUs until `kernel.perf_event_mlock_kb` is at least 1028.
profile BENCH FILTER:
    {{in_shell}} env "GLIBC_TUNABLES={{malloc_tunables}}" cargo bench --profile profiling --bench "$1" --config "target.'cfg(all())'.runner = ['samply', 'record', '--save-only', '--output', 'target/profiling/$1.json', '--']" -- --profile-time {{profile_seconds}} "$2"

# Opens a bench's last recording in the profiler's browser view, served from this machine.
profile-view BENCH:
    {{in_shell}} samply load "target/profiling/$1.json"

# Installs the binary onto the machine's path with cargo, from this checkout.
install:
    cargo install --locked --path .
