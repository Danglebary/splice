//! The files beside the code that state facts about it: the plugin manifest's version and
//! the skill's script examples. Each case reads the file from the tree and holds it to the
//! crate, so a release bump or a grammar change that leaves one behind fails here.

#[cfg(test)]
mod support {
    use std::path::Path;

    pub fn read(relative: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        std::fs::read_to_string(path).unwrap()
    }

    /// The scripts in a Markdown file's fenced blocks: the heredoc body of a block that
    /// runs splice, or a whole block that opens with `===` or `@@`, the latter given a
    /// file header so it parses alone.
    pub fn scripts(markdown: &str) -> Vec<String> {
        let mut scripts = Vec::new();
        for block in markdown.split("```").skip(1).step_by(2) {
            let body = block.split_once('\n').map_or("", |(_, rest)| rest);
            if let Some((_, after_opener)) = body.split_once("<<'EOF'\n") {
                let script = after_opener.split("\nEOF").next().unwrap();
                scripts.push(format!("{script}\n"));
            } else if body.starts_with("===") {
                scripts.push(body.to_owned());
            } else if body.starts_with("@@") {
                scripts.push(format!("=== example\n{body}"));
            }
        }
        scripts
    }
}

mod given_the_plugin_manifest {
    use super::support::read;

    #[test]
    fn when_read_then_its_version_is_the_crate_version() {
        let manifest: serde_json::Value =
            serde_json::from_str(&read("claude-code/splice/.claude-plugin/plugin.json")).unwrap();

        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
    }
}

mod given_the_skill {
    use super::support::{read, scripts};

    #[test]
    fn when_its_examples_are_parsed_then_every_one_is_a_valid_script() {
        let examples = scripts(&read("claude-code/splice/skills/splice/SKILL.md"));

        assert!(
            examples.len() >= 6,
            "the skill shows its patterns as scripts"
        );
        for example in &examples {
            assert!(splice::script::parse(example).is_ok(), "{example}");
        }
    }
}
