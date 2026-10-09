//! Every message splice prints, rendered from the library's typed outcomes. Pure, so
//! each message is built from values a test constructs.

use crate::cli::UsageFault;
use crate::guard::{HookInputFault, Offense};
use crate::plan::{NearMiss, Reason, Refusal};
use crate::script::{Expectation, Fault, ScriptError};

/// The whole grammar, which is what an agent reads before writing its first script.
pub const HELP: &str = "\
splice: apply literal, count-checked edits to text files, all of them or none.

Usage:
  splice [--diff] [--dry-run] [--script PATH]
      Apply the script read from stdin, or from PATH.
  splice try [--expect-fail] [--diff] [--script PATH] -- COMMAND [ARGS...]
      Apply the script, run COMMAND, then restore every file it touched.
  splice guard
      Claude Code PreToolUse hook: reads the hook's JSON on stdin and blocks a Bash
      command that edits a file through sed -i, perl -i, a Python or Node write, or a
      rewrite moved over the original.

Silent on success. --diff prints the diff of what was written; --dry-run prints the
diff and writes nothing.

Script, best passed as a quoted heredoc so nothing in it is ever escaped:

  splice <<'EOF' && cargo test
  === src/lib.rs
  @@
   fn render(row: &Row) -> String {
  -    format!(\"{}\", row.label)
  +    format!(\"{} ({})\", row.label, row.id)
   }
  EOF

  === PATH       Starts the hunks for one file. Consecutive === lines share the hunks below.
  @@ [HEADER]    Starts a hunk. Every hunk line begins with one marker:
   text          a context line: matched and kept
  -text          a removed line: matched and removed
  +text          an added line
  ~              any run of lines, ending where the next matched line matches; kept
                 under a context line, removed under a removed line

Headers:
  @@                     exactly one match
  @@ all                 one match or more, each one changed
  @@ count N             exactly N matches
  @@ line N              the match starts at line N of the file
  @@ regex [all|count N] '-' lines join into one regex, '+' lines into its replacement
                         ($1, ${name}; $$ for a literal $)
  @@ append [jsonl]      '+' lines added at the end; jsonl checks each line is JSON
  @@ create              '+' lines written to a file that does not exist yet

Matching is literal and line by line. Every hunk matches the file as it was before any
hunk applied, so line numbers stay valid across a batch and hunks must not overlap. A
hunk that would leave its file unchanged is refused. Blank lines at a hunk's edges are
dropped; write a lone space for a blank context line there.

Exit status:
  0  every file written
  1  refused: a hunk did not match as declared, and nothing was written
  2  the script or the arguments are malformed, and nothing was written
  3  reading or writing a file failed
  Under try: the command's own status; with --expect-fail, 0 when the command failed
  and 4 when it passed.
";

pub const NOTHING_WRITTEN: &str = "splice: nothing written\n";

pub const INTERRUPTED: &str = "splice: interrupted; the files are restored\n";

#[must_use]
pub fn script_error(error: &ScriptError) -> String {
    format!(
        "splice: script line {}: {}\n",
        error.line,
        fault(&error.fault)
    )
}

fn fault(fault: &Fault) -> String {
    match fault {
        Fault::EmptyScript => "the script names no file; it starts with `=== PATH`".to_owned(),
        Fault::LineBeforeFirstFile => "text before the first `=== PATH` line".to_owned(),
        Fault::LineBeforeFirstHunk => "a line between `=== PATH` and the first `@@` header".to_owned(),
        Fault::EmptyPath => "`===` names no path".to_owned(),
        Fault::PathRepeated { first_line } => {
            format!("this path is already named at script line {first_line}; give each file one section")
        }
        Fault::FileWithoutHunks => "no `@@` hunk follows this file".to_owned(),
        Fault::UnknownHeader { header } => format!(
            "unknown header `@@ {header}`; expected `@@`, `@@ all`, `@@ count N`, `@@ line N`, \
             `@@ regex [all|count N]`, `@@ append [jsonl]`, or `@@ create`"
        ),
        Fault::InvalidNumber { word } => format!("`{word}` is not a whole number of 1 or more"),
        Fault::UnexpectedLine => {
            "a hunk line starts with ' ' (context), '-' (removed), or '+' (added), or is `~` alone".to_owned()
        }
        Fault::NoAnchor => {
            "the hunk has no context or removed line to match; `@@ append` and `@@ create` add without matching"
                .to_owned()
        }
        Fault::ChangesNothing => "the hunk removes and adds nothing".to_owned(),
        Fault::ElisionWithoutLineAbove => "`~` needs a context or removed line directly above it".to_owned(),
        Fault::ElisionWithoutLineBelow => "`~` needs a context or removed line below it to end the run".to_owned(),
        Fault::RegexWithoutPattern => "a regex hunk needs '-' lines holding its pattern".to_owned(),
        Fault::RegexWithContext => "a regex hunk holds '-' pattern lines and '+' replacement lines alone".to_owned(),
        Fault::RegexInvalid { message } => format!("the pattern does not compile: {message}"),
        Fault::OnlyAddedLines => "this hunk holds '+' lines alone".to_owned(),
        Fault::AppendEmpty => "the append adds no line".to_owned(),
        Fault::JsonlInvalid { message } => format!("the line is not JSON: {message}"),
        Fault::CreateNotAlone => "`@@ create` is the only hunk for its file".to_owned(),
    }
}

#[must_use]
pub fn refusal(path: &str, refusal: &Refusal) -> String {
    let mut message = format!(
        "splice: {path}: hunk at script line {}: {}\n",
        refusal.hunk_line,
        reason(&refusal.reason)
    );
    if let Reason::Count {
        near_miss: Some(near),
        ..
    } = &refusal.reason
    {
        message.push_str(&near_miss(near));
    }
    message
}

fn reason(reason: &Reason) -> String {
    match reason {
        Reason::Missing => "the file does not exist; `@@ create` writes a new file".to_owned(),
        Reason::Exists => "`@@ create` found the file already there".to_owned(),
        Reason::NotText => "the file is not UTF-8 text".to_owned(),
        Reason::TooLarge { bytes } => {
            format!("the file holds {bytes} bytes, past the limit splice reads")
        }
        Reason::SameFileAs { path } => {
            format!("this path names the same file as `{path}`; give each file one section")
        }
        Reason::Count {
            expected,
            found,
            found_lines,
            ..
        } => {
            format!(
                "expected {}, found {found}{}",
                expectation(*expected),
                at_lines(*found, found_lines)
            )
        }
        Reason::Overlap { other_hunk_line } => {
            format!(
                "it changes lines the hunk at script line {other_hunk_line} also changes; merge the two hunks"
            )
        }
        Reason::Unchanged => {
            "the replacement equals the matched text, so the hunk changes nothing".to_owned()
        }
    }
}

fn expectation(expectation: Expectation) -> String {
    match expectation {
        Expectation::Once => "exactly 1 match".to_owned(),
        Expectation::All => "at least 1 match".to_owned(),
        Expectation::Exactly(declared) => format!("exactly {declared} matches"),
        Expectation::AtLine(line) => format!("a match starting at line {line}"),
    }
}

fn at_lines(found: usize, found_lines: &[usize]) -> String {
    let listed: Vec<String> = found_lines.iter().map(ToString::to_string).collect();
    let more = if found > found_lines.len() {
        ", and more"
    } else {
        ""
    };
    match listed.as_slice() {
        [] => String::new(),
        [only] => format!(" at line {only}{more}"),
        _ => format!(" at lines {}{more}", listed.join(", ")),
    }
}

fn near_miss(near: &NearMiss) -> String {
    match near {
        NearMiss::Differs {
            file_line,
            expected,
            found,
        } => {
            let found = found
                .as_ref()
                .map_or_else(|| "<end of file>".to_owned(), |text| format!("{text:?}"));
            format!(
                "  closest: file line {file_line} differs from the hunk\n    hunk: {expected:?}\n    file: {found}\n"
            )
        }
        NearMiss::Whitespace { file_line } => {
            format!(
                "  closest: file line {file_line} matches the hunk's first line except for surrounding whitespace\n"
            )
        }
        NearMiss::AfterElision { file_line } => format!(
            "  closest: the lines above `~` match from file line {file_line}, and the lines below it are not found after them\n"
        ),
    }
}

#[must_use]
pub fn usage_fault(fault: &UsageFault) -> String {
    let detail = match fault {
        UsageFault::UnknownArgument(argument) => format!("unknown argument `{argument}`"),
        UsageFault::MissingValue(flag) => format!("`{flag}` needs a value"),
        UsageFault::MissingProgram => "`try` needs a command after `--`".to_owned(),
        UsageFault::DryRunUnderTry => {
            "`--dry-run` does not combine with `try`, which runs the command".to_owned()
        }
        UsageFault::ExpectFailureOutsideTry => "`--expect-fail` belongs to `try`".to_owned(),
    };
    format!("splice: {detail}; `splice --help` prints the grammar\n")
}

#[must_use]
pub fn script_unreadable(detail: &str) -> String {
    format!("splice: could not read the script: {detail}\n")
}

#[must_use]
pub fn io_failure(path: &str, error: &std::io::Error) -> String {
    format!("splice: {path}: {error}\n")
}

/// The files a failed batch wrote before the failure, the one it failed on, and those it
/// never reached, so the state of the tree is stated rather than guessed.
#[must_use]
pub fn write_failure(
    written: &[&str],
    failed: &str,
    error: &std::io::Error,
    unwritten: &[&str],
) -> String {
    let mut message = format!("splice: {failed}: could not write: {error}\n");
    if !written.is_empty() {
        message.push_str("splice: already written: ");
        message.push_str(&written.join(", "));
        message.push('\n');
    }
    if !unwritten.is_empty() {
        message.push_str("splice: not written: ");
        message.push_str(&unwritten.join(", "));
        message.push('\n');
    }
    message
}

#[must_use]
pub fn readback_mismatch(path: &str) -> String {
    format!(
        "splice: {path}: the file read back differs from what was written; another process may be writing it\n"
    )
}

#[must_use]
pub fn restore_failure(path: &str, error: &std::io::Error) -> String {
    format!(
        "splice: {path}: could not restore the original, and the edit is still applied: {error}\n"
    )
}

#[must_use]
pub fn program_failure(program: &str, error: &std::io::Error) -> String {
    format!("splice: could not run `{program}`: {error}; the files are restored\n")
}

#[must_use]
pub fn hook_input_unreadable(detail: &str) -> String {
    format!("splice guard: could not read the hook input: {detail}\n")
}

#[must_use]
pub fn hook_input_fault(fault: &HookInputFault) -> String {
    let detail = match fault {
        HookInputFault::NotJson(message) => format!("it is not JSON: {message}"),
        HookInputFault::NoToolName => "it holds no `tool_name`".to_owned(),
        HookInputFault::NoCommand => "a Bash call holds no `tool_input.command`".to_owned(),
    };
    format!("splice guard: the hook input is not what Claude Code sends: {detail}\n")
}

/// What the agent reads when the hook blocks its command: what was blocked, and the
/// whole of what it needs to redo the edit with splice.
#[must_use]
pub fn guard_denial(offense: Offense) -> String {
    let blocked = match offense {
        Offense::SedInPlace => "`sed -i`",
        Offense::PerlInPlace => "`perl -i`",
        Offense::AwkInPlace => "`awk -i inplace`",
        Offense::PythonWrite => "a Python script writing a file",
        Offense::NodeWrite => "a Node script writing a file",
        Offense::MoveOverOriginal => "a rewrite into a second file moved over the original",
        Offense::Sponge => "`sponge`",
    };
    format!(
        "splice guard: blocked an edit through {blocked}.\n\
         Edit files with splice: it matches literal text, checks each hunk's match count, \
         and writes every file or none.\n\
         \n\
         \x20 splice <<'EOF' && <the check you would chain>\n\
         \x20 === path/to/file\n\
         \x20 @@\n\
         \x20  context line\n\
         \x20 -removed line\n\
         \x20 +added line\n\
         \x20 EOF\n\
         \n\
         `splice --help` prints the whole grammar: counts, line anchors, `~` for a run of \
         lines, regex, append, create, and `splice try` for an edit that is restored after \
         a command runs.\n"
    )
}

#[cfg(test)]
mod tests;
