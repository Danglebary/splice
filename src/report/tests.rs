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
