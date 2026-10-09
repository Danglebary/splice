//! The command line's grammar: the arguments in, the command to run or a usage fault out.

/// What one invocation does.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Apply(Options),
    Try {
        options: Options,
        expect_failure: bool,
        program: Vec<String>,
    },
    Guard,
    Help,
    Version,
}

/// Flags shared by applying and trying a script.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub diff: bool,
    pub dry_run: bool,
    pub script: Option<String>,
}

/// Every way the arguments break the grammar.
#[derive(Debug, PartialEq, Eq)]
pub enum UsageFault {
    UnknownArgument(String),
    MissingValue(String),
    MissingProgram,
    DryRunUnderTry,
    ExpectFailureOutsideTry,
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// Returns the first argument the grammar refuses.
pub fn parse_arguments(arguments: &[String]) -> Result<Command, UsageFault> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--help" | "-h"] => Ok(Command::Help),
        ["--version" | "-V"] => Ok(Command::Version),
        ["guard"] => Ok(Command::Guard),
        ["try", rest @ ..] => parse_try(rest),
        rest => {
            let (options, expect_failure, program) = parse_flags(rest)?;
            if expect_failure {
                return Err(UsageFault::ExpectFailureOutsideTry);
            }
            if let Some(first) = program.first() {
                return Err(UsageFault::UnknownArgument(first.clone()));
            }
            Ok(Command::Apply(options))
        }
    }
}

fn parse_try(words: &[&str]) -> Result<Command, UsageFault> {
    let (options, expect_failure, program) = parse_flags(words)?;
    if options.dry_run {
        return Err(UsageFault::DryRunUnderTry);
    }
    if program.is_empty() {
        return Err(UsageFault::MissingProgram);
    }
    Ok(Command::Try {
        options,
        expect_failure,
        program,
    })
}

/// The flags before `--`, and every word after it as the program to run.
fn parse_flags(words: &[&str]) -> Result<(Options, bool, Vec<String>), UsageFault> {
    let mut options = Options::default();
    let mut expect_failure = false;
    let mut rest = words;
    while let Some((first, tail)) = rest.split_first() {
        rest = tail;
        match *first {
            "--diff" => options.diff = true,
            "--dry-run" => options.dry_run = true,
            "--expect-fail" => expect_failure = true,
            "--script" => {
                let Some((path, after_path)) = rest.split_first() else {
                    return Err(UsageFault::MissingValue((*first).to_owned()));
                };
                options.script = Some((*path).to_owned());
                rest = after_path;
            }
            "--" => {
                let program: Vec<String> = rest.iter().map(|word| (*word).to_owned()).collect();
                assert_eq!(
                    program.len(),
                    rest.len(),
                    "every word after `--` belongs to the program"
                );
                return Ok((options, expect_failure, program));
            }
            other => return Err(UsageFault::UnknownArgument(other.to_owned())),
        }
    }
    Ok((options, expect_failure, Vec::new()))
}

#[cfg(test)]
mod tests;
