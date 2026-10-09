//! The unified diff `--diff` and `--dry-run` print, in the form `git diff` prints.

use similar::TextDiff;

/// Lines of unchanged context around each change.
const CONTEXT_LINES: usize = 3;

/// The diff from `before` to `after` for `path`; `before` is `None` for a file being
/// created, which the header names as `/dev/null`.
///
/// # Panics
///
/// Panics when the texts differ and the diff renders empty, which `similar` never does.
#[must_use]
pub fn unified(path: &str, before: Option<&str>, after: &str) -> String {
    let old_header = if before.is_some() {
        format!("a/{path}")
    } else {
        "/dev/null".to_owned()
    };
    let new_header = format!("b/{path}");
    let diff = TextDiff::from_lines(before.unwrap_or_default(), after);
    let mut unified = diff.unified_diff();
    let rendered = unified
        .context_radius(CONTEXT_LINES)
        .header(&old_header, &new_header)
        .to_string();
    assert!(
        before == Some(after) || !rendered.is_empty(),
        "a changed file renders a non-empty diff"
    );
    rendered
}

#[cfg(test)]
mod tests;
