//! Matching and splicing, each rule pinned by one file text and one script. The script is
//! written in the grammar and parsed, so a case reads as an agent would write it, and
//! every case asserts on the new text or on the refusal's reason, never on a message.

use std::num::NonZeroUsize;

use super::*;
use crate::script::{Expectation, parse};

fn run(original: Option<&str>, hunks: &str) -> Result<String, Vec<Refusal>> {
    let script = parse(&format!("=== file\n{hunks}")).unwrap();
    plan(original, &script.sections[0].hunks)
}

fn applied(original: &str, hunks: &str) -> String {
    run(Some(original), hunks).unwrap()
}

fn refused(original: Option<&str>, hunks: &str) -> Vec<Reason> {
    run(original, hunks)
        .unwrap_err()
        .into_iter()
        .map(|refusal| refusal.reason)
        .collect()
}

fn non_zero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

mod given_a_hunk_matching_once {
    use super::*;

    #[test]
    fn when_planned_then_the_removed_line_is_replaced_and_the_context_kept() {
        let text = applied(
            "fn a() {\n    old();\n}\n",
            "@@\n fn a() {\n-    old();\n+    new();\n }\n",
        );

        assert_eq!(text, "fn a() {\n    new();\n}\n");
    }

    #[test]
    fn when_planned_over_a_crlf_file_then_the_replacement_keeps_crlf() {
        let text = applied("a\r\nb\r\nc\r\n", "@@\n a\n-b\n+B\n");

        assert_eq!(text, "a\r\nB\r\nc\r\n");
    }

    #[test]
    fn when_planned_over_a_file_mixing_crlf_and_lf_then_each_line_keeps_its_own_ending() {
        let text = applied("a\r\nb\nc\r\n", "@@\n-b\n+B\n");

        assert_eq!(text, "a\r\nB\nc\r\n");
    }

    #[test]
    fn when_planned_at_the_last_line_of_a_crlf_file_without_a_final_newline_then_none_is_added() {
        let text = applied("a\r\nb", "@@\n a\n-b\n+B\n");

        assert_eq!(text, "a\r\nB");
    }

    #[test]
    fn when_planned_over_crlf_lines_below_a_newline_at_byte_zero_then_added_lines_end_in_lf() {
        let text = applied("\nb\r\n", "@@\n \n+x\n");

        assert_eq!(text, "\nx\nb\r\n");
    }

    #[test]
    fn when_planned_at_the_last_line_of_a_file_without_a_final_newline_then_none_is_added() {
        let text = applied("a\nb", "@@\n a\n-b\n+B\n");

        assert_eq!(text, "a\nB");
    }

    #[test]
    fn when_planned_to_remove_the_last_line_of_a_file_without_a_final_newline_then_none_is_left() {
        let text = applied("a\nb", "@@\n a\n-b\n");

        assert_eq!(text, "a");
    }

    #[test]
    fn when_planned_with_content_holding_regex_and_shell_characters_then_it_matches_literally() {
        let text = applied(
            "let f = |x| x.0 & '$1';\n",
            "@@\n-let f = |x| x.0 & '$1';\n+let f = |y| y.1;\n",
        );

        assert_eq!(text, "let f = |y| y.1;\n");
    }
}

mod given_a_hunk_declared_once_that_matches_twice {
    use super::*;

    #[test]
    fn when_planned_then_it_is_refused_naming_both_lines() {
        let reasons = refused(Some("x\ny\nx\n"), "@@\n-x\n+z\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 2,
                found_lines: vec![1, 3],
                near_miss: None
            }]
        );
    }
}

mod given_a_hunk_matching_nowhere {
    use super::*;

    #[test]
    fn when_planned_after_some_lines_match_then_the_near_miss_names_the_first_differing_line() {
        let reasons = refused(
            Some("fn a() {\n\tbody();\n}\n"),
            "@@\n fn a() {\n-    body();\n+    other();\n",
        );

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(NearMiss::Differs {
                    file_line: 2,
                    expected: "    body();".to_owned(),
                    found: Some("\tbody();".to_owned()),
                }),
            }]
        );
    }

    #[test]
    fn when_planned_with_a_first_line_differing_in_indentation_then_the_near_miss_names_it() {
        let reasons = refused(Some("a\n\tlet x = 1;\n"), "@@\n-let x = 1;\n+let x = 2;\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(NearMiss::Whitespace { file_line: 2 }),
            }]
        );
    }

    #[test]
    fn when_planned_against_a_block_running_past_the_end_then_the_near_miss_finds_nothing_there() {
        let reasons = refused(Some("a\nb\n"), "@@\n a\n b\n-c\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(NearMiss::Differs {
                    file_line: 3,
                    expected: "c".to_owned(),
                    found: None
                }),
            }]
        );
    }
}

mod given_hunks_declaring_how_many_matches {
    use super::*;

    #[test]
    fn when_planned_holding_all_then_every_occurrence_is_replaced() {
        let text = applied("x\ny\nx\nx\n", "@@ all\n-x\n+z\n");

        assert_eq!(text, "z\ny\nz\nz\n");
    }

    #[test]
    fn when_planned_holding_all_over_no_occurrence_then_it_is_refused() {
        let reasons = refused(Some("y\n"), "@@ all\n-x\n+z\n");

        assert!(matches!(
            reasons.as_slice(),
            [Reason::Count {
                expected: Expectation::All,
                found: 0,
                ..
            }]
        ));
    }

    #[test]
    fn when_planned_holding_count_two_over_three_occurrences_then_it_is_refused() {
        let reasons = refused(Some("x\nx\nx\n"), "@@ count 2\n-x\n+z\n");

        assert!(matches!(
            reasons.as_slice(),
            [Reason::Count { found: 3, .. }]
        ));
    }

    #[test]
    fn when_planned_holding_count_two_over_two_occurrences_then_both_are_replaced() {
        let text = applied("x\ny\nx\n", "@@ count 2\n-x\n+z\n");

        assert_eq!(text, "z\ny\nz\n");
    }
}

mod given_a_hunk_anchored_at_a_line {
    use super::*;

    #[test]
    fn when_planned_over_a_match_at_that_line_then_only_that_occurrence_changes() {
        let text = applied("x\nx\nx\n", "@@ line 2\n-x\n+z\n");

        assert_eq!(text, "x\nz\nx\n");
    }

    #[test]
    fn when_planned_over_matches_at_other_lines_alone_then_it_is_refused_naming_them() {
        let reasons = refused(Some("x\ny\nx\n"), "@@ line 2\n-x\n+z\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::AtLine(non_zero(2)),
                found: 2,
                found_lines: vec![1, 3],
                near_miss: None,
            }]
        );
    }
}

mod given_an_elision {
    use super::*;

    #[test]
    fn when_planned_under_a_removed_line_then_the_whole_span_is_removed() {
        let text = applied(
            "keep\nfn a() {\n    one();\n    two();\n}\nafter\n",
            "@@\n-fn a() {\n~\n-}\n",
        );

        assert_eq!(text, "keep\nafter\n");
    }

    #[test]
    fn when_planned_under_a_context_line_then_the_span_is_kept_and_the_addition_lands_after_it() {
        let text = applied("BEGIN\none\ntwo\nEND\n", "@@\n BEGIN\n~\n+three\n END\n");

        assert_eq!(text, "BEGIN\none\ntwo\nthree\nEND\n");
    }

    #[test]
    fn when_planned_replacing_a_region_between_anchors_then_the_anchors_stay() {
        let text = applied(
            "// BEGIN\nold one\nold two\n// END\n",
            "@@\n // BEGIN\n-old one\n~\n+new\n // END\n",
        );

        assert_eq!(text, "// BEGIN\nnew\n// END\n");
    }

    #[test]
    fn when_planned_ending_at_the_first_closing_line_then_the_span_stops_there() {
        let text = applied("fn a() {\n}\nfn b() {\n}\n", "@@\n-fn a() {\n~\n-}\n");

        assert_eq!(text, "fn b() {\n}\n");
    }

    #[test]
    fn when_planned_with_no_closing_line_below_the_opening_then_the_near_miss_names_the_opening() {
        let reasons = refused(Some("x\nfn a() {\nbody\n"), "@@\n-fn a() {\n~\n-}\n");

        assert!(matches!(
            reasons.as_slice(),
            [Reason::Count {
                near_miss: Some(NearMiss::AfterElision { file_line: 2 }),
                ..
            }]
        ));
    }

    #[test]
    fn when_planned_from_two_openings_above_one_closing_line_then_it_is_refused_naming_both() {
        let reasons = refused(Some("open\nopen\nbody\nclose\n"), "@@\n-open\n~\n-close\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 2,
                found_lines: vec![1, 2],
                near_miss: None
            }]
        );
    }

    #[test]
    fn when_planned_from_an_opening_below_the_only_closing_line_then_it_matches_nothing() {
        let text = applied("open\none\nclose\nopen\ntwo\n", "@@\n-open\n~\n-close\n");

        assert_eq!(text, "open\ntwo\n");
    }

    #[test]
    fn when_planned_holding_all_over_two_blocks_then_each_block_is_replaced() {
        let text = applied(
            "open\none\nclose\nopen\ntwo\nclose\n",
            "@@ all\n-open\n~\n-close\n+gone\n",
        );

        assert_eq!(text, "gone\ngone\n");
    }

    #[test]
    fn when_planned_with_two_elisions_then_each_segment_is_found_below_the_one_before() {
        let text = applied("b\na\nx\nb\ny\nc\n", "@@\n a\n~\n b\n~\n-c\n+C\n");

        assert_eq!(text, "b\na\nx\nb\ny\nC\n");
    }
}

mod given_a_regex_hunk {
    use super::*;

    #[test]
    fn when_planned_holding_all_then_every_match_is_replaced_with_its_groups_expanded() {
        let text = applied(
            "fn alpha(\nfn beta(\n",
            "@@ regex all\n-fn (\\w+)\\(\n+pub fn ${1}_v2(\n",
        );

        assert_eq!(text, "pub fn alpha_v2(\npub fn beta_v2(\n");
    }

    #[test]
    fn when_planned_declared_once_over_two_matches_then_it_is_refused_naming_their_lines() {
        let reasons = refused(Some("a1\nb\na2\n"), "@@ regex\n-a\\d\n+c\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 2,
                found_lines: vec![1, 3],
                near_miss: None
            }]
        );
    }

    #[test]
    fn when_planned_with_a_replacement_equal_to_every_match_then_it_is_refused_as_unchanged() {
        let reasons = refused(Some("abc\n"), "@@ regex\n-b\n+b\n");

        assert_eq!(reasons, vec![Reason::Unchanged]);
    }
}

mod given_an_inline_hunk {
    use super::*;

    #[test]
    fn when_planned_matching_once_mid_line_then_only_that_text_changes() {
        let text = applied(
            "The store keeps every event.\n",
            "@@ inline\n-keeps every event\n+keeps every event in order\n",
        );

        assert_eq!(text, "The store keeps every event in order.\n");
    }

    #[test]
    fn when_planned_with_text_holding_regex_characters_then_it_matches_literally() {
        let text = applied("let y = f(x) * $1.0;\n", "@@ inline\n-f(x) * $1.0\n+g(x)\n");

        assert_eq!(text, "let y = g(x);\n");
    }

    #[test]
    fn when_planned_holding_all_then_every_occurrence_changes() {
        let text = applied("a cat, a cat\nthe cat\n", "@@ inline all\n-cat\n+dog\n");

        assert_eq!(text, "a dog, a dog\nthe dog\n");
    }

    #[test]
    fn when_planned_holding_count_two_over_two_occurrences_then_both_change() {
        let text = applied("x.a + x.a\n", "@@ inline count 2\n-x.a\n+y.b\n");

        assert_eq!(text, "y.b + y.b\n");
    }

    #[test]
    fn when_planned_once_over_two_occurrences_then_it_is_refused_naming_their_lines() {
        let reasons = refused(Some("cat\nthe cat\n"), "@@ inline\n-cat\n+dog\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 2,
                found_lines: vec![1, 2],
                near_miss: None,
            }]
        );
    }

    #[test]
    fn when_planned_once_over_overlapping_occurrences_then_it_is_refused_as_ambiguous() {
        let reasons = refused(Some("aaa\n"), "@@ inline\n-aa\n+b\n");

        assert!(matches!(
            reasons.as_slice(),
            [Reason::Count { found: 2, .. }]
        ));
    }

    #[test]
    fn when_planned_with_text_spanning_lines_then_the_lines_join_at_the_line_break() {
        let text = applied("one two\nthree four\n", "@@ inline\n-two\n-three\n+2\n+3\n");

        assert_eq!(text, "one 2\n3 four\n");
    }

    #[test]
    fn when_planned_with_text_spanning_lines_of_a_crlf_file_then_the_lines_join_at_crlf() {
        let text = applied("a b\r\nc d\r\n", "@@ inline\n-b\n-c\n+B\n+C\n");

        assert_eq!(text, "a B\r\nC d\r\n");
    }

    #[test]
    fn when_planned_with_no_added_line_then_the_text_is_removed() {
        let text = applied("keep this, drop this.\n", "@@ inline\n-, drop this\n");

        assert_eq!(text, "keep this.\n");
    }

    #[test]
    fn when_planned_against_a_misquoted_word_then_the_near_miss_shows_both_from_where_they_part() {
        let reasons = refused(
            Some("The store keeps every event in its log.\n"),
            "@@ inline\n-keeps every evnt in its log\n+x\n",
        );

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: Some(NearMiss::Diverges {
                    file_line: 1,
                    expected: "nt in its log".to_owned(),
                    found: "ent in its log.\n".to_owned(),
                }),
            }]
        );
    }

    #[test]
    fn when_planned_against_text_sharing_too_short_a_prefix_then_no_near_miss_is_named() {
        let reasons = refused(Some("abc\n"), "@@ inline\n-xyz123\n+q\n");

        assert_eq!(
            reasons,
            vec![Reason::Count {
                expected: Expectation::Once,
                found: 0,
                found_lines: Vec::new(),
                near_miss: None,
            }]
        );
    }
}

mod given_an_append_hunk {
    use super::*;

    #[test]
    fn when_planned_over_a_file_ending_in_a_newline_then_the_lines_follow_it() {
        assert_eq!(applied("a\n", "@@ append\n+b\n+c\n"), "a\nb\nc\n");
    }

    #[test]
    fn when_planned_over_a_file_without_a_final_newline_then_the_lines_start_on_a_new_line() {
        assert_eq!(applied("a", "@@ append\n+b\n"), "a\nb");
    }

    #[test]
    fn when_planned_over_an_empty_file_then_the_file_holds_the_lines_and_a_final_newline() {
        assert_eq!(applied("", "@@ append\n+b\n"), "b\n");
    }

    #[test]
    fn when_planned_over_a_crlf_file_then_the_lines_end_in_crlf() {
        assert_eq!(applied("a\r\n", "@@ append\n+b\n"), "a\r\nb\r\n");
    }
}

mod given_a_file_that_does_not_exist {
    use super::*;

    #[test]
    fn when_planned_with_create_then_the_text_holds_the_lines_each_ending_in_a_newline() {
        assert_eq!(run(None, "@@ create\n+a\n+b\n").unwrap(), "a\nb\n");
    }

    #[test]
    fn when_planned_with_a_literal_hunk_then_it_is_refused_as_missing() {
        assert_eq!(refused(None, "@@\n-a\n+b\n"), vec![Reason::Missing]);
    }
}

mod given_a_file_that_exists {
    use super::*;

    #[test]
    fn when_planned_with_create_then_it_is_refused_as_existing() {
        assert_eq!(refused(Some(""), "@@ create\n+a\n"), vec![Reason::Exists]);
    }
}

mod given_several_hunks_in_one_file {
    use super::*;

    #[test]
    fn when_planned_then_every_hunk_matches_against_the_original_text() {
        let text = applied("a\nb\nc\n", "@@\n-a\n+b\n@@\n-b\n+c\n");

        assert_eq!(text, "b\nc\nc\n");
    }

    #[test]
    fn when_planned_with_two_hunks_claiming_one_line_then_the_later_is_refused_as_overlapping() {
        let reasons = refused(Some("a\nb\n"), "@@\n-a\n+x\n@@\n a\n-b\n+y\n");

        assert_eq!(reasons, vec![Reason::Overlap { other_hunk_line: 2 }]);
    }

    #[test]
    fn when_planned_with_two_failing_hunks_then_both_refusals_are_returned() {
        let reasons = refused(Some("a\n"), "@@\n-x\n+y\n@@\n-z\n+w\n");

        assert_eq!(reasons.len(), 2);
    }
}
