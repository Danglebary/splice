//! How the time to plan one hunk grows with the length of the file it edits. Each case
//! generates a file of three-line items at three lengths, parses its hunk once outside
//! the timed loop, checks that the hunk applies or is refused as the case declares, and
//! times `plan` alone. Throughput is counted in lines, so a case that scales linearly
//! keeps its lines per second as the file grows and a case that scales worse loses them.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use splice::plan::plan;
use splice::script::{Hunk, parse};

const ITEM_COUNTS: [usize; 3] = [400, 4_000, 40_000];
const SAMPLE_COUNT: usize = 10;
const WARM_UP_SECONDS: u64 = 1;
const MEASUREMENT_SECONDS: u64 = 3;
/// The line closing every generated file, which no item holds.
const LAST_LINE: &str = "// end of items";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Applied,
    Refused,
}

/// A hunk in the script grammar, named for the shape whose cost it measures.
struct Case {
    name: &'static str,
    hunk: &'static str,
    outcome: Outcome,
}

/// The elision cases lead with `}`, which closes every item, so the lines above each
/// elision match at a third of the file's lines.
const CASES: [Case; 5] = [
    Case {
        name: "line",
        hunk: "@@\n-    let value = 1;\n+    let value = 0;\n",
        outcome: Outcome::Applied,
    },
    Case {
        name: "elision-to-absent-line",
        hunk: "@@\n }\n~\n-absent\n+present\n",
        outcome: Outcome::Refused,
    },
    Case {
        name: "elision-to-last-line",
        hunk: "@@ all\n }\n~\n-// end of items\n+// end\n",
        outcome: Outcome::Applied,
    },
    Case {
        name: "inline",
        hunk: "@@ inline\n-item_1()\n+item_one()\n",
        outcome: Outcome::Applied,
    },
    Case {
        name: "regex",
        hunk: "@@ regex all\n-value = (\\d+);\n+value = ${1}u32;\n",
        outcome: Outcome::Applied,
    },
];

fn plan_by_length(criterion: &mut Criterion) {
    for case in &CASES {
        let hunks = parsed(case.hunk);
        let mut group = criterion.benchmark_group(case.name);
        for count in ITEM_COUNTS {
            let text = items(count);
            let planned = plan(Some(text.as_str()), &hunks);
            let outcome = if planned.is_ok() {
                Outcome::Applied
            } else {
                Outcome::Refused
            };
            assert_eq!(outcome, case.outcome, "{} plans as it declares", case.name);
            let line_count = text.lines().count();
            let Ok(elements) = u64::try_from(line_count) else {
                unreachable!("a line count fits a u64");
            };
            group.throughput(Throughput::Elements(elements));
            let id = BenchmarkId::from_parameter(line_count);
            group.bench_with_input(id, &text, |bencher, text| {
                bencher.iter(|| {
                    let original = black_box(text.as_str());
                    let declared = black_box(hunks.as_slice());
                    plan(Some(original), declared)
                });
            });
        }
        group.finish();
    }
}

/// The hunks of a script naming one file.
fn parsed(hunk: &str) -> Vec<Hunk> {
    let text = format!("=== file\n{hunk}");
    let Ok(script) = parse(&text) else {
        unreachable!("every case's hunk is written in the grammar");
    };
    let Some(section) = script.sections.into_iter().next() else {
        unreachable!("a script naming a file holds its section");
    };
    assert!(!section.hunks.is_empty(), "a case declares a hunk");
    section.hunks
}

/// `count` items of three lines each, every item closed by a `}` line, then `LAST_LINE`.
fn items(count: usize) -> String {
    assert!(count > 0, "a file holds at least one item");
    let mut text = String::new();
    for index in 0..count {
        let item = format!("fn item_{index}() {{\n    let value = {index};\n}}\n");
        text.push_str(&item);
    }
    text.push_str(LAST_LINE);
    text.push('\n');
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
    targets = plan_by_length
}
criterion_main!(benches);
