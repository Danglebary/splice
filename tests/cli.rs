//! The binary end to end, against real files in a scratch directory per case: what it
//! writes, what it leaves alone, what it prints, and the exit code an agent's `&&` chain
//! turns on. The pure rules are tested in the library; these cases cover the wiring.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use support::{Scratch, stderr, stdout};

#[cfg(test)]
mod support {
    use std::fs;
    use std::fs::Permissions;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    /// A directory of its own for one case, removed when the case ends.
    pub struct Scratch {
        pub root: PathBuf,
    }

    impl Scratch {
        pub fn new() -> Self {
            let index = NEXT_DIRECTORY.fetch_add(1, Ordering::SeqCst);
            let name = format!("splice-cli-{}-{index}", std::process::id());
            let root = std::env::temp_dir().join(name);
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        pub fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.root.join(name);
            fs::write(&path, text).unwrap();
            path
        }

        pub fn read(&self, name: &str) -> String {
            fs::read_to_string(self.root.join(name)).unwrap()
        }

        pub fn exists(&self, name: &str) -> bool {
            self.root.join(name).exists()
        }

        pub fn directory(&self, name: &str) {
            fs::create_dir(self.root.join(name)).unwrap();
        }

        /// Makes the directory `name` refuse new files, so a write beside a file in it fails.
        pub fn lock(&self, name: &str) {
            let directory = self.root.join(name);
            fs::set_permissions(&directory, Permissions::from_mode(0o555)).unwrap();
            assert!(
                fs::write(directory.join("probe"), "").is_err(),
                "a locked directory refuses a new file, which a root user is never refused"
            );
        }

        /// Lets the directory `name` take new files again, so the scratch can be removed.
        pub fn unlock(&self, name: &str) {
            let directory = self.root.join(name);
            fs::set_permissions(&directory, Permissions::from_mode(0o755)).unwrap();
        }

        pub fn run(&self, arguments: &[&str], stdin: &str) -> Output {
            let mut child = Command::new(env!("CARGO_BIN_EXE_splice"))
                .args(arguments)
                .current_dir(&self.root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut input = child.stdin.take().unwrap();
            input.write_all(stdin.as_bytes()).unwrap();
            drop(input);
            child.wait_with_output().unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.root) {
                eprintln!("could not remove {}: {error}", self.root.display());
            }
        }
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8(output.stderr.clone()).unwrap()
    }
}

mod given_a_script_whose_hunks_all_match {
    use super::*;

    #[test]
    fn when_applied_then_every_file_is_written_and_nothing_is_printed() {
        let scratch = Scratch::new();
        scratch.write("a.rs", "fn a() {\n    old();\n}\n");
        scratch.write("b.md", "# Title\n");

        let output = scratch.run(
            &[],
            "=== a.rs\n@@\n-    old();\n+    new();\n=== b.md\n@@ append\n+Body.\n",
        );

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(stdout(&output), "");
        assert_eq!(stderr(&output), "");
        assert_eq!(scratch.read("a.rs"), "fn a() {\n    new();\n}\n");
        assert_eq!(scratch.read("b.md"), "# Title\nBody.\n");
    }

    #[test]
    fn when_applied_with_diff_then_the_diff_is_printed_and_the_file_written() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(&["--diff"], "=== a.txt\n@@\n-x\n+y\n");

        assert_eq!(output.status.code(), Some(0));
        assert!(stdout(&output).contains("-x\n+y\n"));
        assert_eq!(scratch.read("a.txt"), "y\n");
    }

    #[test]
    fn when_applied_with_dry_run_then_the_diff_is_printed_and_nothing_written() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(&["--dry-run"], "=== a.txt\n@@\n-x\n+y\n");

        assert_eq!(output.status.code(), Some(0));
        assert!(stdout(&output).contains("+y\n"));
        assert_eq!(scratch.read("a.txt"), "x\n");
    }

    #[test]
    fn when_applied_to_an_executable_then_its_mode_is_kept() {
        let scratch = Scratch::new();
        let path = scratch.write("run.sh", "echo old\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        scratch.run(&[], "=== run.sh\n@@\n-echo old\n+echo new\n");

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn when_applied_through_a_symbolic_link_then_the_target_changes_and_the_link_stays() {
        let scratch = Scratch::new();
        scratch.write("target.txt", "x\n");
        std::os::unix::fs::symlink("target.txt", scratch.root.join("link.txt")).unwrap();

        scratch.run(&[], "=== link.txt\n@@\n-x\n+y\n");

        assert_eq!(scratch.read("target.txt"), "y\n");
        assert!(
            fs::symlink_metadata(scratch.root.join("link.txt"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn when_applied_with_create_then_the_file_takes_the_mode_a_new_file_gets() {
        let scratch = Scratch::new();
        let reference = scratch.write("reference.txt", "");

        scratch.run(&[], "=== new.txt\n@@ create\n+a\n");

        let mode_of = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode_of(&scratch.root.join("new.txt")), mode_of(&reference));
    }

    #[test]
    fn when_applied_with_create_under_a_new_directory_then_the_directory_and_file_are_made() {
        let scratch = Scratch::new();

        let output = scratch.run(&[], "=== deep/new.txt\n@@ create\n+hello\n");

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(scratch.read("deep/new.txt"), "hello\n");
    }

    #[test]
    fn when_applied_from_a_script_file_then_stdin_is_not_read() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");
        scratch.write("edit.splice", "=== a.txt\n@@\n-x\n+y\n");

        let output = scratch.run(&["--script", "edit.splice"], "");

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(scratch.read("a.txt"), "y\n");
    }
}

mod given_a_script_with_one_hunk_that_does_not_match {
    use super::*;

    #[test]
    fn when_applied_then_no_file_is_written_and_it_exits_refused() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");
        scratch.write("b.txt", "y\n");

        let output = scratch.run(
            &[],
            "=== a.txt\n@@\n-x\n+changed\n=== b.txt\n@@\n-missing\n+z\n",
        );

        assert_eq!(output.status.code(), Some(1));
        assert_eq!(scratch.read("a.txt"), "x\n");
        assert_eq!(scratch.read("b.txt"), "y\n");
        assert!(stderr(&output).contains("b.txt"));
    }

    #[test]
    fn when_applied_naming_one_file_by_two_spellings_then_it_is_refused() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\ny\n");

        let output = scratch.run(&[], "=== a.txt\n@@\n-x\n+1\n=== ./a.txt\n@@\n-y\n+2\n");

        assert_eq!(output.status.code(), Some(1));
        assert_eq!(scratch.read("a.txt"), "x\ny\n");
    }
}

mod given_a_malformed_script {
    use super::*;

    #[test]
    fn when_applied_then_it_exits_with_the_usage_code_naming_the_line() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(&[], "=== a.txt\n@@ -1,2 +1,2 @@\n-x\n+y\n");

        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("script line 2"));
        assert_eq!(scratch.read("a.txt"), "x\n");
    }
}

mod given_a_script_naming_a_file_that_cannot_be_written {
    use super::*;

    const SCRIPT: &str = "=== first.txt\n@@\n-x\n+X\n=== locked/second.txt\n@@\n-y\n+Y\n";

    fn scratch_with_a_locked_second_file() -> Scratch {
        let scratch = Scratch::new();
        scratch.write("first.txt", "x\n");
        scratch.directory("locked");
        scratch.write("locked/second.txt", "y\n");
        scratch.lock("locked");
        scratch
    }

    #[test]
    fn when_applied_then_no_file_is_written_and_it_exits_with_the_io_code() {
        let scratch = scratch_with_a_locked_second_file();

        let output = scratch.run(&[], SCRIPT);
        scratch.unlock("locked");

        assert_eq!(output.status.code(), Some(3));
        assert_eq!(scratch.read("first.txt"), "x\n");
        assert_eq!(scratch.read("locked/second.txt"), "y\n");
    }

    #[test]
    fn when_tried_then_the_command_never_runs_and_no_file_is_written() {
        let scratch = scratch_with_a_locked_second_file();

        let output = scratch.run(&["try", "--", "touch", "ran"], SCRIPT);
        scratch.unlock("locked");

        assert_eq!(output.status.code(), Some(3));
        assert!(!scratch.exists("ran"));
        assert_eq!(scratch.read("first.txt"), "x\n");
    }
}

mod given_a_temporary_name_already_taken {
    use super::*;

    /// As many names beside a file as splice tries for a temporary.
    const TEMPORARY_NAMES_MAX: usize = 16;

    #[test]
    fn when_applied_with_a_link_at_the_temporary_name_then_the_link_target_is_left_alone() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");
        scratch.write("victim.txt", "v\n");
        std::os::unix::fs::symlink("victim.txt", scratch.root.join(".a.txt.splice")).unwrap();

        let output = scratch.run(&[], "=== a.txt\n@@\n-x\n+y\n");

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(scratch.read("victim.txt"), "v\n");
        assert_eq!(scratch.read("a.txt"), "y\n");
    }

    #[test]
    fn when_applied_with_every_temporary_name_taken_then_nothing_is_written() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");
        scratch.write(".a.txt.splice", "");
        for attempt in 1..TEMPORARY_NAMES_MAX {
            scratch.write(&format!(".a.txt.splice-{attempt}"), "");
        }

        let output = scratch.run(&[], "=== a.txt\n@@\n-x\n+y\n");

        assert_eq!(output.status.code(), Some(3));
        assert_eq!(scratch.read("a.txt"), "x\n");
    }
}

mod given_the_try_subcommand {
    use super::*;

    #[test]
    fn when_run_then_the_command_sees_the_edit_and_the_file_is_restored_after() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(
            &["try", "--", "grep", "-q", "mutated", "a.txt"],
            "=== a.txt\n@@\n-x\n+mutated\n",
        );

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(scratch.read("a.txt"), "x\n");
    }

    #[test]
    fn when_run_then_a_created_file_is_removed_after() {
        let scratch = Scratch::new();

        let output = scratch.run(
            &["try", "--", "test", "-f", "new.txt"],
            "=== new.txt\n@@ create\n+a\n",
        );

        assert_eq!(output.status.code(), Some(0));
        assert!(!scratch.exists("new.txt"));
    }

    #[test]
    fn when_run_with_a_failing_command_then_its_exit_code_passes_through() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(
            &["try", "--", "sh", "-c", "exit 7"],
            "=== a.txt\n@@\n-x\n+y\n",
        );

        assert_eq!(output.status.code(), Some(7));
        assert_eq!(scratch.read("a.txt"), "x\n");
    }

    #[test]
    fn when_run_expecting_failure_over_a_failing_command_then_it_succeeds() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(
            &["try", "--expect-fail", "--", "false"],
            "=== a.txt\n@@\n-x\n+y\n",
        );

        assert_eq!(output.status.code(), Some(0));
    }

    #[test]
    fn when_run_expecting_failure_over_a_passing_command_then_it_exits_survived() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(
            &["try", "--expect-fail", "--", "true"],
            "=== a.txt\n@@\n-x\n+y\n",
        );

        assert_eq!(output.status.code(), Some(4));
    }

    #[test]
    fn when_run_with_a_hunk_that_does_not_match_then_the_command_never_runs() {
        let scratch = Scratch::new();
        scratch.write("a.txt", "x\n");

        let output = scratch.run(
            &["try", "--", "touch", "ran"],
            "=== a.txt\n@@\n-missing\n+y\n",
        );

        assert_eq!(output.status.code(), Some(1));
        assert!(!scratch.exists("ran"));
    }
}

mod given_the_guard_subcommand {
    use super::*;

    fn hook_input(command: &str) -> String {
        let input = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": command, "description": "x" },
        });
        input.to_string()
    }

    #[test]
    fn when_run_over_sed_dash_i_then_it_blocks_with_the_splice_example_on_stderr() {
        let scratch = Scratch::new();

        let output = scratch.run(&["guard"], &hook_input("sed -i 's/a/b/' f"));

        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("splice <<'EOF'"));
    }

    #[test]
    fn when_run_over_a_read_then_it_allows_silently() {
        let scratch = Scratch::new();

        let output = scratch.run(&["guard"], &hook_input("sed -n '1,5p' f"));

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(stderr(&output), "");
    }

    #[test]
    fn when_run_over_input_that_is_not_json_then_it_reports_a_non_blocking_error() {
        let scratch = Scratch::new();

        let output = scratch.run(&["guard"], "not json");

        assert_eq!(output.status.code(), Some(1));
    }
}

mod given_the_help_flag {
    use super::*;

    #[test]
    fn when_run_then_the_grammar_is_printed() {
        let scratch = Scratch::new();

        let output = scratch.run(&["--help"], "");

        assert_eq!(output.status.code(), Some(0));
        assert!(stdout(&output).contains("@@ regex"));
    }
}

#[test]
fn scratch_directories_are_distinct() {
    let first = Scratch::new();
    let second = Scratch::new();

    assert_ne!(first.root, second.root);
    assert!(Path::new(&first.root).is_dir());
}
