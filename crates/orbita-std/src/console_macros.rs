//! `println!` / `print!` macros writing to the platform console.
//!
//! Declared in a dedicated (private) module so `#[macro_export]` places the
//! macros at the crate root, where the usual `orbita_std::println!` path
//! resolves.

/// Prints to the platform console without a trailing newline.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::console::print_fmt(format_args!($($arg)*))
    };
}

/// Prints to the platform console with a trailing newline (the platform
/// console is line-oriented, so every call emits exactly one line).
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        $crate::console::print_fmt(format_args!($($arg)*))
    };
}

/// Error-variant of `print!` (same stream today).
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {
        $crate::console::print_fmt(format_args!($($arg)*))
    };
}

/// Error-variant of `println!` (same stream today).
#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {
        $crate::console::print_fmt(format_args!($($arg)*))
    };
}
