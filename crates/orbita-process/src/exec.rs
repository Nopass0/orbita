//! Process instances, standard file descriptors and the process engine.
//!
//! The engine owns processes, assigns [`Pid`]s, spreads processes over
//! the available CPUs ([`ProcessEngine::set_logical_cpus`]) and pumps
//! stdin lines through the host ([`ProcessHost`]) — the OS API surface.

use crate::format::OrbExec;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// Process identifier. Pids start at 1; 0 is reserved for the kernel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pid(pub u64);

impl Pid {
    /// The kernel pseudo-pid.
    pub const KERNEL: Pid = Pid(0);
}

/// Privilege level of a process.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Privileges {
    /// Full system access (the seeded system binaries).
    Root,
    /// Restricted access for user processes.
    User,
}

impl Privileges {
    pub fn is_root(&self) -> bool {
        matches!(self, Privileges::Root)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Privileges::Root => "root",
            Privileges::User => "user",
        }
    }
}

/// How a process terminated.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    Failure(u64),
    Killed,
}

/// Lifecycle state of a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    /// Spawned, waiting for stdin / host service.
    Running,
    /// The host asked it to stop; drains on the next pump.
    Exiting(ExitStatus),
    /// Finished; stdout/stderr may still hold undrained lines.
    Exited(ExitStatus),
}

/// One line-based standard descriptor (stdin, stdout or stderr).
///
/// Channels are byte pipes with line framing — the same contract a POSIX
/// process sees on fds 0/1/2, implemented over ring buffers.
#[derive(Debug, Default)]
pub struct FdChannel {
    lines: VecDeque<String>,
    closed: bool,
}

impl FdChannel {
    /// Queues one complete line.
    pub fn push_line(&mut self, line: &str) {
        if !self.closed {
            self.lines.push_back(String::from(line));
        }
    }

    /// Takes everything written so far.
    pub fn drain_lines(&mut self) -> Vec<String> {
        self.lines.drain(..).collect()
    }

    /// True when nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Closes the channel for further writes.
    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// The three standard descriptors, POSIX-style.
#[derive(Debug, Default)]
pub struct FdTable {
    /// fd 0.
    pub stdin: FdChannel,
    /// fd 1.
    pub stdout: FdChannel,
    /// fd 2.
    pub stderr: FdChannel,
}

impl FdTable {
    pub fn has_std(&self) -> bool {
        true
    }
}

/// A running OS process: identity, privileges, fds and CPU placement.
#[derive(Debug)]
pub struct Process {
    pid: Pid,
    name: String,
    privileges: Privileges,
    state: ProcessState,
    /// Standard descriptors (fd 0/1/2).
    pub fd_table: FdTable,
    /// CPU the scheduler placed this process on.
    pub cpu_affinity: u32,
    /// Remaining exit code path bookkeeping.
    exit_requested: Option<ExitStatus>,
}

impl Process {
    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn privileges(&self) -> &Privileges {
        &self.privileges
    }

    pub fn state(&self) -> &ProcessState {
        &self.state
    }

    pub fn fd_table(&self) -> &FdTable {
        &self.fd_table
    }

    /// Mutable stdin access for the console bridge.
    pub fn stdin_mut(&mut self) -> &mut FdChannel {
        &mut self.fd_table.stdin
    }

    /// Asks the engine to stop this process with `status` on the next
    /// pump — the in-process equivalent of `exit()`.
    pub fn request_exit(&mut self, status: ExitStatus) {
        if self.exit_requested.is_none() {
            self.exit_requested = Some(status);
        }
    }
}

/// Why a spawn was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    /// The engine is out of pid space (practically unreachable).
    OutOfPids,
}

/// The OS API surface a process is allowed to call, implemented by the
/// kernel. Every stdin line a process consumes is dispatched here.
pub trait ProcessHost {
    /// Handles one stdin line on behalf of `process`; the host writes
    /// replies into `process.fd_table.stdout` / `stderr`.
    fn on_stdin_line(&mut self, process: &mut Process, line: &str);
}

/// Owns all processes and provides spawn/pump scheduling.
///
/// Placement policy: processes are spread round-robin over
/// `logical_cpus` so load is distributed from the first spawn — the
/// foundation the preemptive SMP scheduler builds on.
#[derive(Debug)]
pub struct ProcessEngine {
    next_pid: u64,
    logical_cpus: u32,
    placement_cursor: u32,
    processes: Vec<Process>,
}

impl ProcessEngine {
    pub fn new() -> Self {
        Self {
            next_pid: 1,
            logical_cpus: 1,
            placement_cursor: 0,
            processes: Vec::new(),
        }
    }

    /// Tells the scheduler how many logical CPUs exist (CPUID-derived).
    pub fn set_logical_cpus(&mut self, cpus: u32) {
        self.logical_cpus = cpus.max(1);
    }

    pub fn logical_cpus(&self) -> u32 {
        self.logical_cpus
    }

    /// Spawns a process from a validated ORBEXEC binary. Root-flagged
    /// binaries run with [`Privileges::Root`].
    pub fn spawn(&mut self, binary: OrbExec) -> Result<Pid, SpawnError> {
        let pid = Pid(self.next_pid);
        self.next_pid = self.next_pid.checked_add(1).ok_or(SpawnError::OutOfPids)?;
        let cpu_affinity = self.placement_cursor % self.logical_cpus;
        self.placement_cursor = self.placement_cursor.wrapping_add(1);
        self.processes.push(Process {
            pid,
            name: String::from(binary.name()),
            privileges: if binary.requires_root() {
                Privileges::Root
            } else {
                Privileges::User
            },
            state: ProcessState::Running,
            fd_table: FdTable::default(),
            cpu_affinity,
            exit_requested: None,
        });
        Ok(pid)
    }

    pub fn process(&self, pid: Pid) -> Option<&Process> {
        self.processes.iter().find(|p| p.pid == pid)
    }

    /// Snapshot of the process table as `(pid, name, state)` rows for `ps`.
    pub fn snapshot(&self) -> Vec<(u32, String, String)> {
        self.processes
            .iter()
            .map(|p| {
                let state = match p.state {
                    ProcessState::Running => "running",
                    ProcessState::Exiting(_) => "exiting",
                    ProcessState::Exited(_) => "exited",
                };
                (p.pid.0 as u32, p.name.clone(), String::from(state))
            })
            .collect()
    }

    pub fn process_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        self.processes.iter_mut().find(|p| p.pid == pid)
    }

    pub fn live_count(&self) -> usize {
        self.processes
            .iter()
            .filter(|p| p.state == ProcessState::Running)
            .count()
    }

    /// Runs one scheduling round: every live process consumes its pending
    /// stdin lines through the host; exit requests are honoured.
    pub fn pump(&mut self, host: &mut dyn ProcessHost) {
        let mut finished: Vec<(usize, ExitStatus)> = Vec::new();
        for index in 0..self.processes.len() {
            let (done, status) = {
                let process = &mut self.processes[index];
                let lines = process.fd_table.stdin.drain_lines();
                for line in lines {
                    if process.state == ProcessState::Running {
                        host.on_stdin_line(process, &line);
                    }
                }
                match process.exit_requested {
                    Some(status) => (true, status),
                    None => (false, ExitStatus::Success),
                }
            };
            if done {
                let process = &mut self.processes[index];
                process.state = ProcessState::Exited(status);
                process.fd_table.stdin.close();
                finished.push((index, status));
            }
        }
        let _ = finished;
    }
}
