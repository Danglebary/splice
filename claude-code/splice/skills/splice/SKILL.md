---
name: splice
description: Edit files from Bash with `splice` instead of sed -i, perl -i, awk, or a Python or Node script. Use whenever an edit would otherwise go through a one-off script or several Edit calls - several edits in one call, an edit chained with a build or test, a line number from a compiler error, an insertion after an anchor line, a region between two markers, a rename across files, rows appended to a file, or a temporary edit (a mutation test) that must be restored after a command runs.
---

# Editing with splice

`splice` reads an edit script on stdin and applies every hunk to every file, or writes nothing at all. It matches text literally, by whole lines or, under `@@ inline`, anywhere within a line, so nothing in the script is ever escaped, and it checks each hunk's match count before it writes. It prints nothing on success; its exit code is the result.

Use it for any edit made from Bash. The Edit tool stays fine for a single change to a file you have open; reach for splice when the edit is a batch, belongs in the same call as its check, or is anchored by line number or by markers. Never edit a file through `sed -i`, `perl -i`, `awk -i inplace`, or a Python or Node write: the hook blocks those commands.

## The shape

Always pass the script as a quoted heredoc (`<<'EOF'`, quotes included), so the shell leaves its content alone, and chain the check with `&&`, never `;`, so it runs only on an edit that landed:

```bash
splice <<'EOF' && cargo test -p ledger
=== crates/ledger/src/store.rs
@@
 fn open(path: &Path) -> Result<Store, Error> {
-    let file = File::open(path)?;
+    let file = File::open(path).map_err(Error::Open)?;
 }
=== crates/ledger/src/lib.rs
@@ append
+pub mod store;
EOF
```

- `=== PATH` starts a file's hunks. Consecutive `===` lines share the hunks below them.
- `@@` starts a hunk. Every line in it begins with one marker: ` ` (a space) for a context line, matched and kept; `-` for a removed line, matched and removed; `+` for an added line.
- `~` alone on a line stands for any run of lines, ending where the next matched line matches. Under a context line the run is kept; under a removed line it is removed.

## Headers

| Header | Meaning |
| --- | --- |
| `@@` | exactly one match, or the hunk is refused |
| `@@ all` | one match or more, and each one changes |
| `@@ count N` | exactly N matches |
| `@@ line N` | the match starts at line N; use the line a compiler error names |
| `@@ regex`, `@@ regex all`, `@@ regex count N` | `-` lines join into one regex, `+` lines into its replacement (`$1`, `${name}`, `$$` for `$`) |
| `@@ inline`, `@@ inline all`, `@@ inline count N` | `-` lines join into text found anywhere, mid-line included, and `+` lines into what replaces it; no context lines |
| `@@ append`, `@@ append jsonl` | `+` lines added at the end; `jsonl` checks each line parses as JSON |
| `@@ create` | `+` lines written to a file that does not exist yet |
| `@@ final newline` | ends the last line of a file that lacks a final newline; no lines follow it |

Every hunk matches the file as it was before any hunk applied, so line numbers stay valid across a batch: write hunks in any order and never adjust a line number for an earlier hunk. Two hunks may not change the same lines; merge them into one. A file that lacks a final newline keeps lacking one through every edit unless `@@ final newline` adds it.

## Patterns

Insert after an anchor line, keeping the anchor:

```
@@
 mod tests;
+mod fixtures;
```

Replace a region between markers without restating it. The first line of the region is written as removed, so `~` removes the rest up to the end marker:

```
@@
 // BEGIN generated
-pub const A: u8 = 1;
~
+pub const A: u8 = 2;
+pub const B: u8 = 3;
 // END generated
```

Delete a whole function by its first and last lines:

```
@@
-fn retired() {
~
-}
```

Change a phrase inside a long line, such as a paragraph kept on one line or a JSON payload on one line, without restating the line:

```
=== docs/guide.md
@@ inline
-the store keeps every event
+the store keeps every event in order
```

Rename across files, with the files listed and the expectation stated:

```
=== src/a.rs
=== src/b.rs
@@ regex all
-\bold_name\b
+new_name
```

Fix the line a compiler error points at, guarded by its content:

```
@@ line 212
-    let limit = 50;
+    let limit = LIMIT_ROWS_MAX;
```

## Mutation tests

`splice try` applies the script, runs a command, and restores every file afterwards, even when the command fails or is interrupted. It refuses to run the command when a hunk does not match, so a mutation that did not apply never passes for one that survived.

```bash
splice try --expect-fail -- cargo test -p ledger <<'EOF'
=== crates/ledger/src/bound.rs
@@
-    if count > LIMIT {
+    if count >= LIMIT {
EOF
```

Under `--expect-fail` the exit code is 0 when the tests failed (the mutation was caught) and 4 when they passed (it survived). Without it, the command's own exit code passes through. An interrupted run exits with 128 plus the signal's number, never 0, so it never passes for a caught mutation.

## When a script is refused

Exit 1 means a hunk did not match as declared and nothing was written; exit 2 means the script itself is malformed. stderr names the file, the hunk's script line, the matches found and their lines, and the closest the file came: the first line that differs, with both shown quoted so a tab or a trailing space is visible. Rewrite the hunk from the file's own text and run the whole script again.

`--diff` prints the diff of what was written; `--dry-run` prints it and writes nothing. `splice --help` prints the full grammar.

## Leave to other tools

- A structural JSON or YAML edit (set a key, filter an array): `jq`/`yq` into a temporary file and `mv` it back. splice matches text, not structure; a phrase inside a JSON string is text, and `@@ inline` changes it.
- Saving command output to a file: a redirect, as always.
- A whole new file: the Write tool, or `@@ create` when it belongs in the same batch.
