//! How long the hook takes to judge one Bash command. One group times commands shaped
//! like the ones agents run, some let through and some denied; the other times a heredoc
//! at three lengths, its throughput counted in lines, so a lexer that scales worse than
//! linearly loses lines per second as the heredoc grows. Each case first checks the
//! verdict it declares, then times `verdict` alone on a command held in memory; starting
//! the process and reading the hook's input stand outside what is timed.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use splice::guard::{Offense, Verdict, verdict};

const HEREDOC_LINE_COUNTS: [usize; 3] = [100, 1_000, 10_000];
const SAMPLE_COUNT: usize = 50;
const WARM_UP_SECONDS: u64 = 1;
const MEASUREMENT_SECONDS: u64 = 2;

/// A command and the verdict the hook gives it, named for its shape.
struct Case {
    name: &'static str,
    command: &'static str,
    verdict: Verdict,
}

const COMMANDS: [Case; 5] = [
    Case {
        name: "listing",
        command: "ls -la",
        verdict: Verdict::Allow,
    },
    Case {
        name: "pipeline",
        command: "git log --oneline | head -20 && cargo test --quiet 2>&1 | tail -5",
        verdict: Verdict::Allow,
    },
    Case {
        name: "splice-heredoc",
        command: "splice <<'EOF'\n=== src/main.rs\n@@\n-old();\n+new();\nEOF",
        verdict: Verdict::Allow,
    },
    Case {
        name: "sed-in-place",
        command: "sed -i 's/old/new/' src/main.rs",
        verdict: Verdict::Deny(Offense::SedInPlace),
    },
    Case {
        name: "python-write",
        command: "python3 -c 'open(\"notes.txt\", \"w\").write(\"text\")'",
        verdict: Verdict::Deny(Offense::PythonWrite),
    },
];

fn verdict_by_command(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("command");
    for case in &COMMANDS {
        let judged = verdict(case.command);
        assert_eq!(
            judged, case.verdict,
            "{} is judged as it declares",
            case.name
        );
        let id = BenchmarkId::from_parameter(case.name);
        group.bench_with_input(id, case.command, |bencher, command| {
            bencher.iter(|| verdict(black_box(command)));
        });
    }
    group.finish();
}

fn verdict_by_heredoc_length(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("heredoc");
    for line_count in HEREDOC_LINE_COUNTS {
        let command = heredoc(line_count);
        let judged = verdict(&command);
        assert_eq!(
            judged,
            Verdict::Allow,
            "a heredoc written to a file is let through"
        );
        let Ok(elements) = u64::try_from(line_count) else {
            unreachable!("a line count fits a u64");
        };
        group.throughput(Throughput::Elements(elements));
        let id = BenchmarkId::from_parameter(line_count);
        group.bench_with_input(id, &command, |bencher, command| {
            bencher.iter(|| verdict(black_box(command.as_str())));
        });
    }
    group.finish();
}

/// A command writing a file through a heredoc whose body is `line_count` lines.
fn heredoc(line_count: usize) -> String {
    assert!(line_count > 0, "a heredoc holds at least one line");
    let mut command = String::from("cat > notes.md <<'EOF'\n");
    for index in 0..line_count {
        let line = format!("line {index} of the notes\n");
        command.push_str(&line);
    }
    command.push_str("EOF");
    command
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
    targets = verdict_by_command, verdict_by_heredoc_length
}
criterion_main!(benches);
