//! The hook's verdict on a Bash command: whether it edits a file in place through a
//! one-off script.
//!
//! Pure and deliberately narrow. It names the in-place editors and interpreter writes
//! and lets everything else through, so a command that reads, or saves output to a new
//! file, is never blocked. Quoted text, comments, and heredoc bodies are blanked before a
//! command's words are read, so a script or a message that only mentions an editor
//! passes; an interpreter's own code is the one place quoted text is read, since that is
//! where its file write is written, and a heredoc fed to a shell or written to a `.sh`
//! file is judged as a script of its own. A rewrite whose command line runs `jq` or `yq`
//! is let through: splice matches text, not JSON structure, so a structural JSON edit
//! stays theirs.

use std::sync::LazyLock;

use regex::Regex;

/// Whether the hook lets a command run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny(Offense),
}

/// The kind of one-off edit a denied command makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offense {
    SedInPlace,
    PerlInPlace,
    AwkInPlace,
    PythonWrite,
    NodeWrite,
    MoveOverOriginal,
    Sponge,
}

/// Words that run the command after them, wrappers and shell keywords alike, so the
/// program is read past them.
const COMMAND_PREFIXES: [&str; 16] = [
    "!", "builtin", "command", "do", "elif", "else", "env", "exec", "if", "nice", "nohup", "sudo",
    "then", "time", "until", "while",
];

/// Programs a heredoc is run by as a script.
const SHELLS: [&str; 4] = ["bash", "dash", "sh", "zsh"];

/// Programs that edit JSON or YAML by structure.
const STRUCTURED_EDITORS: [&str; 4] = ["gojq", "jaq", "jq", "yq"];

/// How many heredocs deep a script inside a script is read.
const SCRIPT_DEPTH_MAX: usize = 2;

/// Characters that end one simple command and start the next.
const COMMAND_SEPARATORS: [char; 9] = [';', '&', '|', '\n', '(', ')', '{', '}', '`'];

/// The `xargs` options that take the next word as their value.
const XARGS_OPTIONS_WITH_VALUE: [&str; 9] = ["-I", "-n", "-L", "-P", "-d", "-a", "-s", "-E", "-e"];

static PYTHON_WRITE: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = concat!(
        r#"open\([^)]*?,\s*(?:mode\s*=\s*)?['"][rbt]*[wax+]"#,
        r#"|\.open\(\s*['"][rbt]*[wax+]"#,
        r"|\.write_(?:text|bytes)\(",
        r"|\bos\.(?:replace|rename)\(",
        r"|\bshutil\.(?:move|copy|copy2|copyfile)\(",
        r"|\bfileinput\.(?:input|FileInput)\([^)]*inplace\s*=\s*True",
    );
    compiled(pattern)
});

static NODE_WRITE: LazyLock<Regex> = LazyLock::new(|| {
    compiled(
        r"\b(?:writeFileSync|writeFile|appendFileSync|appendFile|renameSync|copyFileSync|createWriteStream)\(",
    )
});

static REDIRECT_TARGET: LazyLock<Regex> =
    LazyLock::new(|| compiled(r"(?:^|[^0-9&>])>{1,2}\s*([^\s;&|<>()]+)"));

static SCRIPT_TARGET: LazyLock<Regex> = LazyLock::new(|| {
    compiled(r#"(?i)(?:^|[^0-9&>])>{1,2}\s*["']?[^\s;&|<>()"']*\.sh["']?(?:[\s;&|)]|$)"#)
});

fn compiled(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(error) => unreachable!("a pattern written in this file compiles: {error}"),
    }
}

/// What a `PreToolUse` hook input carries for the verdict.
#[derive(Debug, PartialEq, Eq)]
pub enum HookInput {
    Bash(String),
    OtherTool,
}

/// Every way a hook input fails to be what Claude Code sends.
#[derive(Debug, PartialEq, Eq)]
pub enum HookInputFault {
    NotJson(String),
    NoToolName,
    NoCommand,
}

/// Reads the tool and its command from a `PreToolUse` hook's stdin, whose shape is
/// `{"tool_name": "Bash", "tool_input": {"command": "..."}, ...}`.
/// <https://code.claude.com/docs/en/hooks> — checked 2026-10-08
///
/// # Errors
///
/// Returns the fault when the input is not JSON or lacks the fields a Bash call carries.
pub fn hook_input(input: &[u8]) -> Result<HookInput, HookInputFault> {
    let value: serde_json::Value = serde_json::from_slice(input)
        .map_err(|error| HookInputFault::NotJson(error.to_string()))?;
    let Some(tool) = value.get("tool_name").and_then(serde_json::Value::as_str) else {
        return Err(HookInputFault::NoToolName);
    };
    if tool != "Bash" {
        return Ok(HookInput::OtherTool);
    }
    let command = value
        .get("tool_input")
        .and_then(|tool_input| tool_input.get("command"));
    match command.and_then(serde_json::Value::as_str) {
        Some(command) => Ok(HookInput::Bash(command.to_owned())),
        None => Err(HookInputFault::NoCommand),
    }
}

/// Judges one Bash command as the hook receives it.
#[must_use]
pub fn verdict(command: &str) -> Verdict {
    judge(command, 0)
}

fn judge(command: &str, depth: usize) -> Verdict {
    assert!(
        depth <= SCRIPT_DEPTH_MAX,
        "a nested script is read to a bounded depth"
    );
    let lexed = lex(command);
    let commands = simple_commands(&lexed.outer);
    let structured = commands
        .iter()
        .any(|words| program(words).is_some_and(|(name, _)| STRUCTURED_EDITORS.contains(&name)));
    for words in &commands {
        match editor_offense(words, command) {
            Some(Offense::Sponge) if structured => {}
            Some(offense) => return Verdict::Deny(offense),
            None => {}
        }
    }
    if !structured && moves_over_original(&lexed.outer, &commands) {
        return Verdict::Deny(Offense::MoveOverOriginal);
    }
    if depth < SCRIPT_DEPTH_MAX {
        for heredoc in &lexed.heredocs {
            if !runs_as_script(&heredoc.opener) {
                continue;
            }
            if let Verdict::Deny(offense) = judge(&heredoc.body, after(depth)) {
                return Verdict::Deny(offense);
            }
        }
    }
    Verdict::Allow
}

fn after(depth: usize) -> usize {
    let Some(next) = depth.checked_add(1) else {
        unreachable!("the depth stays at or under its bound");
    };
    next
}

/// A heredoc is a script when a shell reads it or it is written to a `.sh` file. The
/// opener is read raw, since a script's path is often quoted.
fn runs_as_script(opener: &str) -> bool {
    let outer = lex(opener).outer;
    let commands = simple_commands(&outer);
    let fed_to_shell = commands
        .iter()
        .any(|words| program(words).is_some_and(|(name, _)| SHELLS.contains(&name)));
    fed_to_shell || SCRIPT_TARGET.is_match(opener)
}

/// A command split into the shell's own words and the heredoc bodies it carries.
struct Lexed {
    outer: String,
    heredocs: Vec<Heredoc>,
}

/// A heredoc body and the raw text of the line that opened it.
struct Heredoc {
    opener: String,
    body: String,
}

/// The command with every quoted string emptied, every comment dropped, and every
/// heredoc body lifted out beside the raw line that opened it, so `outer` holds the
/// shell's own words and keeps its line breaks.
fn lex(command: &str) -> Lexed {
    let mut outer = String::with_capacity(command.len());
    let mut line = String::new();
    let mut heredocs: Vec<Heredoc> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\'' | '"' => {
                let quoted = quoted_text(&mut characters, character);
                line.push(character);
                line.push_str(&quoted);
                line.push(character);
                outer.push(character);
                outer.push(character);
            }
            '\\' => {
                outer.push(character);
                line.push(character);
                if let Some(escaped) = characters.next() {
                    outer.push(escaped);
                    line.push(escaped);
                }
            }
            '#' if starts_word(&outer) => {
                while characters.next_if(|next| *next != '\n').is_some() {}
            }
            '<' if characters.peek() == Some(&'<') => {
                characters.next();
                let operator = if characters.next_if_eq(&'<').is_some() {
                    "<<<".to_owned()
                } else {
                    let terminator = heredoc_terminator(&mut characters);
                    pending.push(terminator.clone());
                    format!("<<{terminator}")
                };
                outer.push_str(&operator);
                line.push_str(&operator);
            }
            '\n' => {
                for terminator in std::mem::take(&mut pending) {
                    let body = heredoc_body(&mut characters, &terminator);
                    heredocs.push(Heredoc {
                        opener: line.clone(),
                        body,
                    });
                }
                outer.push('\n');
                line.clear();
            }
            _ => {
                outer.push(character);
                line.push(character);
            }
        }
    }
    Lexed { outer, heredocs }
}

type Characters<'c> = std::iter::Peekable<std::str::Chars<'c>>;

/// The text up to the closing `quote`, which is consumed; a double-quoted string keeps
/// each backslash escape whole.
fn quoted_text(characters: &mut Characters<'_>, quote: char) -> String {
    let mut text = String::new();
    while let Some(character) = characters.next() {
        if character == quote {
            break;
        }
        text.push(character);
        if quote == '"' && character == '\\' {
            if let Some(escaped) = characters.next() {
                text.push(escaped);
            }
        }
    }
    text
}

fn starts_word(outer: &str) -> bool {
    outer
        .chars()
        .last()
        .is_none_or(|last| last.is_whitespace() || matches!(last, ';' | '&' | '|' | '('))
}

/// Reads the word after `<<` or `<<-`, quoted or not, as its terminator.
fn heredoc_terminator(characters: &mut Characters<'_>) -> String {
    characters.next_if_eq(&'-');
    while characters
        .next_if(|next| *next == ' ' || *next == '\t')
        .is_some()
    {}
    let mut terminator = String::new();
    let quote = characters.next_if(|next| *next == '\'' || *next == '"');
    while let Some(next) = characters.peek() {
        let ends = match quote {
            Some(quote) => *next == quote,
            None => next.is_whitespace() || matches!(next, ';' | '&' | '|' | '<' | '>' | '(' | ')'),
        };
        if ends {
            break;
        }
        if *next != '\\' {
            terminator.push(*next);
        }
        characters.next();
    }
    if quote.is_some() {
        characters.next();
    }
    terminator
}

/// The heredoc's body up to its terminator line, which is consumed and left out.
fn heredoc_body(characters: &mut Characters<'_>, terminator: &str) -> String {
    let mut body = String::new();
    let mut line = String::new();
    for character in characters.by_ref() {
        if character != '\n' {
            line.push(character);
            continue;
        }
        if line.trim() == terminator {
            return body;
        }
        body.push_str(&line);
        body.push('\n');
        line.clear();
    }
    if line.trim() != terminator {
        body.push_str(&line);
    }
    body
}

/// Splits the blanked command at every operator that starts another command.
fn simple_commands(outer: &str) -> Vec<Vec<&str>> {
    outer
        .split(COMMAND_SEPARATORS)
        .map(|piece| piece.split_whitespace().collect::<Vec<&str>>())
        .filter(|words| !words.is_empty())
        .collect()
}

/// The program a simple command runs, by its base name, and the words after it.
fn program<'w>(words: &'w [&'w str]) -> Option<(&'w str, &'w [&'w str])> {
    let mut rest = words;
    for _ in 0..=words.len() {
        let (first, tail) = rest.split_first()?;
        if is_assignment(first) || COMMAND_PREFIXES.contains(first) {
            rest = tail;
        } else if *first == "xargs" {
            rest = skip_xargs_options(tail);
        } else if *first == "timeout" {
            let options_skipped = skip_options(tail);
            rest = options_skipped
                .split_first()
                .map_or(options_skipped, |(_, after_duration)| after_duration);
        } else {
            let name = first.rsplit('/').next().unwrap_or(first);
            return Some((name, tail));
        }
    }
    unreachable!("every pass consumes at least one word, so the words run out first");
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut characters = name.chars();
    let starts = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    starts && characters.all(|next| next.is_ascii_alphanumeric() || next == '_')
}

fn skip_options<'w>(words: &'w [&'w str]) -> &'w [&'w str] {
    let position = words
        .iter()
        .position(|word| !word.starts_with('-'))
        .unwrap_or(words.len());
    words.get(position..).unwrap_or_default()
}

fn skip_xargs_options<'w>(words: &'w [&'w str]) -> &'w [&'w str] {
    let mut rest = words;
    while let Some((first, tail)) = rest.split_first() {
        if !first.starts_with('-') {
            break;
        }
        rest = if XARGS_OPTIONS_WITH_VALUE.contains(first) {
            tail.split_first().map_or(tail, |(_, after)| after)
        } else {
            tail
        };
    }
    rest
}

fn editor_offense(words: &[&str], raw: &str) -> Option<Offense> {
    let (name, arguments) = program(words)?;
    match name {
        "sed" | "gsed" => sed_in_place(arguments).then_some(Offense::SedInPlace),
        "perl" => perl_in_place(arguments).then_some(Offense::PerlInPlace),
        "awk" | "gawk" => awk_in_place(arguments).then_some(Offense::AwkInPlace),
        "sponge" => Some(Offense::Sponge),
        "node" | "deno" | "bun" => NODE_WRITE.is_match(raw).then_some(Offense::NodeWrite),
        _ if name.starts_with("python") || name.starts_with("pypy") => {
            PYTHON_WRITE.is_match(raw).then_some(Offense::PythonWrite)
        }
        _ => None,
    }
}

/// `sed` edits in place under `--in-place` or an `i` in a short-option cluster before
/// any option that takes a value.
fn sed_in_place(arguments: &[&str]) -> bool {
    arguments.iter().any(|argument| {
        if *argument == "--in-place" || argument.starts_with("--in-place=") {
            return true;
        }
        let Some(cluster) = short_cluster(argument) else {
            return false;
        };
        for flag in cluster.chars() {
            match flag {
                'i' => return true,
                'e' | 'f' | 'l' => return false,
                _ => {}
            }
        }
        false
    })
}

/// `perl` edits in place under an `i` switch. Switches that take an attached value end
/// the cluster, so `-Mstrict` names a module rather than holding `-i`.
fn perl_in_place(arguments: &[&str]) -> bool {
    arguments.iter().any(|argument| {
        let Some(cluster) = short_cluster(argument) else {
            return false;
        };
        let mut flags = cluster.chars().peekable();
        while let Some(flag) = flags.next() {
            match flag {
                'i' => return true,
                'e' | 'E' | 'M' | 'm' | 'I' | 'D' | 'd' | 'F' | 'x' | 'C' => return false,
                '0' | 'l' => while flags.next_if(char::is_ascii_hexdigit).is_some() {},
                _ => {}
            }
        }
        false
    })
}

fn short_cluster(argument: &str) -> Option<&str> {
    if argument.starts_with("--") {
        return None;
    }
    argument.strip_prefix('-')
}

fn awk_in_place(arguments: &[&str]) -> bool {
    let attached = arguments
        .iter()
        .any(|argument| *argument == "-iinplace" || *argument == "--include=inplace");
    let separate = arguments
        .windows(2)
        .any(|pair| matches!(pair, ["-i" | "--include", "inplace"]));
    attached || separate
}

/// A redirect into some file followed by `mv` of that same file is a rewrite of the
/// file `mv` names as its target.
fn moves_over_original(outer: &str, commands: &[Vec<&str>]) -> bool {
    let targets: Vec<&str> = REDIRECT_TARGET
        .captures_iter(outer)
        .filter_map(|captures| captures.get(1).map(|target| target.as_str()))
        .filter(|target| *target != "/dev/null")
        .collect();
    if targets.is_empty() {
        return false;
    }
    commands.iter().any(|words| match program(words) {
        Some(("mv", arguments)) => skip_options(arguments)
            .first()
            .is_some_and(|source| targets.contains(source)),
        _ => false,
    })
}

#[cfg(test)]
mod tests;
