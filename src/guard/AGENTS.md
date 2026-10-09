# The Claude Code hook contract

`guard` integrates with Claude Code's command hooks: `splice guard` is the `PreToolUse` hook the plugin declares in `claude-code/splice/hooks/hooks.json`, matched on the `Bash` tool.

## The API

- Documentation: <https://code.claude.com/docs/en/hooks> (hook input, exit codes, JSON output) and <https://code.claude.com/docs/en/plugins-reference> (where a plugin declares hooks) — checked 2026-10-08.
- Input: one JSON object on stdin. `tool_name` names the tool; for `Bash`, `tool_input.command` holds the command. `hook_input` reads those two fields and nothing else.
- Output: exit 0 lets the call proceed; exit 2 blocks it and hands stderr to the model as the reason; any other non-zero exit is a non-blocking error the user sees and the call proceeds. `exit_code::HOOK_BLOCK` and `exit_code::HOOK_ERROR` hold the two codes splice uses.
- Auth and configuration: none. The hook needs the `splice` binary on `PATH`; without it the shell's exit 127 surfaces as a non-blocking error on every Bash call.

## Using it from the rest of the code

`verdict` is pure and takes the command string; `hook_input` is pure and takes the raw stdin bytes; `main.rs` reads stdin under a size bound, calls both, and prints `report::guard_denial` on a block. A malformed input is a non-blocking error rather than a block, so a change in Claude Code's input shape never stops every Bash call.

The verdict is deliberately narrow: in-place editors, interpreter file writes, a heredoc that is a script carrying either, and a rewrite moved over the original unless `jq` or `yq` made it. Widen it only against real commands, as `AGENTS.md` at the root says.
