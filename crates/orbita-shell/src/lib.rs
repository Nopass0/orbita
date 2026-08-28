#![no_std]

//! Orbita shell: parser and interactive command runtime.
//!
//! This crate provides:
//!
//! - [`ShellParser`] — a real tokenizer/parser for shell scripts
//!   (quotes, escapes, `;` sequences, `|` pipelines, `<`/`>`/`>>`
//!   redirections, `VAR=value` assignments, `$VAR` expansion).
//! - [`ShellRuntime`] — execution of parsed scripts against an
//!   in-memory volume ([`MemoryVolume`](orbita_fs::MemoryVolume)) with a growing
//!   builtin command set, plus the real package manager (`pkg`).
//!
//! The unused `builtin`/`session`/`tty` contract sketches were removed;
//! dispatch lives in the `runtime` module and the kernel wires the terminal directly.

extern crate alloc;

mod command;
mod parser;
mod runtime;

pub use command::{
    CommandArg, CommandLine, CommandName, CommandPipeline, CommandWord, ParsedCommand,
    RedirectKind, RedirectSpec, ShellAssignment, ShellCommandError, ShellScript, SimpleCommand,
};
pub mod interp;
pub use parser::{ParseError, ShellParser};
pub use runtime::{NoopShellHost, ShellEnvironment, ShellHost, ShellOutput, ShellRuntime, ShellSystemInfo};
