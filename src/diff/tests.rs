//! The diff's header and change lines, read from the rendered text a reader sees.

use super::*;

mod given_a_changed_line {
    use super::*;

    #[test]
    fn when_rendered_then_the_diff_names_both_sides_and_marks_the_line() {
        let rendered = unified("src/lib.rs", Some("a\nb\n"), "a\nc\n");

        assert_eq!(
            rendered,
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n"
        );
    }
}

mod given_a_created_file {
    use super::*;

    #[test]
    fn when_rendered_then_the_old_side_is_dev_null() {
        let rendered = unified("new.rs", None, "a\n");

        assert_eq!(rendered, "--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+a\n");
    }
}
