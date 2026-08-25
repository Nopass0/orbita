//! Kernel-side host adapters bridging shell/process contracts to kernel services.

use orbita_net::{Ipv4Address, NetworkStack, StackEvent};
use orbita_process::{ProcessEngine, ProcessHost};
use orbita_shell::ShellRuntime;
use orbita_std::{format, println, String, Vec};

/// The kernel side of the process API: the shell process consumes stdin
/// lines (commands) and answers on stdout. Everything the process can do
/// goes through this trait implementation — it has no other access to
/// kernel structures.
pub(crate) struct ShellProcessHost<'a> {
    #[allow(dead_code)]
    pub(crate) runtime: &'a ShellRuntime,
}

impl ProcessHost for ShellProcessHost<'_> {
    fn on_stdin_line(&mut self, process: &mut orbita_process::Process, line: &str) {
        // Minimal in-process path: echo + command listing come from the
        // ORBEXEC manifest; real execution is bridged by the console loop
        // (see kernel_main) which also owns the MemoryVolume.
        let upper = line.trim();
        if upper == "exit" {
            process.request_exit(orbita_process::ExitStatus::Success);
            return;
        }
        process.fd_table.stdout.push_line(line);
    }
}

/// Adapter exposing kernel services (process execution, process table,
/// networking) to the shell through the [`orbita_shell::ShellHost`] trait.
pub(crate) struct KernelShellHost<'a> {
    pub(crate) process_engine: &'a mut Option<ProcessEngine>,
    pub(crate) net_stack: &'a mut NetworkStack,
    pub(crate) live_nic: &'a mut Option<orbita_hw::E1000>,
}

impl orbita_shell::ShellHost for KernelShellHost<'_> {
    fn exec_app(
        &mut self,
        _env: &mut orbita_shell::ShellEnvironment,
        fs: &mut orbita_fs::MemoryVolume,
        path: &str,
        _args: &[String],
    ) -> Result<(i32, String), String> {
        let bytes = fs
            .read_file_path(path)
            .map_err(|_| format!("run: cannot read {path}"))?;
        let binary = orbita_process::OrbExec::parse(&bytes)
            .map_err(|_| format!("run: {path} is not a valid ORBEXEC binary"))?;
        let net_info = self.net_stack.summary();
        let run = crate::abi::exec_native(fs, net_info, binary.payload())?;
        let output = run.stdout.join("\n");
        println!("Orbita OS: app {} exited with code {}", binary.name(), run.code);
        Ok((run.code, output))
    }

    fn process_rows(&mut self) -> Vec<(u32, String, String)> {
        self.process_engine
            .as_ref()
            .map(|engine| engine.snapshot())
            .unwrap_or_default()
    }

    fn ping(&mut self, target: &str) -> String {
        self.ping_once(target)
    }
}

impl KernelShellHost<'_> {
    /// One bounded ping attempt: ARP-resolve the target if needed, send an
    /// ICMP echo request, and poll the NIC until a reply or timeout.
    fn ping_once(&mut self, target: &str) -> String {
        let Some(nic) = self.live_nic.as_mut() else {
            return String::from("ping: no live network interface (e1000 not bound)");
        };
        let octets: Vec<u8> = target
            .split('.')
            .filter_map(|part| part.parse::<u8>().ok())
            .collect();
        if octets.len() != 4 {
            return format!("ping: invalid address `{target}`");
        }
        let destination = Ipv4Address::new([octets[0], octets[1], octets[2], octets[3]]);

        // Resolve via ARP first (bounded poll loop; QEMU answers fast).
        if self.net_stack.arp.lookup(destination.0).is_none() {
            self.net_stack.send_arp_request(destination);
            flush_tx(self.net_stack, nic);
            for _ in 0..2_000_000 {
                for frame in nic.poll_rx() {
                    let _ = self.net_stack.receive(&frame);
                }
                if self.net_stack.arp.lookup(destination.0).is_some() {
                    break;
                }
                core::hint::spin_loop();
            }
        }

        let sent = self.net_stack.send_icmp_echo_request(destination, 0x0B17, 1);
        if !sent {
            return format!("ping {target}: arp resolution failed");
        }
        flush_tx(self.net_stack, nic);

        for _ in 0..4_000_000 {
            for frame in nic.poll_rx() {
                for event in self.net_stack.receive(&frame) {
                    if let StackEvent::IcmpEchoReply { source, .. } = event {
                        if source == destination {
                            let stats = nic.stats();
                            return format!(
                                "64 bytes from {target}: icmp_seq=1 (rx={} tx={})",
                                stats.rx_frames, stats.tx_frames
                            );
                        }
                    }
                }
            }
            core::hint::spin_loop();
        }
        format!("ping {target}: request timed out")
    }
}

/// Push every queued TX frame through the NIC.
fn flush_tx(net_stack: &mut NetworkStack, nic: &mut orbita_hw::E1000) {
    while let Some(frame) = net_stack.take_tx_frame() {
        nic.send(&frame);
    }
}
