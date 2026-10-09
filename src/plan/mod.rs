//! Matching and splicing for one file, all of it pure.
//!
//! The file's text and its hunks go in, and the new text or every refusal comes out, so
//! each matching rule is tested against a string. Every hunk matches against the text as
//! it was before any hunk applied, which is what keeps a line number valid across a
//! batch.

use std::ops::Range;

use regex::Regex;

use crate::script::{BlockLine, Expectation, Hunk, Operation};

/// How many match lines a refusal names; the count beside them stays exact.
pub const FOUND_LINES_SHOWN_MAX: usize = 10;

/// One hunk that cannot apply as declared, named by its header's script line.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal {
    pub hunk_line: usize,
    pub reason: Reason,
}

/// Every reason a file's hunks are refused. `NotText`, `TooLarge`, and `SameFileAs` are
/// decided where the file is read; the rest are decided here.
#[derive(Debug, PartialEq, Eq)]
pub enum Reason {
    Missing,
    Exists,
    NotText,
    TooLarge {
        bytes: u64,
    },
    SameFileAs {
        path: String,
    },
    Count {
        expected: Expectation,
        found: usize,
        found_lines: Vec<usize>,
        near_miss: Option<NearMiss>,
    },
    Overlap {
        other_hunk_line: usize,
    },
    Unchanged,
}

/// The closest the file came to a hunk that matched nowhere, so the next attempt can be
/// written from the file's own text.
#[derive(Debug, PartialEq, Eq)]
pub enum NearMiss {
    /// The hunk's leading lines match from some line, and this is the first that does not.
    Differs {
        file_line: usize,
        expected: String,
        found: Option<String>,
    },
    /// No line matches exactly, and this one matches the hunk's first line once
    /// surrounding whitespace is ignored.
    Whitespace { file_line: usize },
    /// The lines above the first elision match from this line, and the lines below it are
    /// found nowhere after them.
    AfterElision { file_line: usize },
    /// The longest start of an inline hunk's text the file holds runs up to this line,
    /// and `expected` and `found` are the two texts from where they part.
    Diverges {
        file_line: usize,
        expected: String,
        found: String,
    },
}

/// The fewest characters an inline hunk and the file share before a divergence is named:
/// a shorter shared start occurs almost anywhere and points nowhere useful.
const NEAR_MISS_PREFIX_CHARS_MIN: usize = 8;

/// How many characters of each side a diverging near miss shows.
const NEAR_MISS_SNIPPET_CHARS_MAX: usize = 40;

/// Applies every hunk to `original`, or `None` for a file that does not exist.
///
/// # Errors
///
/// Returns every refusal when any hunk cannot apply as declared; nothing is applied then.
///
/// # Panics
///
/// Panics when `hunks` is empty, which no parsed script produces.
pub fn plan(original: Option<&str>, hunks: &[Hunk]) -> Result<String, Vec<Refusal>> {
    assert!(
        !hunks.is_empty(),
        "a file is planned with at least one hunk"
    );
    let Some(text) = original else {
        return plan_missing(hunks);
    };
    let document = Document::new(text);
    let mut edits: Vec<Edit> = Vec::new();
    let mut appended: Vec<&str> = Vec::new();
    let mut refusals: Vec<Refusal> = Vec::new();
    for hunk in hunks {
        let outcome = match &hunk.operation {
            Operation::Create { .. } => Err(Reason::Exists),
            Operation::Append { lines } => {
                appended.extend(lines.iter().map(String::as_str));
                Ok(Vec::new())
            }
            Operation::Literal { block, expectation } => {
                literal_edits(&document, block, *expectation)
            }
            Operation::Regex {
                pattern,
                replacement,
                expectation,
            } => regex_edits(&document, pattern, replacement, *expectation),
            Operation::Inline {
                old,
                new,
                expectation,
            } => inline_edits(&document, old, new, *expectation),
        };
        match outcome {
            Ok(replacements) if unchanged(text, &replacements) => {
                refusals.push(Refusal {
                    hunk_line: hunk.line,
                    reason: Reason::Unchanged,
                });
            }
            Ok(replacements) => {
                let tagged = replacements.into_iter().map(|replacement| Edit {
                    hunk_line: hunk.line,
                    replacement,
                });
                edits.extend(tagged);
            }
            Err(reason) => refusals.push(Refusal {
                hunk_line: hunk.line,
                reason,
            }),
        }
    }
    edits.sort_by_key(|edit| {
        (
            edit.replacement.range.start,
            edit.replacement.range.end,
            edit.hunk_line,
        )
    });
    refusals.extend(overlaps(&edits));
    if !refusals.is_empty() {
        return Err(refusals);
    }
    let planned = assemble(&document, &edits, &appended);
    assert_ne!(planned, text, "a plan with no refusal changes the text");
    Ok(planned)
}

/// A file's text split into lines, each line's terminator kept apart from its content.
struct Document<'t> {
    text: &'t str,
    lines: Vec<LineSpan>,
    eol: &'static str,
}

#[derive(Clone, Copy)]
struct LineSpan {
    start: usize,
    content_end: usize,
    end: usize,
}

/// A byte range of the original text and what replaces it.
struct Replacement {
    range: Range<usize>,
    text: String,
}

struct Edit {
    hunk_line: usize,
    replacement: Replacement,
}

impl<'t> Document<'t> {
    /// A file is CRLF when every line ending in it is CRLF; a file mixing the two is LF,
    /// its carriage returns kept as content, so each line is written back as it was.
    fn new(text: &'t str) -> Self {
        let newline_count = text.matches('\n').count();
        let crlf = newline_count > 0 && text.matches("\r\n").count() == newline_count;
        let eol = if crlf { "\r\n" } else { "\n" };
        let mut lines = Vec::with_capacity(newline_count);
        let mut start = 0;
        for (index, _) in text.match_indices('\n') {
            let content_end = if crlf { before(index) } else { index };
            let end = after(index);
            lines.push(LineSpan {
                start,
                content_end,
                end,
            });
            start = end;
        }
        if start < text.len() {
            lines.push(LineSpan {
                start,
                content_end: text.len(),
                end: text.len(),
            });
        }
        assert!(
            lines.last().is_none_or(|last| last.end == text.len()),
            "the lines cover the whole text"
        );
        Self { text, lines, eol }
    }

    fn content(&self, index: usize) -> Option<&'t str> {
        let span = self.lines.get(index)?;
        let content = self.text.get(span.start..span.content_end);
        assert!(
            content.is_some(),
            "a line's content ends before its terminator, on a character boundary"
        );
        content
    }

    const fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The one-based number of the line holding `byte`; the end of the text counts as
    /// the last line.
    fn line_number_of_byte(&self, byte: usize) -> usize {
        assert!(byte <= self.text.len(), "a match starts inside the text");
        let index = self.lines.partition_point(|span| span.end <= byte);
        let last = self.lines.len().saturating_sub(1);
        after(index.min(last))
    }

    fn span(&self, index: usize) -> LineSpan {
        let Some(span) = self.lines.get(index) else {
            unreachable!("a matched line index lies inside the document");
        };
        *span
    }

    fn lacks_final_newline(&self) -> bool {
        !self.text.is_empty() && !self.text.ends_with('\n')
    }
}

fn after(index: usize) -> usize {
    let Some(next) = index.checked_add(1) else {
        unreachable!("an index into a text is below usize::MAX");
    };
    next
}

fn before(index: usize) -> usize {
    let Some(previous) = index.checked_sub(1) else {
        unreachable!("a CRLF newline has its carriage return before it");
    };
    previous
}

fn offset(start: usize, by: usize) -> usize {
    let Some(sum) = start.checked_add(by) else {
        unreachable!("an index into a text plus a length inside it fits a usize");
    };
    sum
}

fn plan_missing(hunks: &[Hunk]) -> Result<String, Vec<Refusal>> {
    match hunks {
        [
            Hunk {
                operation: Operation::Create { lines },
                ..
            },
        ] => {
            let mut text = String::new();
            for line in lines {
                text.push_str(line);
                text.push('\n');
            }
            Ok(text)
        }
        [first, ..] => Err(vec![Refusal {
            hunk_line: first.line,
            reason: Reason::Missing,
        }]),
        [] => unreachable!("a file is planned with at least one hunk"),
    }
}

fn unchanged(text: &str, replacements: &[Replacement]) -> bool {
    !replacements.is_empty()
        && replacements.iter().all(|replacement| {
            text.get(replacement.range.clone()) == Some(replacement.text.as_str())
        })
}

/// Edits arrive sorted by where they start; an edit starting inside the one before it
/// claims text that edit already claimed.
fn overlaps(edits: &[Edit]) -> Vec<Refusal> {
    let mut refusals = Vec::new();
    for pair in edits.windows(2) {
        let [previous, next] = pair else {
            unreachable!("a window of two holds two edits");
        };
        if next.replacement.range.start < previous.replacement.range.end {
            let reason = Reason::Overlap {
                other_hunk_line: previous.hunk_line,
            };
            refusals.push(Refusal {
                hunk_line: next.hunk_line,
                reason,
            });
        }
    }
    refusals
}

fn assemble(document: &Document<'_>, edits: &[Edit], appended: &[&str]) -> String {
    let text = document.text;
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for edit in edits {
        let range = &edit.replacement.range;
        assert!(cursor <= range.start, "edits are sorted and do not overlap");
        let Some(kept) = text.get(cursor..range.start) else {
            unreachable!("an edit's range lies on character boundaries inside the text");
        };
        output.push_str(kept);
        output.push_str(&edit.replacement.text);
        cursor = range.end;
    }
    let Some(rest) = text.get(cursor..) else {
        unreachable!("the last edit ends inside the text");
    };
    output.push_str(rest);
    if !appended.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push_str(document.eol);
        }
        for line in appended {
            output.push_str(line);
            output.push_str(document.eol);
        }
    }
    if document.lacks_final_newline() {
        if let Some(kept) = output.strip_suffix(document.eol) {
            output.truncate(kept.len());
        }
    }
    output
}

/// The block's matched lines, split at each elision.
fn segments(block: &[BlockLine]) -> Vec<Vec<&str>> {
    let mut segments: Vec<Vec<&str>> = vec![Vec::new()];
    for line in block {
        match line {
            BlockLine::Context(text) | BlockLine::Remove(text) => {
                let Some(current) = segments.last_mut() else {
                    unreachable!("segments start with one and only grow");
                };
                current.push(text);
            }
            BlockLine::ElideKeep | BlockLine::ElideRemove => segments.push(Vec::new()),
            BlockLine::Add(_) => {}
        }
    }
    assert!(
        segments.iter().all(|segment| !segment.is_empty()),
        "the parser puts a matched line on both sides of an elision"
    );
    segments
}

fn segment_at(document: &Document<'_>, segment: &[&str], start: usize) -> bool {
    segment
        .iter()
        .enumerate()
        .all(|(position, expected)| document.content(offset(start, position)) == Some(*expected))
}

/// The line ranges of each segment when the block matches from `start`; each elision
/// ends at the first line its next segment matches from.
fn match_at(
    document: &Document<'_>,
    segments: &[Vec<&str>],
    start: usize,
) -> Option<Vec<Range<usize>>> {
    let mut ranges = Vec::with_capacity(segments.len());
    let mut position = start;
    for (index, segment) in segments.iter().enumerate() {
        let found = if index == 0 {
            segment_at(document, segment, position).then_some(position)
        } else {
            (position..document.line_count())
                .find(|candidate| segment_at(document, segment, *candidate))
        };
        let begin = found?;
        let end = offset(begin, segment.len());
        ranges.push(begin..end);
        position = end;
    }
    assert_eq!(ranges.len(), segments.len(), "a match places every segment");
    Some(ranges)
}

/// Picks the matches an expectation declares, each paired with its one-based line.
fn choose<T>(
    matches: Vec<(usize, T)>,
    expectation: Expectation,
) -> Result<Vec<T>, (usize, Vec<usize>)> {
    let found = matches.len();
    let satisfied = match expectation {
        Expectation::Once => found == 1,
        Expectation::All => found > 0,
        Expectation::Exactly(declared) => found == declared.get(),
        Expectation::AtLine(line) => matches.iter().any(|(at, _)| *at == line.get()),
    };
    if !satisfied {
        let found_lines = matches
            .iter()
            .map(|(line, _)| *line)
            .take(FOUND_LINES_SHOWN_MAX)
            .collect();
        return Err((found, found_lines));
    }
    let chosen: Vec<T> = match expectation {
        Expectation::AtLine(line) => matches
            .into_iter()
            .filter(|(at, _)| *at == line.get())
            .map(|(_, item)| item)
            .collect(),
        Expectation::Once | Expectation::All | Expectation::Exactly(_) => {
            matches.into_iter().map(|(_, item)| item).collect()
        }
    };
    assert!(
        !chosen.is_empty(),
        "a satisfied expectation chooses at least one match"
    );
    Ok(chosen)
}

fn literal_edits(
    document: &Document<'_>,
    block: &[BlockLine],
    expectation: Expectation,
) -> Result<Vec<Replacement>, Reason> {
    let segments = segments(block);
    let mut matches: Vec<(usize, Vec<Range<usize>>)> = Vec::new();
    let mut claimed_until = 0;
    for start in 0..document.line_count() {
        let Some(ranges) = match_at(document, &segments, start) else {
            continue;
        };
        let Some(end) = ranges.last().map(|range| range.end) else {
            unreachable!("a match holds a range per segment");
        };
        if matches!(expectation, Expectation::All) && start < claimed_until {
            continue;
        }
        claimed_until = end;
        matches.push((after(start), ranges));
    }
    match choose(matches, expectation) {
        Ok(chosen) => Ok(chosen
            .iter()
            .map(|ranges| literal_replacement(document, block, ranges))
            .collect()),
        Err((found, found_lines)) => {
            let near_miss = if found == 0 {
                near_miss(document, &segments)
            } else {
                None
            };
            Err(Reason::Count {
                expected: expectation,
                found,
                found_lines,
                near_miss,
            })
        }
    }
}

fn literal_replacement(
    document: &Document<'_>,
    block: &[BlockLine],
    ranges: &[Range<usize>],
) -> Replacement {
    let (Some(first), Some(last)) = (ranges.first(), ranges.last()) else {
        unreachable!("a match holds a range per segment");
    };
    let mut text = String::new();
    let mut cursor = first.start;
    let mut following = ranges.iter().skip(1);
    for line in block {
        match line {
            BlockLine::Context(_) => {
                push_line(&mut text, document, cursor);
                cursor = after(cursor);
            }
            BlockLine::Remove(_) => cursor = after(cursor),
            BlockLine::Add(added) => {
                text.push_str(added);
                text.push_str(document.eol);
            }
            BlockLine::ElideKeep | BlockLine::ElideRemove => {
                let Some(next) = following.next() else {
                    unreachable!("every elision is followed by a segment");
                };
                if matches!(line, BlockLine::ElideKeep) {
                    for index in cursor..next.start {
                        push_line(&mut text, document, index);
                    }
                }
                cursor = next.start;
            }
        }
    }
    assert_eq!(
        cursor, last.end,
        "the block walks exactly the matched lines"
    );
    let range = document.span(first.start).start..document.span(before(last.end)).end;
    Replacement { range, text }
}

fn push_line(text: &mut String, document: &Document<'_>, index: usize) {
    let Some(content) = document.content(index) else {
        unreachable!("a matched line lies inside the document");
    };
    text.push_str(content);
    text.push_str(document.eol);
}

fn regex_edits(
    document: &Document<'_>,
    pattern: &str,
    replacement: &str,
    expectation: Expectation,
) -> Result<Vec<Replacement>, Reason> {
    let Ok(regex) = Regex::new(pattern) else {
        unreachable!("the parser compiled this pattern");
    };
    let mut matches = Vec::new();
    for captures in regex.captures_iter(document.text) {
        let Some(whole) = captures.get(0) else {
            unreachable!("a match holds its whole span as group zero");
        };
        let mut expanded = String::new();
        captures.expand(replacement, &mut expanded);
        let line = document.line_number_of_byte(whole.start());
        matches.push((
            line,
            Replacement {
                range: whole.range(),
                text: expanded,
            },
        ));
    }
    choose(matches, expectation).map_err(|(found, found_lines)| Reason::Count {
        expected: expectation,
        found,
        found_lines,
        near_miss: None,
    })
}

/// Every occurrence of the joined text. A hunk declared `all` takes them left to right
/// without overlap; any other counts overlapping occurrences too, so text that matches
/// twice within one span is refused as ambiguous.
fn inline_edits(
    document: &Document<'_>,
    old: &[String],
    new: &[String],
    expectation: Expectation,
) -> Result<Vec<Replacement>, Reason> {
    let pattern = old.join(document.eol);
    let replacement = new.join(document.eol);
    let Some(first) = pattern.chars().next() else {
        unreachable!("the parser refuses an inline hunk without text");
    };
    let step = if matches!(expectation, Expectation::All) {
        pattern.len()
    } else {
        first.len_utf8()
    };
    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(found) = document
        .text
        .get(from..)
        .and_then(|rest| rest.find(pattern.as_str()))
    {
        let start = offset(from, found);
        let range = start..offset(start, pattern.len());
        let line = document.line_number_of_byte(start);
        let text = replacement.clone();
        matches.push((line, Replacement { range, text }));
        from = offset(start, step);
    }
    choose(matches, expectation).map_err(|(found, found_lines)| {
        let near_miss = if found == 0 {
            inline_near_miss(document, &pattern)
        } else {
            None
        };
        Reason::Count {
            expected: expectation,
            found,
            found_lines,
            near_miss,
        }
    })
}

/// The longest start of `pattern` the file holds, found by halving: a start that occurs
/// means every shorter start occurs too.
fn inline_near_miss(document: &Document<'_>, pattern: &str) -> Option<NearMiss> {
    let ends: Vec<usize> = pattern
        .char_indices()
        .map(|(index, character)| offset(index, character.len_utf8()))
        .collect();
    let prefix = |characters: usize| -> &str {
        let end = characters
            .checked_sub(1)
            .and_then(|last| ends.get(last))
            .copied()
            .unwrap_or_default();
        pattern.get(..end).unwrap_or_default()
    };
    let mut low = 0;
    let mut high = ends.len();
    while low < high {
        let middle = after(low.midpoint(high));
        if document.text.contains(prefix(middle)) {
            low = middle;
        } else {
            high = before(middle);
        }
    }
    assert!(low < ends.len(), "a pattern found whole is no near miss");
    if low < NEAR_MISS_PREFIX_CHARS_MIN.min(ends.len()) {
        return None;
    }
    let shared = prefix(low);
    let start = document.text.find(shared)?;
    let parted = offset(start, shared.len());
    let expected = pattern.get(shared.len()..)?;
    let found = document.text.get(parted..)?;
    Some(NearMiss::Diverges {
        file_line: document.line_number_of_byte(parted),
        expected: expected.chars().take(NEAR_MISS_SNIPPET_CHARS_MAX).collect(),
        found: found.chars().take(NEAR_MISS_SNIPPET_CHARS_MAX).collect(),
    })
}

fn near_miss(document: &Document<'_>, segments: &[Vec<&str>]) -> Option<NearMiss> {
    let leading = segments.first()?;
    let mut best: Option<(usize, usize)> = None;
    for start in 0..document.line_count() {
        let matched = leading
            .iter()
            .enumerate()
            .take_while(|(position, expected)| {
                document.content(offset(start, *position)) == Some(**expected)
            })
            .count();
        if matched > best.map_or(0, |(_, most)| most) {
            best = Some((start, matched));
        }
    }
    if let Some((start, matched)) = best {
        if matched == leading.len() {
            return Some(NearMiss::AfterElision {
                file_line: after(start),
            });
        }
        let differing = offset(start, matched);
        let Some(expected) = leading.get(matched) else {
            unreachable!("a partial match stops inside the segment");
        };
        let found = document.content(differing).map(str::to_owned);
        return Some(NearMiss::Differs {
            file_line: after(differing),
            expected: (*expected).to_owned(),
            found,
        });
    }
    let target = leading.first()?.trim();
    if target.is_empty() {
        return None;
    }
    let similar = (0..document.line_count()).find(|index| {
        document
            .content(*index)
            .is_some_and(|content| content.trim() == target)
    });
    similar.map(|index| NearMiss::Whitespace {
        file_line: after(index),
    })
}

#[cfg(test)]
mod tests;
