//! The Orbita system surface: one concern per module.
//!
//! - [`process`] — stdout, exit codes
//! - [`fs`] — files and directories on the live OrbitaFS volume
//! - [`net`] — network inventory (sockets arrive with a later ABI)
//! - [`time`] — monotonic time
//! - [`os`] — kernel/OS information

pub mod fs;
pub mod net;
pub mod os;
pub mod process;
pub mod time;
