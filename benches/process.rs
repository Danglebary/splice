//! What one call to the built binary costs from spawn to exit, which is the cost an
//! agent pays per call: the hook judging a Bash command, and an edit read, planned, and
//! written with its fsync. Each case spawns the binary cargo builds for this bench,
//! writes to its stdin what its caller would, and waits for it to exit; `--version`
//! reads nothing and stands as the floor every call pays to start a process. Each case
//! checks its exit code before it is timed, and the edit case writes its file back
//! outside the timed routine before every run.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use splice::exit_code;

const BINARY: &str = env!("CARGO_BIN_EXE_splice");
const SCRATCH_DIRECTORY: &str = env!("CARGO_TARGET_TMPDIR");
const LINE_COUNTS: [usize; 2] = [1_000, 100_000];
const SAMPLE_COUNT: usize = 50;
const WARM_UP_SECONDS: u64 = 1;
const MEASUREMENT_SECONDS: u64 = 3;

/// One call: the arguments, what its caller writes to stdin, and the code it exits with.
struct Call {
    name: &'static str,
    arguments: &'static [&'static str],
    input: &'static str,
    exit_code: u8,
}

const CALLS: [Call; 4] = [
    Call {
        name: "version",
        arguments: &["--version"],
        input: "",
        exit_code: exit_code::SUCCESS,
    },
    Call {
        name: "guard-allow",
        arguments: &["guard"],
        input: r#"{"tool_name":"Bash","tool_input":{"command":"ls -la"}}"#,
        exit_code: exit_code::SUCCESS,
    },
    Call {
        name: "guard-redirect",
        arguments: &["guard"],
        input: r#"{"tool_name":"Bash","tool_input":{"command":"cargo test --quiet 2>&1 | tail -5"}}"#,
        exit_code: exit_code::SUCCESS,
    },
    Call {
        name: "guard-deny",
        arguments: &["guard"],
        input: r#"{"tool_name":"Bash","tool_input":{"command":"python3 -c 'open(\"notes.txt\", \"w\").write(\"text\")'"}}"#,
        exit_code: exit_code::HOOK_BLOCK,
    },
];

fn call_by_kind(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("call");
    for call in &CALLS {
        let code = exit_code_of(call.arguments, call.input);
        assert_eq!(
            code,
            Some(i32::from(call.exit_code)),
            "{} exits as it declares",
            call.name
        );
        let id = BenchmarkId::from_parameter(call.name);
        group.bench_with_input(id, call, |bencher, call| {
            bencher.iter(|| exit_code_of(call.arguments, call.input));
        });
    }
    group.finish();
}

fn edit_by_length(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("edit");
    for line_count in LINE_COUNTS {
        let name = format!("lines-{line_count}.txt");
        let path = Path::new(SCRATCH_DIRECTORY).join(name);
        let original = numbered_lines(line_count);
        let script = format!("=== {}\n@@\n-line 1\n+line one\n", path.display());
        write_scratch(&path, &original);
        let code = exit_code_of(&[], &script);
        assert_eq!(
            code,
            Some(i32::from(exit_code::SUCCESS)),
            "the edit applies"
        );
        let edited = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => unreachable!("the edited scratch file reads back: {error}"),
        };
        assert_ne!(edited, original, "the edit changed the file");
        let id = BenchmarkId::from_parameter(line_count);
        group.bench_with_input(id, &script, |bencher, script| {
            bencher.iter_batched(
                || write_scratch(&path, &original),
                |()| exit_code_of(&[], script),
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

/// Spawns the binary with `arguments`, writes `input` to its stdin, and returns the code
/// it exits with, or `None` when a signal ends it.
fn exit_code_of(arguments: &[&str], input: &str) -> Option<i32> {
    // The child runs under the C library's default allocator settings, as an agent's call
    // does, whatever thresholds this bench process runs under.
    let spawned = Command::new(BINARY)
        .args(arguments)
        .env_remove("GLIBC_TUNABLES")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => unreachable!("the bench's own binary starts: {error}"),
    };
    let Some(mut stdin) = child.stdin.take() else {
        unreachable!("a child spawned with a piped stdin holds its end");
    };
    if let Err(error) = stdin.write_all(input.as_bytes()) {
        unreachable!("the binary reads its input whole: {error}");
    }
    drop(stdin);
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => unreachable!("a spawned child is waited on: {error}"),
    };
    status.code()
}

fn write_scratch(path: &Path, text: &str) {
    if let Err(error) = fs::write(path, text) {
        unreachable!("the scratch file is written: {error}");
    }
}

/// `line_count` lines, each `line` and its index, so every line is distinct.
fn numbered_lines(line_count: usize) -> String {
    assert!(line_count > 1, "the edited line lies inside the file");
    let mut text = String::new();
    for index in 0..line_count {
        let line = format!("line {index}\n");
        text.push_str(&line);
    }
    text
}

fn configured() -> Criterion {
    let warm_up = Duration::from_secs(WARM_UP_SECONDS);
    let measurement = Duration::from_secs(MEASUREMENT_SECONDS);
    Criterion::default()
        .sample_size(SAMPLE_COUNT)
        .warm_up_time(warm_up)
        .measurement_time(measurement)
}

criterion_group! {
    name = benches;
    config = configured();
    targets = call_by_kind, edit_by_length
}
criterion_main!(benches);
