# splice

Coding agents edit files through one-off scripts: `sed -i`, `perl -0pi`, a `python3 - <<EOF` that reads, replaces, and writes, `jq > tmp && mv`. They do it to batch several edits into one call and to chain the edit with the test that checks it. The scripts fail in the same handful of ways: a `|` delimiter turned into regex alternation by Rust closures, a `\n` pattern `sed` never matches and never reports, an apostrophe workaround written into the source, a line number one off, a substitution that changed a line it was not meant to, and an interpreter missing from the machine. A review of about 1,100 such edits in real Claude Code sessions found about one in ten errored, did nothing, or changed the wrong thing.

splice is the tool those scripts were trying to be. It reads an edit script on stdin, matches literal text line by line, checks how many times each hunk matched, and writes every file or none. It prints nothing on success. A Claude Code plugin in this repository adds a skill that teaches agents the script and a hook that blocks the one-off editors.

Measured the same way over the 211 splice calls in this machine's sessions through 2026-10-10: 25 (12%) were refused, every refusal was correct and named the fix, every refused edit landed on the next attempt, and none of the 186 edits that landed changed text beyond what its hunk declared. The failure rate is the same; the failures are loud, happen before anything is written, and are the agent's to correct from the message.

```bash
splice <<'EOF' && cargo test
=== src/lib.rs
@@
 fn render(row: &Row) -> String {
-    format!("{}", row.label)
+    format!("{} ({})", row.label, row.id)
 }
=== src/main.rs
@@ line 212
-    let limit = 50;
+    let limit = LIMIT_ROWS_MAX;
EOF
```

Every hunk line opens with its marker: a space for a context line, `-` for a removed one, `+` for an added one, and `~` alone for any run of lines. A line holding only its marker is a blank line of that kind. The header after `@@` declares the match:

| Header | Meaning |
| --- | --- |
| `@@` | exactly one match |
| `@@ all` | one match or more, each one changed |
| `@@ count N` | exactly N matches |
| `@@ line N` | the match starts at line N |
| `@@ inline [all\|count N]` | `-` lines join into text found anywhere, mid-line included; `+` lines into what replaces it |
| `@@ regex [all\|count N]` | `-` lines join into one regex, `+` lines into its replacement (`$1`, `${name}`, `$$`) |
| `@@ append [jsonl]` | `+` lines added at the end; `jsonl` checks each line is JSON |
| `@@ create` | `+` lines written to a file that does not exist yet |
| `@@ final newline` | ends the last line of a file that lacks a final newline |

`splice try` applies a script, runs a command, and restores every file afterwards; `--expect-fail` turns a failing command into exit 0, for a mutation test. `splice --help` prints the whole grammar, and the Claude Code skill shows the patterns. The binary runs on Linux and macOS.

## Install

The binary, from the latest release (Linux and macOS, x86_64 and aarch64):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/Danglebary/splice/main/install.sh | sh
```

It installs to `~/.local/bin`, or to `$SPLICE_INSTALL_DIR`. With Nix, `nix profile install github:Danglebary/splice`; with cargo, `cargo install --locked --git https://github.com/Danglebary/splice`.

The Claude Code plugin, once the binary is on `PATH`:

```
/plugin marketplace add Danglebary/splice
/plugin install splice@splice
```

The hook runs `splice guard` before every Bash call. Without the binary on `PATH` every call reports a non-blocking hook error, which is the signal to install it.

## Exit status

| Code | Meaning |
| --- | --- |
| 0 | every file written |
| 1 | a hunk did not match as declared; nothing written |
| 2 | the script or the arguments are malformed; nothing written |
| 3 | reading or writing a file failed; stderr names any file left changed |

`splice try` exits with the command's own status, or under `--expect-fail` with 0 when the command failed and 4 when it passed. An interrupted `splice try` exits with 128 plus the signal's number, whatever the command did.

## Development

`just check` is the gate: the format check, clippy with every warning denied, the tests, and Claude Code's plugin validator. The toolchain is the one `rust-toolchain.toml` pins; the flake's shell provides it, and the recipes enter that shell themselves.

A release is cut by raising `version` in `Cargo.toml` and in `claude-code/splice/.claude-plugin/plugin.json` (a test holds the two equal) and merging to `main`; the release workflow builds and publishes it.
