//! Application process services: stdout and exit codes.

use crate::abi;

/// Handle to the application's stdout stream.
pub struct Stdout;

impl Stdout {
    /// Emit one line (a trailing newline is added by the host).
    pub fn line(&self, text: &str) {
        abi::stdout_line(text);
    }
}

/// The application's stdout stream.
pub fn stdout() -> Stdout {
    Stdout
}

/// Record the process exit code and finish.
///
/// v1 note: execution still unwinds to the end of `main` normally
/// (there is no preemptive process kill yet); the recorded code
/// overrides `main`'s return value.
pub fn exit(code: i32) {
    abi::set_exit_code(code);
}
