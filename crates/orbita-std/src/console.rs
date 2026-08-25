//! Console output: the kernel's `println!` / `print!`, routed through the
//! platform console (`orbita-platform`), mirroring the std macros.
//!
//! Before the platform console is initialized (`platform::init_early_console`
//! in the kernel entry), output is dropped silently — the platform layer
//! handles that itself.

/// Backend used by the `print!` family of macros. Line-oriented: the
/// platform console emits one serial line per call.
///
/// In host-side unit tests there is no kernel console and the platform
/// serial backend would execute privileged port I/O, so output is dropped.
#[cfg(not(test))]
pub fn print_fmt(args: core::fmt::Arguments<'_>) {
    orbita_platform::log_line_fmt(args);
}

#[cfg(test)]
pub fn print_fmt(_args: core::fmt::Arguments<'_>) {}
