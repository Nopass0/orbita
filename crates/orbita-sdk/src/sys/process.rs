//! Application process services: stdout and exit codes (ABI v2 syscall
//! transport).

use crate::abi::{self, nr};

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
/// In ring 3 the EXIT syscall terminates the process immediately (the
/// kernel resumes its own context); the ring-0 exec path only records
/// the code and returns, so `main` keeps running to its end.
pub fn exit(code: i32) {
    abi::set_exit_code(code);
    let _ = abi::call(nr::EXIT, code as u32 as u64, 0, 0, 0);
}
