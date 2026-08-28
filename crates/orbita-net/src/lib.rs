#![no_std]
//! Orbita network stack.
//!
//! Layer-by-layer packet models with pure parse/build logic:
//!
//! * `ethernet` — MAC addresses, EtherType, frame parse/build
//! * `arp` — IPv4-over-Ethernet ARP
//! * `ipv4` — addresses, header, header checksum
//! * `icmp` — echo request/reply
//! * `udp` / `tcp` — segment parse/build with pseudo-header checksums
//! * `nic` — NIC device model + PCI id matching (e1000, virtio-net, ...)
//! * `wifi` — 802.11 station model (scan records, security, channels)
//! * `bluetooth` — BT device model (addresses, classes, pairing state)
//! * `stack` — interfaces (loopback + NICs), routing, RX dispatch
//!
//! All logic operates on byte slices, no hardware touched — drivers feed
//! frames in and get frames out.

extern crate alloc;

pub mod arp;
pub mod bluetooth;
pub mod ethernet;
pub mod icmp;
pub mod ipv4;
pub mod nic;
pub mod stack;
pub mod tcp;
pub mod tcp_state;
pub mod udp;
pub mod wifi;

pub use ethernet::{EthernetFrame, EtherType, MacAddress};
pub use ipv4::{Ipv4Address, Ipv4Header};
pub use nic::{NicDriverKind, NicInfo, NicStatus};
pub use stack::{InterfaceKind, NetworkInterface, NetworkStack, StackEvent};
