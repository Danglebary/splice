# splice

Coding agents edit files through one-off scripts: `sed -i`, `perl -0pi`, a `python3 - <<EOF` that reads, replaces, and writes, `jq > tmp && mv`. They do it to batch several edits into one call and to chain the edit with the test that checks it. The scripts fail in the same handful of ways: a `|` delimiter turned into regex alternation by Rust closures, a `\n` pattern `sed` never matches and never reports, an apostrophe workaround written into the source, a line number one off, a substitution that changed a line it was not meant to, and an interpreter missing from the machine. A review of about 1,100 such edits in real Claude Code sessions found about one in ten errored, did nothing, or changed the wrong thing.

splice is the tool those scripts were trying to be. It reads an edit script on stdin, matches literal text line by line, checks how many times each hunk matched, and writes every file or none. It prints nothing on success. A Claude Code plugin in this repository adds a skill that teaches agents the script and a hook that blocks the one-off editors.

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

`splice --help` prints the whole grammar: match counts, line anchors, `~` for a run of lines, inline hunks that change a phrase mid-line, regex hunks, appends, creates, a final newline for a file that lacks one, and `splice try`, which applies a script, runs a command, and restores every file afterwards.

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
| 3 | reading or writing a file failed; stderr names what was and was not written |

`splice try` exits with the command's own status, or under `--expect-fail` with 0 when the command failed and 4 when it passed.

## Development

`just check` is the gate: the format check, clippy with every warning denied, the tests, and Claude Code's plugin validator. The toolchain is the one `rust-toolchain.toml` pins; the flake's shell provides it, and the recipes enter that shell themselves.

A release is cut by raising `version` in `Cargo.toml` and in `claude-code/splice/.claude-plugin/plugin.json` (a test holds the two equal) and merging to `main`; the release workflow builds and publishes it.
