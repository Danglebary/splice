//! The hook's verdict over commands shaped like the ones agents ran: each row is one
//! Bash command and the verdict it must get. The blocked rows are the in-place editors
//! and interpreter writes found in the transcripts; the allowed rows are reads, saved
//! output, and text that only mentions an editor, which the verdict must never block.

use super::*;

fn verdict_of(command: &str) -> Verdict {
    verdict(command)
}

mod given_an_in_place_editor {
    use super::*;

    #[test]
    fn when_judged_holding_sed_dash_i_then_it_is_denied_as_sed() {
        assert_eq!(
            verdict_of("sed -i 's/a/b/' src/lib.rs"),
            Verdict::Deny(Offense::SedInPlace)
        );
    }

    #[test]
    fn when_judged_holding_sed_with_a_backup_suffix_then_it_is_denied_as_sed() {
        assert_eq!(
            verdict_of("sed -i.bak -e 's/a/b/' f"),
            Verdict::Deny(Offense::SedInPlace)
        );
    }

    #[test]
    fn when_judged_holding_sed_with_i_inside_a_flag_cluster_then_it_is_denied_as_sed() {
        assert_eq!(
            verdict_of("sed -Ei 's/a/b/' f"),
            Verdict::Deny(Offense::SedInPlace)
        );
    }

    #[test]
    fn when_judged_holding_sed_long_in_place_after_cd_then_it_is_denied_as_sed() {
        assert_eq!(
            verdict_of("cd /x && sed --in-place 's/a/b/' f"),
            Verdict::Deny(Offense::SedInPlace)
        );
    }

    #[test]
    fn when_judged_holding_sed_under_xargs_then_it_is_denied_as_sed() {
        assert_eq!(
            verdict_of("grep -rl old src | xargs -0 sed -i 's/old/new/g'"),
            Verdict::Deny(Offense::SedInPlace)
        );
    }

    #[test]
    fn when_judged_holding_perl_dash_zero_pi_then_it_is_denied_as_perl() {
        assert_eq!(
            verdict_of("perl -0pi -e 's/a/b/' f"),
            Verdict::Deny(Offense::PerlInPlace)
        );
    }

    #[test]
    fn when_judged_holding_perl_with_a_program_spanning_lines_then_it_is_denied_as_perl() {
        assert_eq!(
            verdict_of("perl -0pi -e 's/a\n  b/c\n  d/' f && cargo test"),
            Verdict::Deny(Offense::PerlInPlace)
        );
    }

    #[test]
    fn when_judged_holding_perl_dash_i_dash_pe_then_it_is_denied_as_perl() {
        assert_eq!(
            verdict_of("perl -i -pe 's/a/b/' f"),
            Verdict::Deny(Offense::PerlInPlace)
        );
    }

    #[test]
    fn when_judged_holding_gawk_inplace_then_it_is_denied_as_awk() {
        assert_eq!(
            verdict_of("gawk -i inplace '{print}' f"),
            Verdict::Deny(Offense::AwkInPlace)
        );
    }
}

mod given_an_interpreter_writing_a_file {
    use super::*;

    #[test]
    fn when_judged_holding_python_from_a_heredoc_calling_write_text_then_it_is_denied_as_python() {
        let command = "python3 - <<'EOF'\nfrom pathlib import Path\np = Path('f')\np.write_text(p.read_text().replace('a', 'b'))\nEOF";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::PythonWrite));
    }

    #[test]
    fn when_judged_holding_python_dash_c_opening_for_write_then_it_is_denied_as_python() {
        assert_eq!(
            verdict_of("python3 -c \"open('f', 'w').write('x')\""),
            Verdict::Deny(Offense::PythonWrite)
        );
    }

    #[test]
    fn when_judged_holding_node_calling_write_file_sync_then_it_is_denied_as_node() {
        assert_eq!(
            verdict_of("node -e \"require('fs').writeFileSync('f', 'x')\""),
            Verdict::Deny(Offense::NodeWrite)
        );
    }

    #[test]
    fn when_judged_holding_a_heredoc_written_to_a_script_python_then_runs_then_it_is_denied() {
        let command = "cat > /tmp/fix.py <<'EOF'\nfrom pathlib import Path\nPath('f').write_text('x')\nEOF\npython3 /tmp/fix.py";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::PythonWrite));
    }

    #[test]
    fn when_judged_holding_a_heredoc_written_to_a_quoted_script_path_python_runs_then_it_is_denied()
    {
        let command = "cat > \"$S/fix.py\" <<'EOF'\nimport os\nos.replace('a', 'b')\nEOF\npython3 \"$S/fix.py\"";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::PythonWrite));
    }
}

mod given_an_interpreter_beside_code_it_does_not_run {
    use super::*;

    #[test]
    fn when_judged_holding_a_test_file_written_by_a_heredoc_then_run_by_pytest_then_it_is_allowed()
    {
        let command = "cat > tests/test_io.py <<'EOF'\ndef test_roundtrip(tmp_path):\n    with open(tmp_path / 'f', 'w') as f:\n        f.write('x')\nEOF\npython3 -m pytest tests/test_io.py";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_python_beside_a_grep_for_a_write_call_then_it_is_allowed() {
        assert_eq!(
            verdict_of("python3 -m pytest && grep -rn 'os.rename(' src/"),
            Verdict::Allow
        );
    }

    #[test]
    fn when_judged_holding_node_beside_a_commit_message_naming_a_write_call_then_it_is_allowed() {
        assert_eq!(
            verdict_of("node build.js && git commit -m \"Cache through writeFileSync(\""),
            Verdict::Allow
        );
    }
}

mod given_a_rewrite_through_a_second_file {
    use super::*;

    #[test]
    fn when_judged_holding_awk_into_a_temporary_moved_over_the_original_then_it_is_denied() {
        let command = "awk '{print}' f.nix > f.nix.new && mv f.nix.new f.nix";

        assert_eq!(
            verdict_of(command),
            Verdict::Deny(Offense::MoveOverOriginal)
        );
    }

    #[test]
    fn when_judged_holding_a_mktemp_variable_moved_over_the_original_then_it_is_denied() {
        let command = "tmp=$(mktemp) && sed 's/a/b/' f > \"$tmp\" && mv \"$tmp\" f";

        assert_eq!(
            verdict_of(command),
            Verdict::Deny(Offense::MoveOverOriginal)
        );
    }

    #[test]
    fn when_judged_holding_sponge_then_it_is_denied_as_sponge() {
        assert_eq!(
            verdict_of("sed 's/a/b/' f | sponge f"),
            Verdict::Deny(Offense::Sponge)
        );
    }

    #[test]
    fn when_judged_holding_a_pipeline_into_a_temporary_moved_over_its_input_then_it_is_denied() {
        let command = "grep -v x f | sort > f.tmp && mv f.tmp f";

        assert_eq!(
            verdict_of(command),
            Verdict::Deny(Offense::MoveOverOriginal)
        );
    }

    #[test]
    fn when_judged_holding_jq_into_a_temporary_moved_over_the_original_then_it_is_allowed() {
        let command = "jq '.a = 1' f.json > f.json.tmp && mv f.json.tmp f.json";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_jq_into_sponge_then_it_is_allowed() {
        assert_eq!(verdict_of("jq . f.json | sponge f.json"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_download_moved_into_place_then_it_is_allowed() {
        let command =
            "curl -sSL https://example.com/x.tar.gz > x.tar.gz.part && mv x.tar.gz.part x.tar.gz";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_download_to_quoted_paths_moved_into_place_then_it_is_allowed() {
        let command = "curl -sSL \"$url\" > \"$out.part\" && mv \"$out.part\" \"$out\"";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_log_moved_into_a_directory_then_it_is_allowed() {
        assert_eq!(
            verdict_of("cargo build > build.log && mv build.log logs/"),
            Verdict::Allow
        );
    }
}

mod given_an_editor_inside_shell_syntax {
    use super::*;

    #[test]
    fn when_judged_holding_perl_inside_a_for_loop_then_it_is_denied_as_perl() {
        let command = "for f in a.rs b.rs; do perl -0pi -e 's/a/b/' $f; done";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::PerlInPlace));
    }

    #[test]
    fn when_judged_holding_sed_after_if_then_it_is_denied_as_sed() {
        let command = "if grep -q x f; then sed -i 's/x/y/' f; fi";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::SedInPlace));
    }

    #[test]
    fn when_judged_holding_a_script_written_to_a_sh_file_then_its_body_is_judged() {
        let command =
            "cat > $S/mutate.sh <<'EOF'\nsed -i \"$2\" \"$1\"\nEOF\nbash $S/mutate.sh f 's/a/b/'";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::SedInPlace));
    }

    #[test]
    fn when_judged_holding_a_script_written_to_a_quoted_sh_path_then_its_body_is_judged() {
        let command = "cat > \"$S/mutate.sh\" <<'EOF'\nperl -0pi -e \"$3\" \"$2\"\nEOF\nbash \"$S/mutate.sh\"";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::PerlInPlace));
    }

    #[test]
    fn when_judged_holding_a_heredoc_fed_to_bash_then_its_body_is_judged() {
        let command = "bash <<'EOF'\nperl -pi -e 's/a/b/' f\nEOF";

        assert_eq!(verdict_of(command), Verdict::Deny(Offense::PerlInPlace));
    }

    #[test]
    fn when_judged_holding_a_markdown_heredoc_naming_sed_then_it_is_allowed() {
        let command = "cat > notes.md <<'EOF'\nsed -i 's/a/b/' f\nEOF";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }
}

mod given_a_command_that_reads_or_saves_output {
    use super::*;

    #[test]
    fn when_judged_holding_sed_dash_n_then_it_is_allowed() {
        assert_eq!(verdict_of("sed -n '1,20p' f"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_sed_redirected_to_a_new_file_then_it_is_allowed() {
        assert_eq!(verdict_of("sed 's/a/b/' f > out.txt"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_perl_dash_ne_then_it_is_allowed() {
        assert_eq!(verdict_of("perl -ne 'print if /x/' f"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_perl_loading_a_module_whose_name_holds_an_i_then_it_is_allowed() {
        assert_eq!(verdict_of("perl -Mstrict -e 'print 1'"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_awk_printing_then_it_is_allowed() {
        assert_eq!(verdict_of("awk '{print $1}' f"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_jq_saved_to_scratch_then_it_is_allowed() {
        assert_eq!(verdict_of("jq '.a' f.json > /tmp/out.json"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_python_reading_a_file_then_it_is_allowed() {
        assert_eq!(
            verdict_of("python3 -c 'print(open(\"f\").read())'"),
            Verdict::Allow
        );
    }

    #[test]
    fn when_judged_holding_python_writing_to_stdout_then_it_is_allowed() {
        assert_eq!(
            verdict_of("python3 - <<'EOF'\nimport sys\nsys.stdout.write('x')\nEOF"),
            Verdict::Allow
        );
    }
}

mod given_a_hook_input {
    use super::*;

    #[test]
    fn when_read_holding_a_bash_call_then_the_command_is_returned() {
        let input = br#"{"tool_name":"Bash","tool_input":{"command":"ls","description":"List"}}"#;

        assert_eq!(hook_input(input), Ok(HookInput::Bash("ls".to_owned())));
    }

    #[test]
    fn when_read_holding_another_tool_then_it_is_another_tool() {
        let input = br#"{"tool_name":"Edit","tool_input":{"file_path":"a"}}"#;

        assert_eq!(hook_input(input), Ok(HookInput::OtherTool));
    }

    #[test]
    fn when_read_holding_a_bash_call_without_a_command_then_it_is_refused() {
        assert_eq!(
            hook_input(br#"{"tool_name":"Bash","tool_input":{}}"#),
            Err(HookInputFault::NoCommand)
        );
    }

    #[test]
    fn when_read_holding_text_that_is_not_json_then_it_is_refused() {
        assert!(matches!(
            hook_input(b"sed -i"),
            Err(HookInputFault::NotJson(_))
        ));
    }
}

mod given_text_that_only_mentions_an_editor {
    use super::*;

    #[test]
    fn when_judged_holding_a_heredoc_payload_naming_sed_then_it_is_allowed() {
        let command = "cat > /tmp/payload.json <<'EOF'\n{\"cmd\": \"sed -i s/a/b/ f\"}\nEOF";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_splice_script_whose_content_names_sed_then_it_is_allowed() {
        let command =
            "splice <<'EOF'\n=== doc.md\n@@\n-Run sed -i to edit.\n+Run splice to edit.\nEOF";

        assert_eq!(verdict_of(command), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_commit_message_naming_sed_then_it_is_allowed() {
        assert_eq!(
            verdict_of("git commit -m \"Replace sed -i with splice\""),
            Verdict::Allow
        );
    }

    #[test]
    fn when_judged_holding_a_quoted_grep_pattern_naming_sed_then_it_is_allowed() {
        assert_eq!(verdict_of("grep -n 'sed -i' notes.md"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_comment_naming_sed_then_it_is_allowed() {
        assert_eq!(verdict_of("# sed -i is blocked\nls"), Verdict::Allow);
    }

    #[test]
    fn when_judged_holding_a_here_string_then_it_is_allowed() {
        assert_eq!(verdict_of("jq -r .a <<< '{\"a\": 1}'"), Verdict::Allow);
    }
}
