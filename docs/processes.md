# Processes and the ORBEXEC format

Orbita runs its important components as **internal processes** — including
the shell. This document describes the process model implemented in
`crates/orbita-process`.

## The ORBEXEC binary format

Components are compiled *by the OS rules* into `ORBEXEC` binaries stored
in the filesystem (`/bin/*.orbexec`):

```text
offset  size  field
0       8     magic "ORBEXEC\0"
8       2     format version (LE u16, = 1)
10      2     flags (LE u16): bit0 = requires root
12      4     OS API version (LE u32) — loader rejects mismatches
16      4     manifest length (LE u32)
20      4     payload length  (LE u32)
24      …     manifest: "key=value\n" lines (name=, entry=, …)
…       …     payload: component-defined bytes
```

`OrbExecBuilder` produces binaries ("the compile step"),
`OrbExec::parse` validates them (magic, version, API, bounds).

## Process identity and privileges

- `Pid` — starts at 1, 0 reserved for the kernel.
- `Privileges::Root` / `Privileges::User` — set from the binary's root
  flag. The seeded system binaries (`/bin/orbita-shell`, …) are root.

## Standard file descriptors

`FdTable` gives every process POSIX-style fds:

- fd 0 `stdin` — line-framed input (the console pushes keystrokes here)
- fd 1 `stdout` — line-framed output (rendered by the desktop console)
- fd 2 `stderr` — diagnostics stream

Channels are ring buffers with `push_line` / `drain_lines` / `close`.

## The OS API for processes

A process never touches kernel structures. It consumes stdin lines and
produces stdout/stderr lines; the kernel side of the contract is the
`ProcessHost` trait:

```rust
trait ProcessHost {
    fn on_stdin_line(&mut self, process: &mut Process, line: &str);
}
```

`ProcessEngine::pump(host)` is the scheduling round: every live process
drains its stdin through the host; exit requests are honoured. Placement
is round-robin over `set_logical_cpus(n)` — the load-spreading
foundation for the preemptive SMP scheduler (roadmap: per-CPU runqueues
driven by the APIC timer, AP bring-up via INIT-SIPI-SIPI).

## Lifecycle

```
spawn(OrbExec) -> Pid            Running
stdin line → ProcessHost         Running
request_exit(status)             Exiting → Exited (on next pump)
```

Host-side unit tests cover: binary roundtrip + corruption rejection, API
version rejection, full lifecycle with fds, and CPU spreading.

## SMP bring-up status (experimental)

The `smp_ap` module in `orbita-arch-x86_64` implements the full
INIT-SIPI-SIPI path: a hand-assembled real-mode trampoline (installed
at physical 0x8000, all addresses patched at copy time) that walks the
AP through 16-bit -> 32-bit -> long mode reusing the BSP page tables,
and parks it in `ap_entry` with an online counter. The sequence is
verified against QEMU ICR semantics (ESR stays clean, delivery status
polled) and is config-gated: set `smp=on` in `/etc/orbita.conf` to
attempt AP wake-up. On OVMF/QEMU the APs do not currently leave the
firmware park (clean ESR but no trampoline execution) - investigation
continues; the failure mode is safe (the BSP continues, the log reports
the partial result).

## SMP bring-up status (experimental)

 in  implements the full INIT-SIPI-SIPI
path: a hand-assembled real-mode trampoline (installed at physical
0x8000, all addresses patched at copy time) that walks the AP through
16-bit → 32-bit → long mode reusing the BSP page tables, and parks it
in  with an online counter. The sequence is verified against
QEMU ICR semantics (ESR stays clean, delivery status polled) and is
**config-gated**: set  in  to attempt AP
wake-up. On OVMF/QEMU the APs do not currently leave the firmware park
(clean ESR but no trampoline execution) — investigation continues; the
failure mode is safe (BSP continues, log reports the partial result).
