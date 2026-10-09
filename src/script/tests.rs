//! The grammar's rules, each pinned by a script that obeys or breaks exactly one of
//! them. Every case parses a literal script and asserts on the typed script or on the
//! fault and the line it names, never on a message.

use std::num::NonZeroUsize;

use super::*;

fn non_zero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

fn fault_of(text: &str) -> (usize, Fault) {
    let error = parse(text).unwrap_err();
    (error.line, error.fault)
}

mod given_a_literal_hunk_with_context_removed_and_added_lines {
    use super::*;

    #[test]
    fn when_parsed_then_it_holds_the_block_in_order_under_the_once_expectation() {
        let script =
            parse("=== src/lib.rs\n@@\n fn a() {\n-    old();\n+    new();\n }\n").unwrap();

        assert_eq!(
            script,
            Script {
                sections: vec![Section {
                    paths: vec!["src/lib.rs".to_owned()],
                    hunks: vec![Hunk {
                        line: 2,
                        operation: Operation::Literal {
                            block: vec![
                                BlockLine::Context("fn a() {".to_owned()),
                                BlockLine::Remove("    old();".to_owned()),
                                BlockLine::Add("    new();".to_owned()),
                                BlockLine::Context("}".to_owned()),
                            ],
                            expectation: Expectation::Once,
                        },
                    }],
                }],
            }
        );
    }
}

mod given_hunk_headers_naming_an_expectation {
    use super::*;

    fn expectation_of(header: &str) -> Expectation {
        let script = parse(&format!("=== a\n{header}\n-x\n+y\n")).unwrap();
        match &script.sections[0].hunks[0].operation {
            Operation::Literal { expectation, .. } | Operation::Regex { expectation, .. } => {
                *expectation
            }
            other => panic!("not a matching hunk: {other:?}"),
        }
    }

    #[test]
    fn when_parsed_holding_all_then_the_expectation_is_all() {
        assert_eq!(expectation_of("@@ all"), Expectation::All);
    }

    #[test]
    fn when_parsed_holding_count_three_then_the_expectation_is_exactly_three() {
        assert_eq!(
            expectation_of("@@ count 3"),
            Expectation::Exactly(non_zero(3))
        );
    }

    #[test]
    fn when_parsed_holding_count_one_then_the_expectation_is_once() {
        assert_eq!(expectation_of("@@ count 1"), Expectation::Once);
    }

    #[test]
    fn when_parsed_holding_line_212_then_the_expectation_is_at_line_212() {
        assert_eq!(
            expectation_of("@@ line 212"),
            Expectation::AtLine(non_zero(212))
        );
    }

    #[test]
    fn when_parsed_holding_regex_all_then_the_regex_expectation_is_all() {
        assert_eq!(expectation_of("@@ regex all"), Expectation::All);
    }
}

mod given_header_words_outside_the_grammar {
    use super::*;

    #[test]
    fn when_parsed_holding_a_unified_diff_range_then_it_is_refused_as_an_unknown_header() {
        let (line, fault) = fault_of("=== a\n@@ -1,3 +1,4 @@\n-x\n+y\n");

        assert_eq!(line, 2);
        assert!(matches!(fault, Fault::UnknownHeader { .. }));
    }

    #[test]
    fn when_parsed_holding_count_zero_then_it_is_refused_as_an_invalid_number() {
        let (line, fault) = fault_of("=== a\n@@ count 0\n-x\n+y\n");

        assert_eq!(line, 2);
        assert!(matches!(fault, Fault::InvalidNumber { .. }));
    }

    #[test]
    fn when_parsed_holding_line_without_a_number_then_it_is_refused_as_an_unknown_header() {
        let (line, fault) = fault_of("=== a\n@@ line\n-x\n+y\n");

        assert_eq!(line, 2);
        assert!(matches!(fault, Fault::UnknownHeader { .. }));
    }
}

mod given_blank_lines_around_and_inside_a_hunk {
    use super::*;

    #[test]
    fn when_parsed_then_the_edges_are_dropped_and_an_inner_blank_is_an_empty_context_line() {
        let script = parse("=== a\n@@\n\n a\n\n-b\n\n@@\n-c\n+d\n").unwrap();

        assert_eq!(
            script.sections[0].hunks[0].operation,
            Operation::Literal {
                block: vec![
                    BlockLine::Context("a".to_owned()),
                    BlockLine::Context(String::new()),
                    BlockLine::Remove("b".to_owned()),
                ],
                expectation: Expectation::Once,
            }
        );
    }

    #[test]
    fn when_parsed_holding_a_lone_space_line_at_the_edge_then_it_is_kept_as_an_empty_context_line()
    {
        let script = parse("=== a\n@@\n-b\n \n").unwrap();

        assert_eq!(
            script.sections[0].hunks[0].operation,
            Operation::Literal {
                block: vec![
                    BlockLine::Remove("b".to_owned()),
                    BlockLine::Context(String::new())
                ],
                expectation: Expectation::Once,
            }
        );
    }
}

mod given_consecutive_file_headers {
    use super::*;

    #[test]
    fn when_parsed_then_the_hunks_that_follow_apply_to_every_file_of_the_group() {
        let script =
            parse("=== a.rs\n=== b.rs\n@@ all\n-old\n+new\n=== c.rs\n@@\n-x\n+y\n").unwrap();

        let paths: Vec<Vec<String>> = script
            .sections
            .iter()
            .map(|section| section.paths.clone())
            .collect();
        assert_eq!(
            paths,
            vec![
                vec!["a.rs".to_owned(), "b.rs".to_owned()],
                vec!["c.rs".to_owned()]
            ]
        );
    }

    #[test]
    fn when_parsed_naming_one_path_twice_then_the_second_naming_is_refused() {
        let (line, fault) = fault_of("=== a\n@@\n-x\n+y\n=== a\n@@\n-z\n+w\n");

        assert_eq!(line, 5);
        assert_eq!(fault, Fault::PathRepeated { first_line: 1 });
    }

    #[test]
    fn when_parsed_with_a_file_header_followed_by_no_hunk_then_it_is_refused_at_the_header() {
        let (line, fault) = fault_of("=== a\n@@\n-x\n+y\n=== b\n");

        assert_eq!(line, 5);
        assert_eq!(fault, Fault::FileWithoutHunks);
    }

    #[test]
    fn when_parsed_with_an_empty_path_then_it_is_refused_at_the_header() {
        let (line, fault) = fault_of("===   \n@@\n-x\n+y\n");

        assert_eq!(line, 1);
        assert_eq!(fault, Fault::EmptyPath);
    }
}

mod given_lines_outside_any_hunk {
    use super::*;

    #[test]
    fn when_parsed_with_text_before_the_first_file_then_it_is_refused_at_that_line() {
        assert_eq!(
            fault_of("hello\n=== a\n@@\n-x\n"),
            (1, Fault::LineBeforeFirstFile)
        );
    }

    #[test]
    fn when_parsed_with_a_body_line_between_a_file_and_its_first_hunk_then_it_is_refused_at_that_line()
     {
        assert_eq!(
            fault_of("=== a\n-x\n@@\n-x\n"),
            (2, Fault::LineBeforeFirstHunk)
        );
    }

    #[test]
    fn when_parsed_with_a_hunk_before_any_file_then_it_is_refused_at_the_header() {
        assert_eq!(fault_of("@@\n-x\n"), (1, Fault::LineBeforeFirstFile));
    }

    #[test]
    fn when_parsed_with_a_merge_marker_inside_a_hunk_then_it_is_refused_as_an_unexpected_line() {
        assert_eq!(
            fault_of("=== a\n@@\n-x\n=======\n+y\n"),
            (4, Fault::UnexpectedLine)
        );
    }

    #[test]
    fn when_parsed_holding_nothing_then_it_is_refused_as_empty() {
        assert_eq!(fault_of("\n\n"), (1, Fault::EmptyScript));
    }
}

mod given_literal_hunks_that_cannot_change_anything_as_written {
    use super::*;

    #[test]
    fn when_parsed_holding_only_added_lines_then_it_is_refused_as_unanchored() {
        assert_eq!(fault_of("=== a\n@@\n+x\n"), (2, Fault::NoAnchor));
    }

    #[test]
    fn when_parsed_holding_only_context_lines_then_it_is_refused_as_changing_nothing() {
        assert_eq!(fault_of("=== a\n@@\n x\n y\n"), (2, Fault::ChangesNothing));
    }
}

mod given_an_elision_line {
    use super::*;

    fn block_of(text: &str) -> Vec<BlockLine> {
        match parse(text)
            .unwrap()
            .sections
            .remove(0)
            .hunks
            .remove(0)
            .operation
        {
            Operation::Literal { block, .. } => block,
            other => panic!("not a literal hunk: {other:?}"),
        }
    }

    #[test]
    fn when_parsed_under_a_removed_line_then_the_elided_lines_are_removed() {
        let block = block_of("=== a\n@@\n-fn a() {\n~\n-}\n");

        assert_eq!(
            block,
            vec![
                BlockLine::Remove("fn a() {".to_owned()),
                BlockLine::ElideRemove,
                BlockLine::Remove("}".to_owned()),
            ]
        );
    }

    #[test]
    fn when_parsed_under_a_context_line_then_the_elided_lines_are_kept() {
        let block = block_of("=== a\n@@\n BEGIN\n~\n+new\n END\n");

        assert_eq!(
            block,
            vec![
                BlockLine::Context("BEGIN".to_owned()),
                BlockLine::ElideKeep,
                BlockLine::Add("new".to_owned()),
                BlockLine::Context("END".to_owned()),
            ]
        );
    }

    #[test]
    fn when_parsed_as_the_first_line_of_the_hunk_then_it_is_refused_at_the_elision() {
        assert_eq!(
            fault_of("=== a\n@@\n~\n-x\n"),
            (3, Fault::ElisionWithoutLineAbove)
        );
    }

    #[test]
    fn when_parsed_under_an_added_line_then_it_is_refused_at_the_elision() {
        assert_eq!(
            fault_of("=== a\n@@\n-x\n+y\n~\n-z\n"),
            (5, Fault::ElisionWithoutLineAbove)
        );
    }

    #[test]
    fn when_parsed_with_no_matched_line_below_then_it_is_refused_at_the_elision() {
        assert_eq!(
            fault_of("=== a\n@@\n-x\n~\n+y\n"),
            (4, Fault::ElisionWithoutLineBelow)
        );
    }

    #[test]
    fn when_parsed_holding_text_after_the_tilde_then_it_is_refused_as_an_unexpected_line() {
        assert_eq!(
            fault_of("=== a\n@@\n-x\n~ more\n-y\n"),
            (4, Fault::UnexpectedLine)
        );
    }
}

mod given_a_regex_hunk {
    use super::*;

    #[test]
    fn when_parsed_then_the_removed_lines_are_the_pattern_and_the_added_lines_the_replacement() {
        let script = parse("=== a\n@@ regex all\n-fn (\\w+)\\(\n+pub fn $1(\n").unwrap();

        assert_eq!(
            script.sections[0].hunks[0].operation,
            Operation::Regex {
                pattern: "fn (\\w+)\\(".to_owned(),
                replacement: "pub fn $1(".to_owned(),
                expectation: Expectation::All,
            }
        );
    }

    #[test]
    fn when_parsed_with_a_pattern_that_does_not_compile_then_it_is_refused_at_the_header() {
        let (line, fault) = fault_of("=== a\n@@ regex\n-(unclosed\n+x\n");

        assert_eq!(line, 2);
        assert!(matches!(fault, Fault::RegexInvalid { .. }));
    }

    #[test]
    fn when_parsed_holding_a_context_line_then_it_is_refused_at_that_line() {
        assert_eq!(
            fault_of("=== a\n@@ regex\n-x\n y\n+z\n"),
            (4, Fault::RegexWithContext)
        );
    }

    #[test]
    fn when_parsed_holding_no_removed_line_then_it_is_refused_as_without_a_pattern() {
        assert_eq!(
            fault_of("=== a\n@@ regex\n+z\n"),
            (2, Fault::RegexWithoutPattern)
        );
    }
}

mod given_append_and_create_hunks {
    use super::*;

    #[test]
    fn when_parsed_holding_append_then_the_added_lines_are_held_in_order() {
        let script = parse("=== a\n@@ append\n+one\n+two\n").unwrap();

        assert_eq!(
            script.sections[0].hunks[0].operation,
            Operation::Append {
                lines: vec!["one".to_owned(), "two".to_owned()]
            }
        );
    }

    #[test]
    fn when_parsed_holding_append_with_a_removed_line_then_it_is_refused_at_that_line() {
        assert_eq!(
            fault_of("=== a\n@@ append\n+one\n-two\n"),
            (4, Fault::OnlyAddedLines)
        );
    }

    #[test]
    fn when_parsed_holding_append_with_no_lines_then_it_is_refused_at_the_header() {
        assert_eq!(
            fault_of("=== a\n@@ append\n=== b\n@@\n-x\n+y\n"),
            (2, Fault::AppendEmpty)
        );
    }

    #[test]
    fn when_parsed_holding_append_jsonl_with_a_row_that_is_not_json_then_it_is_refused_at_that_row()
    {
        let (line, fault) = fault_of("=== a\n@@ append jsonl\n+{\"a\":1}\n+{\"a\":\n");

        assert_eq!(line, 4);
        assert!(matches!(fault, Fault::JsonlInvalid { .. }));
    }

    #[test]
    fn when_parsed_holding_append_jsonl_with_valid_rows_then_the_rows_are_held_as_lines() {
        let script = parse("=== a\n@@ append jsonl\n+{\"a\":1}\n").unwrap();

        assert_eq!(
            script.sections[0].hunks[0].operation,
            Operation::Append {
                lines: vec!["{\"a\":1}".to_owned()]
            }
        );
    }

    #[test]
    fn when_parsed_holding_create_beside_another_hunk_then_it_is_refused_at_the_create() {
        assert_eq!(
            fault_of("=== a\n@@\n-x\n+y\n@@ create\n+z\n"),
            (5, Fault::CreateNotAlone)
        );
    }

    #[test]
    fn when_parsed_holding_create_with_no_lines_then_it_holds_an_empty_file() {
        let script = parse("=== a\n@@ create\n").unwrap();

        assert_eq!(
            script.sections[0].hunks[0].operation,
            Operation::Create { lines: Vec::new() }
        );
    }
}
