//! The messages are the output under test here, so these cases read the rendered text.
//! Each pins one property a reader relies on, never a whole sentence: which lines a
//! message names, and that whitespace a near miss turns on is visible.

use std::num::NonZeroUsize;

use super::*;

mod given_a_refusal_naming_where_the_hunk_matched {
    use super::*;

    #[test]
    fn when_rendered_then_it_names_the_file_the_hunk_and_every_matched_line() {
        let refusal = Refusal {
            hunk_line: 7,
            reason: Reason::Count {
                expected: Expectation::Once,
                found: 2,
                found_lines: vec![3, 9],
                near_miss: None,
            },
        };

        let message = super::refusal("src/lib.rs", &refusal);

        assert!(message.starts_with("splice: src/lib.rs: hunk at script line 7: "));
        assert!(message.contains("at lines 3, 9"));
    }

    #[test]
    fn when_rendered_with_more_matches_than_listed_then_it_says_there_are_more() {
        let found_lines: Vec<usize> = (1..=10).collect();
        let expected = Expectation::Exactly(NonZeroUsize::new(2).unwrap());
        let refusal = Refusal {
            hunk_line: 1,
            reason: Reason::Count {
                expected,
                found: 40,
                found_lines,
                near_miss: None,
            },
        };

        assert!(super::refusal("f", &refusal).contains(", and more"));
    }
}

mod given_a_near_miss_differing_in_whitespace {
    use super::*;

    #[test]
    fn when_rendered_then_a_tab_shows_as_an_escape_beside_the_spaces_the_hunk_held() {
        let near_miss = NearMiss::Differs {
            file_line: 2,
            expected: "    body();".to_owned(),
            found: Some("\tbody();".to_owned()),
        };
        let refusal = Refusal {
            hunk_line: 2,
            reason: Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(near_miss),
            },
        };

        let message = super::refusal("f", &refusal);

        assert!(message.contains("\"    body();\""));
        assert!(message.contains("\"\\tbody();\""));
    }
}

mod given_a_refusal_whose_hunks_only_add_a_final_newline {
    use super::*;

    #[test]
    fn when_rendered_then_it_names_the_header_that_adds_one() {
        let refusal = Refusal {
            hunk_line: 2,
            reason: Reason::OnlyAddsFinalNewline,
        };

        assert!(super::refusal("f", &refusal).contains("`@@ final newline`"));
    }
}

mod given_a_near_miss_at_a_line_ending_in_a_carriage_return {
    use super::*;

    #[test]
    fn when_rendered_then_it_names_the_header_that_matches_the_text_before_it() {
        let refusal = Refusal {
            hunk_line: 2,
            reason: Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(NearMiss::CarriageReturn { file_line: 1 }),
            },
        };

        assert!(super::refusal("f", &refusal).contains("`@@ inline`"));
    }
}

mod given_a_script_error {
    use super::*;

    #[test]
    fn when_rendered_then_it_names_the_script_line() {
        let error = ScriptError {
            line: 12,
            fault: Fault::UnexpectedLine,
        };

        assert!(script_error(&error).starts_with("splice: script line 12: "));
    }
}

mod given_a_denied_command {
    use super::*;

    #[test]
    fn when_rendered_then_it_carries_a_runnable_splice_example() {
        let message = guard_denial(Offense::SedInPlace);

        assert!(message.contains("splice <<'EOF'"));
        assert!(message.contains("`sed -i`"));
    }
}

mod given_a_fault_an_agent_fixes_by_moving_its_added_lines {
    use super::*;

    #[test]
    fn when_a_context_only_hunk_is_rendered_then_it_says_where_the_added_lines_go() {
        let error = ScriptError {
            line: 4,
            fault: Fault::OnlyContextLines,
        };

        let message = script_error(&error);

        assert!(message.contains("script line 4"));
        assert!(message.contains("`+` lines directly under"));
    }

    #[test]
    fn when_an_elision_under_an_added_line_is_rendered_then_it_names_the_added_line() {
        let error = ScriptError {
            line: 4,
            fault: Fault::ElisionWithoutLineAbove,
        };

        let message = script_error(&error);

        assert!(message.contains("never under a `+` line"));
    }
}

mod given_a_near_miss_found_inside_a_line {
    use super::*;

    #[test]
    fn when_rendered_then_it_names_the_line_and_the_inline_header() {
        let refusal = Refusal {
            hunk_line: 2,
            reason: Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(NearMiss::Inside { file_line: 7 }),
            },
        };

        let message = super::refusal("f", &refusal);

        assert!(message.contains("file line 7"));
        assert!(message.contains("`@@ inline`"));
    }
}

mod given_a_script_that_landed {
    use super::*;

    fn landed_of(hunk_count: usize, file_count: usize) -> Landed {
        Landed {
            hunk_count: NonZeroUsize::new(hunk_count).unwrap(),
            file_count: NonZeroUsize::new(file_count).unwrap(),
        }
    }

    #[test]
    fn when_rendered_then_it_is_one_line_under_the_splice_prefix() {
        let message = landed(&landed_of(3, 2));

        assert!(message.starts_with("splice: "));
        assert_eq!(message.lines().count(), 1);
        assert!(message.ends_with('\n'));
    }

    #[test]
    fn when_rendered_then_it_counts_the_hunks_and_the_files_in_the_right_number() {
        let cases = [
            (1, 1, "1 hunk written", "to 1 file\n"),
            (3, 1, "3 hunks written", "to 1 file\n"),
            (1, 2, "1 hunk written", "to 2 files\n"),
        ];
        for (hunk_count, file_count, hunks, files) in cases {
            let message = landed(&landed_of(hunk_count, file_count));

            assert!(message.contains(hunks), "{message:?} lacks {hunks:?}");
            assert!(message.contains(files), "{message:?} lacks {files:?}");
        }
    }
}
