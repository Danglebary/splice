//! The command line's grammar, each row an argument list and the command or fault it
//! parses to.

use super::*;

fn parsed(words: &[&str]) -> Result<Command, UsageFault> {
    let arguments: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    parse_arguments(&arguments)
}

mod given_no_subcommand {
    use super::*;

    #[test]
    fn when_parsed_holding_no_argument_then_it_applies_from_stdin_quietly() {
        assert_eq!(parsed(&[]), Ok(Command::Apply(Options::default())));
    }

    #[test]
    fn when_parsed_holding_diff_and_a_script_path_then_both_are_set() {
        let options = Options {
            diff: true,
            dry_run: false,
            script: Some("edits.splice".to_owned()),
        };

        assert_eq!(
            parsed(&["--diff", "--script", "edits.splice"]),
            Ok(Command::Apply(options))
        );
    }

    #[test]
    fn when_parsed_holding_script_without_a_path_then_it_is_refused() {
        assert_eq!(
            parsed(&["--script"]),
            Err(UsageFault::MissingValue("--script".to_owned()))
        );
    }

    #[test]
    fn when_parsed_holding_an_unknown_flag_then_it_is_refused() {
        assert_eq!(
            parsed(&["-i"]),
            Err(UsageFault::UnknownArgument("-i".to_owned()))
        );
    }

    #[test]
    fn when_parsed_holding_expect_fail_then_it_is_refused_outside_try() {
        assert_eq!(
            parsed(&["--expect-fail"]),
            Err(UsageFault::ExpectFailureOutsideTry)
        );
    }
}

mod given_the_try_subcommand {
    use super::*;

    #[test]
    fn when_parsed_with_a_program_after_the_separator_then_the_program_keeps_its_own_flags() {
        let command = parsed(&["try", "--expect-fail", "--", "cargo", "test", "--quiet"]);

        assert_eq!(
            command,
            Ok(Command::Try {
                options: Options::default(),
                expect_failure: true,
                program: vec!["cargo".to_owned(), "test".to_owned(), "--quiet".to_owned()],
            })
        );
    }

    #[test]
    fn when_parsed_without_a_program_then_it_is_refused() {
        assert_eq!(parsed(&["try", "--"]), Err(UsageFault::MissingProgram));
    }

    #[test]
    fn when_parsed_holding_dry_run_then_it_is_refused() {
        assert_eq!(
            parsed(&["try", "--dry-run", "--", "true"]),
            Err(UsageFault::DryRunUnderTry)
        );
    }
}
