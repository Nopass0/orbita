//! # orbita-process — Orbita process model
//!
//! The kernel-side contract for "things that run as processes inside the
//! OS": the [`OrbExec`] binary format, process identity ([`Pid`],
//! [`Privileges`]), standard file descriptors ([`FdTable`] with
//! stdin/stdout/stderr), and the cooperative [`ProcessEngine`].
//!
//! ## The ORBEXEC format
//!
//! Components compiled *by the OS rules* into its own binaries use the
//! `ORBEXEC` container (already seeded on disk as `/bin/*.orbexec`):
//!
//! ```text
//! [0..8)    magic "ORBEXEC"
//! [8..10)   format version (u16 LE, currently 1)
//! [10..12)  flags (u16 LE): bit0 = root privileges required)
//! [12..16)  api_version (u32 LE) — OS API the binary is built against
//! [16..20)  manifest_len (u32 LE)
//! [20..24)  payload_len  (u32 LE)
//! [24..24+manifest_len)  manifest: "key=value\n" lines (name=, entry=, …)
//! [..+payload_len)       payload: component-specific data
//! ```
//!
//! Loading validates the magic, version, flags and API version before a
//! [`Process`] is spawned — a binary built for a different OS API is
//! rejected, not mis-executed.
//!
//! ## Processes and the OS API
//!
//! A process never touches kernel structures directly: it receives lines
//! on **stdin**, produces lines on **stdout**/**stderr**, and calls the
//! host through the [`ProcessHost`] trait — that trait *is* the OS API
//! surface for processes. The console bridges keystrokes into the shell
//! process's stdin and renders its stdout.

#![no_std]

extern crate alloc;

pub mod exec;
pub mod format;

pub use exec::{ExitStatus, FdChannel, FdTable, Pid, Privileges, Process, ProcessEngine, ProcessHost, ProcessState, SpawnError};
pub use format::{OrbExec, OrbExecBuilder, ORBEXEC_API_VERSION};

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::{String, ToString};
    use alloc::vec;

    #[test]
    fn orbexec_roundtrip_and_validation() {
        let binary = OrbExecBuilder::new("orbita-shell", "shell_main")
            .with_root()
            .manifest_line("commands=ls,cat,write")
            .build();
        let exec = OrbExec::parse(&binary).expect("parses");
        assert_eq!(exec.manifest().get("name").map(String::as_str), Some("orbita-shell"));
        assert_eq!(exec.manifest().get("entry").map(String::as_str), Some("shell_main"));
        assert!(exec.requires_root());

        // Corruption is rejected.
        let mut broken = binary.clone();
        broken[0] = b'X';
        assert!(OrbExec::parse(&broken).is_err());
    }

    #[test]
    fn api_version_mismatch_rejected() {
        let binary = OrbExecBuilder::new("x", "e").build();
        let mut stale = binary.clone();
        stale[12] = 0xEE; // wrong api_version byte
        stale[13] = 0xEE;
        assert!(OrbExec::parse(&stale).is_err());
    }

    /// Host used by tests: echoes every stdin line to stdout in upper case.
    struct UpperHost;

    impl ProcessHost for UpperHost {
        fn on_stdin_line(&mut self, process: &mut Process, line: &str) {
            process.fd_table.stdout.push_line(&line.to_uppercase());
        }
    }

    #[test]
    fn process_lifecycle_with_fds() {
        let binary = OrbExecBuilder::new("orbita-shell", "shell_main").with_root().build();
        let exec = OrbExec::parse(&binary).unwrap();
        let mut engine = ProcessEngine::new();
        let pid = engine.spawn(exec).expect("spawn");
        assert_eq!(pid, Pid(1));

        // The process has the standard descriptors open.
        {
            let p = engine.process(pid).unwrap();
            assert!(p.fd_table().has_std());
            assert!(p.privileges().is_root());
        }

        // Root-only spawn policy: a non-root binary still runs, but is
        // marked unprivileged.
        let user_bin = OrbExecBuilder::new("guest", "main").build();
        let pid2 = engine.spawn(OrbExec::parse(&user_bin).unwrap()).unwrap();
        assert!(!engine.process(pid2).unwrap().privileges().is_root());

        // Feed stdin, pump, drain stdout.
        engine.process_mut(pid).unwrap().stdin_mut().push_line("hello");
        engine.pump(&mut UpperHost);
        let lines = engine.process_mut(pid).unwrap().fd_table.stdout.drain_lines();
        assert_eq!(lines, vec!["HELLO".to_string()]);
        assert_eq!(*engine.process(pid).unwrap().state(), ProcessState::Running);

        // Exit path via the host.
        let p = engine.process_mut(pid).unwrap();
        p.request_exit(ExitStatus::Success);
        engine.pump(&mut UpperHost);
        assert_eq!(*engine.process(pid).unwrap().state(), ProcessState::Exited(ExitStatus::Success));
    }

    #[test]
    fn scheduler_distributes_over_cpus() {
        // 4 logical CPUs (as QEMU gives us): spawn 4 processes and check
        // they land on different CPUs via the engine's placement.
        let mut engine = ProcessEngine::new();
        engine.set_logical_cpus(4);
        let mut pids = alloc::vec::Vec::new();
        for i in 0..4 {
            let name = alloc::format!("worker-{i}");
            let bin = OrbExecBuilder::new(&name, "main").build();
            pids.push(engine.spawn(OrbExec::parse(&bin).unwrap()).unwrap());
        }
        let mut seen = alloc::vec::Vec::new();
        for pid in pids {
            seen.push(engine.process(pid).unwrap().cpu_affinity);
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 4, "processes spread across all CPUs");
    }
}
