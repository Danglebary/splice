//! The edit script's grammar. Everything here is pure, text in and a typed script or the
//! first fault out, so every rule of the grammar is tested without touching a file.

use std::collections::HashMap;
use std::num::NonZeroUsize;

use regex::Regex;

/// A parsed script: the files it edits, in the order it names them, each with its hunks.
#[derive(Debug, PartialEq, Eq)]
pub struct Script {
    pub sections: Vec<Section>,
}

/// The paths named by consecutive `===` lines and the hunks applied to each of them.
#[derive(Debug, PartialEq, Eq)]
pub struct Section {
    pub paths: Vec<String>,
    pub hunks: Vec<Hunk>,
}

/// One `@@` hunk. `line` is the one-based script line of its header, which names the
/// hunk in every message about it.
#[derive(Debug, PartialEq, Eq)]
pub struct Hunk {
    pub line: usize,
    pub operation: Operation,
}

/// What a hunk does to its file.
#[derive(Debug, PartialEq, Eq)]
pub enum Operation {
    /// Matches the block's context and removed lines literally, line by line.
    Literal {
        block: Vec<BlockLine>,
        expectation: Expectation,
    },
    /// Matches `pattern` over the whole text and replaces each match with `replacement`,
    /// whose `$1` and `${name}` expand to the match's groups.
    Regex {
        pattern: Pattern,
        replacement: String,
        expectation: Expectation,
    },
    /// Matches the `old` lines, joined at the file's own line break, as text anywhere in
    /// the file, mid-line included, and replaces each match with the `new` lines joined
    /// the same way.
    Inline {
        old: Vec<String>,
        new: Vec<String>,
        expectation: Expectation,
    },
    /// Adds the lines at the end of an existing file.
    Append { lines: Vec<String> },
    /// Writes a file that does not exist yet, holding the lines.
    Create { lines: Vec<String> },
}

/// A regex hunk's pattern, compiled once as the script is parsed. Two patterns are equal
/// when their source text is.
#[derive(Debug)]
pub struct Pattern {
    regex: Regex,
}

impl Pattern {
    /// Compiles `source` under the regex crate's default options.
    ///
    /// # Errors
    ///
    /// Returns the compiler's error when `source` is not a valid pattern.
    ///
    /// # Panics
    ///
    /// Panics when the compiled pattern does not hold `source` as its text, which the
    /// regex crate never does.
    pub fn compile(source: &str) -> Result<Self, regex::Error> {
        let regex = Regex::new(source)?;
        assert_eq!(
            regex.as_str(),
            source,
            "a compiled pattern keeps its source text, which equality compares"
        );
        Ok(Self { regex })
    }

    /// The compiled pattern.
    #[must_use]
    pub const fn regex(&self) -> &Regex {
        &self.regex
    }
}

impl PartialEq for Pattern {
    // Every pattern compiles from its source under the same default options, so equal
    // source text means the two match alike.
    fn eq(&self, other: &Self) -> bool {
        self.regex.as_str() == other.regex.as_str()
    }
}

impl Eq for Pattern {}

/// One line of a literal hunk's block.
#[derive(Debug, PartialEq, Eq)]
pub enum BlockLine {
    Context(String),
    Remove(String),
    Add(String),
    /// A `~` under a context line: the lines it stands for are matched and kept.
    ElideKeep,
    /// A `~` under a removed line: the lines it stands for are matched and removed.
    ElideRemove,
}

/// How many matches a hunk declares, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expectation {
    Once,
    All,
    Exactly(NonZeroUsize),
    AtLine(NonZeroUsize),
}

/// The first fault in a script, at the one-based line it was found on.
#[derive(Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub line: usize,
    pub fault: Fault,
}

/// Every way a script breaks the grammar.
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    EmptyScript,
    LineBeforeFirstFile,
    LineBeforeFirstHunk,
    EmptyPath,
    PathRepeated { first_line: usize },
    FileWithoutHunks,
    UnknownHeader { header: String },
    InvalidNumber { word: String },
    UnexpectedLine,
    NoAnchor,
    ChangesNothing,
    ElisionWithoutLineAbove,
    ElisionWithoutLineBelow,
    RegexWithoutPattern,
    RegexWithContext,
    RegexInvalid { message: String },
    InlineWithoutText,
    InlineWithContext,
    OnlyAddedLines,
    AppendEmpty,
    JsonlInvalid { message: String },
    CreateNotAlone,
}

/// Parses a whole script.
///
/// # Errors
///
/// Returns the first fault, at the line it was found on.
pub fn parse(text: &str) -> Result<Script, ScriptError> {
    let mut sections: Vec<SectionDraft> = Vec::new();
    let mut draft: Option<HunkDraft> = None;
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (index, raw) in text.lines().enumerate() {
        let Some(line) = index.checked_add(1) else {
            unreachable!("a script's line count fits a usize");
        };
        match classify(raw) {
            Item::File(path) => {
                finish(&mut sections, draft.take())?;
                add_path(&mut sections, &mut seen, path, line)?;
            }
            Item::Header(words) => {
                if sections.is_empty() {
                    return Err(ScriptError {
                        line,
                        fault: Fault::LineBeforeFirstFile,
                    });
                }
                finish(&mut sections, draft.take())?;
                let header = parse_header(words).map_err(|fault| ScriptError { line, fault })?;
                draft = Some(HunkDraft {
                    line,
                    header,
                    body: Vec::new(),
                });
            }
            Item::Blank => {
                if let Some(open) = draft.as_mut() {
                    open.body.push((line, Entry::Blank));
                }
            }
            Item::Body(entry) => match draft.as_mut() {
                Some(open) => open.body.push((line, entry)),
                None => {
                    return Err(ScriptError {
                        line,
                        fault: outside_hunk(&sections),
                    });
                }
            },
            Item::Unexpected => {
                let fault = if draft.is_some() {
                    Fault::UnexpectedLine
                } else {
                    outside_hunk(&sections)
                };
                return Err(ScriptError { line, fault });
            }
        }
    }
    finish(&mut sections, draft.take())?;
    close(sections)
}

/// A line of script text, classified without context: what it is never depends on the
/// lines around it, which is what lets content hold any text once it carries a marker.
enum Item<'t> {
    Blank,
    File(&'t str),
    Header(&'t str),
    Body(Entry<'t>),
    Unexpected,
}

/// A line inside a hunk's body, before the header decides what it means.
#[derive(Clone, Copy)]
enum Entry<'t> {
    Blank,
    Context(&'t str),
    Remove(&'t str),
    Add(&'t str),
    Elision,
}

/// What a hunk's header declares, before its body is read.
enum Header {
    Literal(Expectation),
    Regex(Expectation),
    Inline(Expectation),
    Append { jsonl: bool },
    Create,
}

struct HunkDraft<'t> {
    line: usize,
    header: Header,
    body: Vec<(usize, Entry<'t>)>,
}

struct SectionDraft {
    first_line: usize,
    paths: Vec<String>,
    hunks: Vec<Hunk>,
}

fn classify(line: &str) -> Item<'_> {
    if line.is_empty() {
        return Item::Blank;
    }
    if line == "===" {
        return Item::File("");
    }
    if let Some(path) = line.strip_prefix("=== ") {
        return Item::File(path);
    }
    if line == "@@" {
        return Item::Header("");
    }
    if let Some(words) = line.strip_prefix("@@ ") {
        return Item::Header(words);
    }
    if line.trim_end() == "~" {
        return Item::Body(Entry::Elision);
    }
    let mut characters = line.chars();
    let marker = characters.next();
    let text = characters.as_str();
    match marker {
        Some(' ') => Item::Body(Entry::Context(text)),
        Some('-') => Item::Body(Entry::Remove(text)),
        Some('+') => Item::Body(Entry::Add(text)),
        _ => Item::Unexpected,
    }
}

const fn outside_hunk(sections: &[SectionDraft]) -> Fault {
    if sections.is_empty() {
        Fault::LineBeforeFirstFile
    } else {
        Fault::LineBeforeFirstHunk
    }
}

fn add_path(
    sections: &mut Vec<SectionDraft>,
    seen: &mut HashMap<String, usize>,
    raw: &str,
    line: usize,
) -> Result<(), ScriptError> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(ScriptError {
            line,
            fault: Fault::EmptyPath,
        });
    }
    if let Some(first_line) = seen.get(path) {
        return Err(ScriptError {
            line,
            fault: Fault::PathRepeated {
                first_line: *first_line,
            },
        });
    }
    seen.insert(path.to_owned(), line);
    match sections.last_mut() {
        Some(group) if group.hunks.is_empty() => group.paths.push(path.to_owned()),
        _ => sections.push(SectionDraft {
            first_line: line,
            paths: vec![path.to_owned()],
            hunks: Vec::new(),
        }),
    }
    assert!(
        seen.contains_key(path),
        "a named path is remembered for the repeat check"
    );
    Ok(())
}

fn finish(sections: &mut [SectionDraft], draft: Option<HunkDraft<'_>>) -> Result<(), ScriptError> {
    let Some(draft) = draft else {
        return Ok(());
    };
    let Some(section) = sections.last_mut() else {
        unreachable!("a hunk opens only under a file header");
    };
    let hunk = build(draft)?;
    let earlier_create = section
        .hunks
        .iter()
        .find(|held| matches!(held.operation, Operation::Create { .. }));
    if let Some(create) = earlier_create {
        return Err(ScriptError {
            line: create.line,
            fault: Fault::CreateNotAlone,
        });
    }
    if matches!(hunk.operation, Operation::Create { .. }) {
        if !section.hunks.is_empty() {
            return Err(ScriptError {
                line: hunk.line,
                fault: Fault::CreateNotAlone,
            });
        }
    }
    section.hunks.push(hunk);
    Ok(())
}

fn close(sections: Vec<SectionDraft>) -> Result<Script, ScriptError> {
    if let Some(last) = sections.last() {
        if last.hunks.is_empty() {
            return Err(ScriptError {
                line: last.first_line,
                fault: Fault::FileWithoutHunks,
            });
        }
    } else {
        return Err(ScriptError {
            line: 1,
            fault: Fault::EmptyScript,
        });
    }
    let script = Script {
        sections: sections
            .into_iter()
            .map(|draft| Section {
                paths: draft.paths,
                hunks: draft.hunks,
            })
            .collect(),
    };
    assert!(
        script
            .sections
            .iter()
            .all(|section| !section.paths.is_empty()),
        "every section names a path"
    );
    assert!(
        script
            .sections
            .iter()
            .all(|section| !section.hunks.is_empty()),
        "every section holds a hunk"
    );
    Ok(script)
}

fn parse_header(text: &str) -> Result<Header, Fault> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [] => Ok(Header::Literal(Expectation::Once)),
        ["all"] => Ok(Header::Literal(Expectation::All)),
        ["count", word] => Ok(Header::Literal(count(word)?)),
        ["line", word] => Ok(Header::Literal(Expectation::AtLine(number(word)?))),
        ["regex"] => Ok(Header::Regex(Expectation::Once)),
        ["regex", "all"] => Ok(Header::Regex(Expectation::All)),
        ["regex", "count", word] => Ok(Header::Regex(count(word)?)),
        ["inline"] => Ok(Header::Inline(Expectation::Once)),
        ["inline", "all"] => Ok(Header::Inline(Expectation::All)),
        ["inline", "count", word] => Ok(Header::Inline(count(word)?)),
        ["append"] => Ok(Header::Append { jsonl: false }),
        ["append", "jsonl"] => Ok(Header::Append { jsonl: true }),
        ["create"] => Ok(Header::Create),
        _ => Err(Fault::UnknownHeader {
            header: text.trim().to_owned(),
        }),
    }
}

fn number(word: &str) -> Result<NonZeroUsize, Fault> {
    let parsed = word.parse::<usize>().ok().and_then(NonZeroUsize::new);
    parsed.ok_or_else(|| Fault::InvalidNumber {
        word: word.to_owned(),
    })
}

fn count(word: &str) -> Result<Expectation, Fault> {
    let declared = number(word)?;
    if declared.get() == 1 {
        Ok(Expectation::Once)
    } else {
        Ok(Expectation::Exactly(declared))
    }
}

fn build(draft: HunkDraft<'_>) -> Result<Hunk, ScriptError> {
    let mut body: Vec<(usize, Entry<'_>)> = draft
        .body
        .into_iter()
        .skip_while(|(_, entry)| matches!(entry, Entry::Blank))
        .collect();
    while matches!(body.last(), Some((_, Entry::Blank))) {
        body.pop();
    }
    let operation = match draft.header {
        Header::Literal(expectation) => build_literal(draft.line, &body, expectation)?,
        Header::Regex(expectation) => build_regex(draft.line, &body, expectation)?,
        Header::Inline(expectation) => build_inline(draft.line, &body, expectation)?,
        Header::Append { jsonl } => {
            let lines = build_added(&body)?;
            if lines.is_empty() {
                return Err(ScriptError {
                    line: draft.line,
                    fault: Fault::AppendEmpty,
                });
            }
            if jsonl {
                check_jsonl(&body)?;
            }
            Operation::Append { lines }
        }
        Header::Create => Operation::Create {
            lines: build_added(&body)?,
        },
    };
    Ok(Hunk {
        line: draft.line,
        operation,
    })
}

fn build_literal(
    header_line: usize,
    body: &[(usize, Entry<'_>)],
    expectation: Expectation,
) -> Result<Operation, ScriptError> {
    let mut block = Vec::with_capacity(body.len());
    for (position, (line, entry)) in body.iter().enumerate() {
        let next = match entry {
            Entry::Blank => BlockLine::Context(String::new()),
            Entry::Context(text) => BlockLine::Context((*text).to_owned()),
            Entry::Remove(text) => BlockLine::Remove((*text).to_owned()),
            Entry::Add(text) => BlockLine::Add((*text).to_owned()),
            Entry::Elision => elision(&block, body.iter().skip(position).skip(1), *line)?,
        };
        block.push(next);
    }
    let anchored = block
        .iter()
        .any(|line| matches!(line, BlockLine::Context(_) | BlockLine::Remove(_)));
    if !anchored {
        return Err(ScriptError {
            line: header_line,
            fault: Fault::NoAnchor,
        });
    }
    let changes = block.iter().any(|line| {
        matches!(
            line,
            BlockLine::Remove(_) | BlockLine::Add(_) | BlockLine::ElideRemove
        )
    });
    if !changes {
        return Err(ScriptError {
            line: header_line,
            fault: Fault::ChangesNothing,
        });
    }
    Ok(Operation::Literal { block, expectation })
}

/// An elision takes the role of the line directly above it, and needs a matched line
/// somewhere below it to end the lines it stands for.
fn elision<'b, 't: 'b>(
    above: &[BlockLine],
    mut below: impl Iterator<Item = &'b (usize, Entry<'t>)>,
    line: usize,
) -> Result<BlockLine, ScriptError> {
    let role = match above.last() {
        Some(BlockLine::Context(_)) => BlockLine::ElideKeep,
        Some(BlockLine::Remove(_)) => BlockLine::ElideRemove,
        _ => {
            return Err(ScriptError {
                line,
                fault: Fault::ElisionWithoutLineAbove,
            });
        }
    };
    let ending = below.find(|(_, entry)| !matches!(entry, Entry::Add(_)));
    match ending {
        Some((_, Entry::Blank | Entry::Context(_) | Entry::Remove(_))) => Ok(role),
        _ => Err(ScriptError {
            line,
            fault: Fault::ElisionWithoutLineBelow,
        }),
    }
}

fn build_regex(
    header_line: usize,
    body: &[(usize, Entry<'_>)],
    expectation: Expectation,
) -> Result<Operation, ScriptError> {
    let mut pattern_lines = Vec::new();
    let mut replacement_lines = Vec::new();
    for (line, entry) in body {
        match entry {
            Entry::Remove(text) => pattern_lines.push(*text),
            Entry::Add(text) => replacement_lines.push(*text),
            Entry::Blank | Entry::Context(_) | Entry::Elision => {
                return Err(ScriptError {
                    line: *line,
                    fault: Fault::RegexWithContext,
                });
            }
        }
    }
    if pattern_lines.is_empty() {
        return Err(ScriptError {
            line: header_line,
            fault: Fault::RegexWithoutPattern,
        });
    }
    let source = pattern_lines.join("\n");
    let pattern = match Pattern::compile(&source) {
        Ok(pattern) => pattern,
        Err(error) => {
            return Err(ScriptError {
                line: header_line,
                fault: Fault::RegexInvalid {
                    message: error.to_string(),
                },
            });
        }
    };
    let replacement = replacement_lines.join("\n");
    Ok(Operation::Regex {
        pattern,
        replacement,
        expectation,
    })
}

fn build_inline(
    header_line: usize,
    body: &[(usize, Entry<'_>)],
    expectation: Expectation,
) -> Result<Operation, ScriptError> {
    let mut old = Vec::new();
    let mut new = Vec::new();
    for (line, entry) in body {
        match entry {
            Entry::Remove(text) => old.push((*text).to_owned()),
            Entry::Add(text) => new.push((*text).to_owned()),
            Entry::Blank | Entry::Context(_) | Entry::Elision => {
                return Err(ScriptError {
                    line: *line,
                    fault: Fault::InlineWithContext,
                });
            }
        }
    }
    if old.join("\n").is_empty() {
        return Err(ScriptError {
            line: header_line,
            fault: Fault::InlineWithoutText,
        });
    }
    if old == new {
        return Err(ScriptError {
            line: header_line,
            fault: Fault::ChangesNothing,
        });
    }
    assert!(!old.is_empty(), "an inline hunk holds text to match");
    Ok(Operation::Inline {
        old,
        new,
        expectation,
    })
}

fn build_added(body: &[(usize, Entry<'_>)]) -> Result<Vec<String>, ScriptError> {
    let mut lines = Vec::with_capacity(body.len());
    for (line, entry) in body {
        match entry {
            Entry::Add(text) => lines.push((*text).to_owned()),
            Entry::Blank | Entry::Context(_) | Entry::Remove(_) | Entry::Elision => {
                return Err(ScriptError {
                    line: *line,
                    fault: Fault::OnlyAddedLines,
                });
            }
        }
    }
    assert_eq!(
        lines.len(),
        body.len(),
        "every body line of an added-only hunk is an added line"
    );
    Ok(lines)
}

fn check_jsonl(body: &[(usize, Entry<'_>)]) -> Result<(), ScriptError> {
    for (line, entry) in body {
        let Entry::Add(text) = entry else {
            unreachable!("an append's body holds added lines alone once it is built");
        };
        if let Err(error) = serde_json::from_str::<serde_json::Value>(text) {
            return Err(ScriptError {
                line: *line,
                fault: Fault::JsonlInvalid {
                    message: error.to_string(),
                },
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
