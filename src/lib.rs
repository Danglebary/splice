//! The functional core of splice: the script grammar, matching and splicing one file's
//! text, the diff, the hook's verdict, and every message. Nothing here reads or writes a
//! file, reads the environment, or starts a process, so each rule is tested against
//! strings; `main.rs` is the shell that does all of that.

pub mod cli;
pub mod diff;
pub mod exit_code;
pub mod guard;
pub mod plan;
pub mod report;
pub mod script;
