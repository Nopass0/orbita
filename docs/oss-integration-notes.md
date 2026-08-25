# OSS Integration Notes

This repository now has a UI-independent shell runtime in `crates/orbita-shell`.
The following upstream projects are the most relevant candidates for deeper parser
and driver work.

## Shell Parser / Interpreter

- Flash Shell (Rust, POSIX-style parser/interpreter): https://github.com/raphamorim/flash
- minishell (C, bash-like grammar subset with pipes/redirections): https://github.com/twagger/minishell

Why these matter:

- `flash` is the strongest Rust-side candidate for replacing the current hand-written
  shell subset with a larger POSIX shell grammar.
- `minishell` is useful as a grammar and behavior reference when validating parser
  and redirection semantics.

## Driver / Device Candidates

- VirtIO guest drivers in Rust: https://github.com/rcore-os/virtio-drivers
- xHCI in Rust: https://github.com/rust-osdev/xhci
- AHCI in Rust: https://github.com/Starry-OS/simple-ahci
- NVMe in Rust: https://github.com/H4n-uL/nvme-oxide
- IXGBE in Rust: https://github.com/drivercraft/ixgbe-driver/
- PS/2 in Rust: https://github.com/lucis-fluxum/ps2-rs
- PC keyboard decoding in Rust: https://github.com/rust-embedded-community/pc-keyboard.git
- PCI typed model in Rust: https://github.com/rust-osdev/pci_types
- Rust OSDev organization: https://github.com/rust-osdev
- Example Rust OS with PCI / VirtIO / filesystems: https://github.com/jdreaver/rust-os
- Windows Rust driver samples: https://github.com/microsoft/Windows-rust-driver-samples
- Windows Rust driver platform: https://github.com/microsoft/windows-drivers-rs

Why these matter:

- `virtio-drivers` is the best immediate candidate for standalone Rust driver reuse
  in QEMU and other virtualized environments.
- `xhci`, `simple-ahci`, `nvme-oxide`, `ixgbe-driver`, `ps2-rs`, `pc-keyboard`,
  `pci_types`, and `ids_rs` are all suitable for isolation behind Cargo features
  in a separate driver integration crate.
- `rust-osdev` contains reusable ecosystem crates around PCI, PS/2, USB, and boot/runtime.
- `jdreaver/rust-os` is a practical architecture reference for integrating PCI,
  VirtIO, userspace, and VFS without tying execution to a GUI terminal.
- Microsoft Rust driver repositories are not directly reusable in this kernel, but are
  relevant for separate host-side driver/tooling work.
- `rtl8139-rs` is a useful candidate, but its current dependency chain is not clean
  on this toolchain, so it should stay out of the default compile path for now.

## Integration Rule

Do not import upstream code directly into the kernel path without first isolating:

1. License compatibility.
2. `no_std` viability.
3. Interrupt / DMA / memory ownership assumptions.
4. Bus and device ABI boundaries.
5. Independent buildability in a separate crate or workspace.
