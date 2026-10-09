set positional-arguments

# The prefix that puts a command inside the flake's shell: nothing where the shell's
# marker `SPLICE_SHELL` is set, so a command already inside it runs in place, and
# `nix develop` everywhere else, since a plain shell resolves the machine's toolchain
# rather than the project's.
in_shell := if env("SPLICE_SHELL", "") == "1" { "" } else { "nix develop --command" }

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

# Installs the binary onto the machine's path with cargo, from this checkout.
install:
    cargo install --locked --path .
