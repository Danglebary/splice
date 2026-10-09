//! Every exit code splice ends with, each with one owner.

pub const SUCCESS: u8 = 0;
/// A hunk did not match as declared; nothing was written.
pub const REFUSED: u8 = 1;
/// The script or the arguments break the grammar; nothing was written.
pub const USAGE: u8 = 2;
/// Reading or writing a file failed.
pub const IO: u8 = 3;
/// Under `try --expect-fail`, the command passed where it was expected to fail.
pub const SURVIVED: u8 = 4;
/// Under `try`, the program exists and could not be started; the shells' convention.
pub const PROGRAM_NOT_EXECUTABLE: u8 = 126;
/// Under `try`, no program by that name was found; the shells' convention.
pub const PROGRAM_NOT_FOUND: u8 = 127;
/// Under `try`, a command ended by a signal exits with this plus the signal's number;
/// the shells' convention.
pub const SIGNAL_BASE: u8 = 128;

// The two codes a Claude Code command hook speaks: 2 blocks the tool call and hands
// stderr to the model, and any other non-zero code is a non-blocking error the user sees.
// https://code.claude.com/docs/en/hooks — checked 2026-10-08
pub const HOOK_BLOCK: u8 = 2;
pub const HOOK_ERROR: u8 = 1;
