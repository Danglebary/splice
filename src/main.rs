//! The imperative shell: reads the script and the files, writes the files, runs the
//! command under `try`, and turns every outcome into an exit code. Every decision it acts
//! on is made by the library.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use splice::cli::{self, Command, Options};
use splice::guard::{self, HookInput, Verdict};
use splice::plan::{self, Reason, Refusal};
use splice::script::{self, Script};
use splice::{diff, exit_code, report};

/// The largest script read, so a runaway pipe cannot exhaust memory.
const SCRIPT_BYTES_MAX: u64 = 16_777_216;
/// The largest file edited.
const FILE_BYTES_MAX: u64 = 67_108_864;
/// The largest hook input read.
const HOOK_INPUT_BYTES_MAX: u64 = 4_194_304;
/// How many names beside a file are tried for its temporary before staging gives up:
/// enough to step past the leftovers of a few interrupted runs, and a bound on a
/// directory someone has filled with them.
const TEMPORARY_NAMES_MAX: usize = 16;
/// The mode a temporary holding an existing file's text is created with, readable by its
/// owner alone until it takes the original's permissions.
const PRIVATE_FILE_MODE: u32 = 0o600;
/// The mode a new file is created with before the umask applies, the one
/// `File::create` uses.
const NEW_FILE_MODE: u32 = 0o666;
/// The interruption flag's value until a signal arrives; signals are numbered from 1.
const NO_SIGNAL: usize = 0;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let code = match cli::parse_arguments(&arguments) {
        Ok(Command::Help) => {
            print!("{}", report::HELP);
            exit_code::SUCCESS
        }
        Ok(Command::Version) => {
            println!("splice {}", env!("CARGO_PKG_VERSION"));
            exit_code::SUCCESS
        }
        Ok(Command::Guard) => run_guard(),
        Ok(Command::Apply(options)) => run_apply(&options),
        Ok(Command::Try {
            options,
            expect_failure,
            program,
        }) => run_try(&options, expect_failure, &program),
        Err(fault) => {
            eprint!("{}", report::usage_fault(&fault));
            exit_code::USAGE
        }
    };
    ExitCode::from(code)
}

/// A file a script edits, as it stood before any write. `location` is the canonical
/// path of an existing file, so a symbolic link is edited at its target. `directories`
/// are those above a new file that do not exist yet, deepest first, which writing it
/// makes.
struct Target {
    path: String,
    location: PathBuf,
    original: Option<String>,
    permissions: Option<fs::Permissions>,
    directories: Vec<PathBuf>,
}

struct Change {
    target: Target,
    planned: String,
}

enum Prepared {
    Ready(Vec<Change>),
    Refused(Vec<(String, Refusal)>),
    Failed { path: String, error: io::Error },
}

fn run_apply(options: &Options) -> u8 {
    let changes = match load_changes(options) {
        Ok(changes) => changes,
        Err(code) => return code,
    };
    if options.diff || options.dry_run {
        print_diffs(&changes);
    }
    if options.dry_run {
        return exit_code::SUCCESS;
    }
    if let Err(code) = write_all(&changes) {
        return code;
    }
    verify_written(&changes)
}

fn run_try(options: &Options, expect_failure: bool, program: &[String]) -> u8 {
    let changes = match load_changes(options) {
        Ok(changes) => changes,
        Err(code) => return code,
    };
    let Some((name, arguments)) = program.split_first() else {
        unreachable!("the grammar refuses `try` without a program");
    };
    let interruption = match register_interruption() {
        Ok(interruption) => interruption,
        Err(code) => return code,
    };
    if options.diff {
        print_diffs(&changes);
    }
    if let Err(code) = write_all(&changes) {
        return code;
    }
    // An interrupt that lands while the files are written keeps the command from starting.
    let status = if interruption.load(Ordering::SeqCst) == NO_SIGNAL {
        let command = process::Command::new(name)
            .args(arguments)
            .stdin(Stdio::null())
            .status();
        Some(command)
    } else {
        None
    };
    let everything: Vec<&Change> = changes.iter().collect();
    let restored = restore(&everything);
    let removed = remove_directories(&everything);
    if restored != exit_code::SUCCESS {
        return restored;
    }
    if removed != exit_code::SUCCESS {
        return removed;
    }
    let signal = interruption.load(Ordering::SeqCst);
    if signal != NO_SIGNAL {
        eprint!("{}", report::INTERRUPTED);
        return interrupted_code(signal);
    }
    let Some(status) = status else {
        unreachable!("the command runs unless an interrupt came before it");
    };
    command_outcome(name, status, expect_failure)
}

/// A flag holding the number of the first signal that interrupts `try`. Each handler
/// replaces its signal's default of ending splice, so an interrupt reaches the command
/// and splice lives on to restore the files; handlers do not survive the command's exec,
/// so the command still ends on the signal as usual.
fn register_interruption() -> Result<Arc<AtomicUsize>, u8> {
    let interruption = Arc::new(AtomicUsize::new(NO_SIGNAL));
    for signal in [SIGINT, SIGTERM, SIGHUP] {
        let Ok(number) = usize::try_from(signal) else {
            unreachable!("a signal's number is positive");
        };
        assert_ne!(number, NO_SIGNAL, "no signal is numbered 0");
        let registered =
            signal_hook::flag::register_usize(signal, Arc::clone(&interruption), number);
        if let Err(error) = registered {
            eprint!("{}", report::io_failure("signal handler", &error));
            return Err(exit_code::IO);
        }
    }
    Ok(interruption)
}

/// The shells' code for a run ended by `signal`, which an interrupted `try` exits with
/// whatever the command did, since an interrupted run shows nothing about the edit.
fn interrupted_code(signal: usize) -> u8 {
    let Ok(number) = u8::try_from(signal) else {
        unreachable!("the signals `try` handles are numbered below 128");
    };
    let Some(code) = exit_code::SIGNAL_BASE.checked_add(number) else {
        unreachable!("the signals `try` handles are numbered below 128");
    };
    code
}

fn command_outcome(name: &str, status: io::Result<ExitStatus>, expect_failure: bool) -> u8 {
    match status {
        Ok(status) => try_outcome(status, expect_failure),
        Err(error) => {
            eprint!("{}", report::program_failure(name, &error));
            if error.kind() == io::ErrorKind::NotFound {
                exit_code::PROGRAM_NOT_FOUND
            } else {
                exit_code::PROGRAM_NOT_EXECUTABLE
            }
        }
    }
}

fn try_outcome(status: ExitStatus, expect_failure: bool) -> u8 {
    if expect_failure {
        return if status.success() {
            exit_code::SURVIVED
        } else {
            exit_code::SUCCESS
        };
    }
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(u8::MAX);
    }
    let Some(signal) = status.signal() else {
        unreachable!("a status with no exit code ended on a signal");
    };
    exit_code::SIGNAL_BASE.saturating_add(u8::try_from(signal).unwrap_or(u8::MAX))
}

fn run_guard() -> u8 {
    let input = match read_bounded(io::stdin().lock(), HOOK_INPUT_BYTES_MAX) {
        Ok(Some(input)) => input,
        Ok(None) => {
            eprint!(
                "{}",
                report::hook_input_unreadable("it is past the size splice reads")
            );
            return exit_code::HOOK_ERROR;
        }
        Err(error) => {
            eprint!("{}", report::hook_input_unreadable(&error.to_string()));
            return exit_code::HOOK_ERROR;
        }
    };
    let command = match guard::hook_input(&input) {
        Ok(HookInput::Bash(command)) => command,
        Ok(HookInput::OtherTool) => return exit_code::SUCCESS,
        Err(fault) => {
            eprint!("{}", report::hook_input_fault(&fault));
            return exit_code::HOOK_ERROR;
        }
    };
    match guard::verdict(&command) {
        Verdict::Allow => exit_code::SUCCESS,
        Verdict::Deny(offense) => {
            eprint!("{}", report::guard_denial(offense));
            exit_code::HOOK_BLOCK
        }
    }
}

/// Reads and parses the script, then reads and plans every file it names.
fn load_changes(options: &Options) -> Result<Vec<Change>, u8> {
    let script = read_script(options)?;
    match prepare(&script) {
        Prepared::Ready(changes) => Ok(changes),
        Prepared::Refused(refused) => {
            for (path, refusal) in &refused {
                eprint!("{}", report::refusal(path, refusal));
            }
            eprint!("{}", report::NOTHING_WRITTEN);
            Err(exit_code::REFUSED)
        }
        Prepared::Failed { path, error } => {
            eprint!("{}", report::io_failure(&path, &error));
            eprint!("{}", report::NOTHING_WRITTEN);
            Err(exit_code::IO)
        }
    }
}

fn read_script(options: &Options) -> Result<Script, u8> {
    let read = match &options.script {
        Some(path) => fs::File::open(path).and_then(|file| read_bounded(file, SCRIPT_BYTES_MAX)),
        None => read_bounded(io::stdin().lock(), SCRIPT_BYTES_MAX),
    };
    let bytes = match read {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            eprint!(
                "{}",
                report::script_unreadable(&format!("it is past {SCRIPT_BYTES_MAX} bytes"))
            );
            return Err(exit_code::USAGE);
        }
        Err(error) => {
            eprint!("{}", report::script_unreadable(&error.to_string()));
            return Err(exit_code::IO);
        }
    };
    let Ok(text) = String::from_utf8(bytes) else {
        eprint!("{}", report::script_unreadable("it is not UTF-8 text"));
        return Err(exit_code::USAGE);
    };
    script::parse(&text).map_err(|error| {
        eprint!("{}", report::script_error(&error));
        exit_code::USAGE
    })
}

/// Reads at most `limit` bytes, and `None` when the reader holds more.
fn read_bounded(reader: impl Read, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let within = u64::try_from(bytes.len()).is_ok_and(|length| length <= limit);
    Ok(within.then_some(bytes))
}

fn prepare(script: &Script) -> Prepared {
    let mut changes: Vec<Change> = Vec::new();
    let mut refused: Vec<(String, Refusal)> = Vec::new();
    for section in &script.sections {
        let Some(first_hunk) = section.hunks.first() else {
            unreachable!("the parser refuses a section without hunks");
        };
        for path in &section.paths {
            let target = match load(path) {
                Ok(Ok(target)) => target,
                Ok(Err(reason)) => {
                    refused.push((
                        path.clone(),
                        Refusal {
                            hunk_line: first_hunk.line,
                            reason,
                        },
                    ));
                    continue;
                }
                Err(error) => {
                    return Prepared::Failed {
                        path: path.clone(),
                        error,
                    };
                }
            };
            if let Some(earlier) = changes
                .iter()
                .find(|change| change.target.location == target.location)
            {
                let reason = Reason::SameFileAs {
                    path: earlier.target.path.clone(),
                };
                refused.push((
                    path.clone(),
                    Refusal {
                        hunk_line: first_hunk.line,
                        reason,
                    },
                ));
                continue;
            }
            match plan::plan(target.original.as_deref(), &section.hunks) {
                Ok(planned) => changes.push(Change { target, planned }),
                Err(refusals) => {
                    for refusal in refusals {
                        refused.push((path.clone(), refusal));
                    }
                }
            }
        }
    }
    if refused.is_empty() {
        Prepared::Ready(changes)
    } else {
        Prepared::Refused(refused)
    }
}

/// The file as it stands, `Ok(Err(..))` for a file splice refuses to edit.
fn load(path: &str) -> io::Result<Result<Target, Reason>> {
    let written = PathBuf::from(path);
    let metadata = match fs::metadata(&written) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let target = Target {
                path: path.to_owned(),
                original: None,
                permissions: None,
                directories: missing_directories(&written)?,
                location: written,
            };
            return Ok(Ok(target));
        }
        Err(error) => return Err(error),
    };
    if metadata.len() > FILE_BYTES_MAX {
        return Ok(Err(Reason::TooLarge {
            bytes: metadata.len(),
        }));
    }
    let location = fs::canonicalize(&written)?;
    let bytes = fs::read(&location)?;
    let Ok(original) = String::from_utf8(bytes) else {
        return Ok(Err(Reason::NotText));
    };
    let permissions = Some(metadata.permissions());
    Ok(Ok(Target {
        path: path.to_owned(),
        location,
        original: Some(original),
        permissions,
        directories: Vec::new(),
    }))
}

/// The directories above `path` that do not exist yet, deepest first.
fn missing_directories(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    for directory in path.ancestors().skip(1) {
        if directory.as_os_str().is_empty() {
            break;
        }
        match fs::symlink_metadata(directory) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(directory.to_path_buf());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(missing)
}

fn print_diffs(changes: &[Change]) {
    for change in changes {
        print!(
            "{}",
            diff::unified(
                &change.target.path,
                change.target.original.as_deref(),
                &change.planned
            )
        );
    }
}

/// Writes every change or none. Every new text is written and synced beside its file
/// before any file is replaced, so a full disk or a locked directory fails while every
/// original still stands; a replacement that fails puts back the files replaced before
/// it.
fn write_all(changes: &[Change]) -> Result<(), u8> {
    let staged = stage_all(changes)?;
    assert_eq!(
        staged.len(),
        changes.len(),
        "every change is staged before any file is replaced"
    );
    commit_all(changes, &staged)
}

/// The temporary file holding each change's text, removing every one written when one
/// fails.
fn stage_all(changes: &[Change]) -> Result<Vec<PathBuf>, u8> {
    let mut staged: Vec<PathBuf> = Vec::with_capacity(changes.len());
    for change in changes {
        let target = &change.target;
        match stage(
            &target.location,
            &change.planned,
            target.permissions.as_ref(),
        ) {
            Ok(temporary) => staged.push(temporary),
            Err(error) => {
                eprint!("{}", report::write_failure(&target.path, &error));
                discard(&staged);
                eprint!("{}", report::NOTHING_WRITTEN);
                let everything: Vec<&Change> = changes.iter().collect();
                remove_directories(&everything);
                return Err(exit_code::IO);
            }
        }
    }
    Ok(staged)
}

/// Renames each staged temporary over its file. When one rename fails, the temporaries
/// not yet renamed are removed and the files already replaced are restored.
fn commit_all(changes: &[Change], staged: &[PathBuf]) -> Result<(), u8> {
    for (index, (change, temporary)) in changes.iter().zip(staged).enumerate() {
        let Err(error) = fs::rename(temporary, &change.target.location) else {
            continue;
        };
        eprint!("{}", report::write_failure(&change.target.path, &error));
        let Some(unrenamed) = staged.get(index..) else {
            unreachable!("a failed rename's index lies inside the staged temporaries");
        };
        discard(unrenamed);
        let replaced: Vec<&Change> = changes.iter().take(index).collect();
        if restore(&replaced) == exit_code::SUCCESS {
            eprint!("{}", report::NOTHING_WRITTEN);
        }
        let everything: Vec<&Change> = changes.iter().collect();
        remove_directories(&everything);
        return Err(exit_code::IO);
    }
    Ok(())
}

/// Removes temporaries that will never be renamed, naming any that remain.
fn discard(temporaries: &[PathBuf]) {
    for temporary in temporaries {
        if let Err(error) = fs::remove_file(temporary) {
            let path = temporary.display().to_string();
            eprint!("{}", report::temporary_remains(&path, &error));
        }
    }
}

/// Reads every written file back, so a write that did not land as planned is reported
/// rather than assumed.
fn verify_written(changes: &[Change]) -> u8 {
    for change in changes {
        match fs::read_to_string(&change.target.location) {
            Ok(text) if text == change.planned => {}
            Ok(_) => {
                eprint!("{}", report::readback_mismatch(&change.target.path));
                return exit_code::IO;
            }
            Err(error) => {
                eprint!("{}", report::io_failure(&change.target.path, &error));
                return exit_code::IO;
            }
        }
    }
    exit_code::SUCCESS
}

/// Puts every file back as it was, removing those the script created, and reads each
/// back. It restores every file it can even after one fails.
fn restore(changes: &[&Change]) -> u8 {
    let mut code = exit_code::SUCCESS;
    for change in changes {
        let target = &change.target;
        let restored = match &target.original {
            Some(original) => {
                write_atomically(&target.location, original, target.permissions.as_ref())
                    .and_then(|()| fs::read_to_string(&target.location))
                    .and_then(|text| {
                        if &text == original {
                            Ok(())
                        } else {
                            Err(io::Error::other("the file read back differs"))
                        }
                    })
            }
            None => fs::remove_file(&target.location),
        };
        if let Err(error) = restored {
            eprint!("{}", report::restore_failure(&target.path, &error));
            code = exit_code::IO;
        }
    }
    code
}

/// Removes the directories each change's new file needed, deepest first. A directory
/// that holds something else stays, and so does every directory above it.
fn remove_directories(changes: &[&Change]) -> u8 {
    let mut code = exit_code::SUCCESS;
    for change in changes {
        for directory in &change.target.directories {
            match fs::remove_dir(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => break,
                Err(error) => {
                    let path = directory.display().to_string();
                    eprint!("{}", report::directory_remains(&path, &error));
                    code = exit_code::IO;
                    break;
                }
            }
        }
    }
    code
}

/// Writes a sibling temporary file, syncs it, and renames it over `location`, so a
/// reader sees the old file or the new one and never a partial write.
fn write_atomically(
    location: &Path,
    text: &str,
    permissions: Option<&fs::Permissions>,
) -> io::Result<()> {
    let temporary = stage(location, text, permissions)?;
    fs::rename(&temporary, location).map_err(|error| without_temporary(&temporary, error))
}

/// Writes and syncs `text` to a temporary file beside `location`, creating the
/// directory it lies in, and returns the temporary's path.
fn stage(
    location: &Path,
    text: &str,
    permissions: Option<&fs::Permissions>,
) -> io::Result<PathBuf> {
    let directory = match location.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    fs::create_dir_all(directory)?;
    let Some(name) = location.file_name() else {
        return Err(io::Error::other("the path names no file"));
    };
    let mode = permissions.map_or(NEW_FILE_MODE, |_| PRIVATE_FILE_MODE);
    let (temporary, file) = create_temporary(directory, name, mode)?;
    match write_synced(file, text, permissions) {
        Ok(()) => Ok(temporary),
        Err(error) => Err(without_temporary(&temporary, error)),
    }
}

/// Creates the first free temporary name beside the file `name`, opening only a path
/// that does not exist yet, so a leftover temporary or a link someone planted at the
/// name is stepped past rather than written through.
fn create_temporary(directory: &Path, name: &OsStr, mode: u32) -> io::Result<(PathBuf, fs::File)> {
    for attempt in 0..TEMPORARY_NAMES_MAX {
        let temporary = directory.join(temporary_name(name, attempt));
        let opened = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary);
        match opened {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("all {TEMPORARY_NAMES_MAX} temporary names beside the file are taken"),
    ))
}

/// `.NAME.splice` for the first attempt, then `.NAME.splice-1`, `.NAME.splice-2`, and on.
fn temporary_name(name: &OsStr, attempt: usize) -> OsString {
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(".splice");
    if attempt > 0 {
        temporary.push(format!("-{attempt}"));
    }
    temporary
}

/// `error` once the temporary it left behind is removed, or `error` naming the
/// temporary when it cannot be.
fn without_temporary(temporary: &Path, error: io::Error) -> io::Error {
    match fs::remove_file(temporary) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => error,
        Err(cleanup) => io::Error::other(format!(
            "{error}; the temporary {} remains: {cleanup}",
            temporary.display()
        )),
    }
}

fn write_synced(
    mut file: fs::File,
    text: &str,
    permissions: Option<&fs::Permissions>,
) -> io::Result<()> {
    file.write_all(text.as_bytes())?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions.clone())?;
    }
    file.sync_all()
}
