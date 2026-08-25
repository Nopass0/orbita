//! The network stack: interfaces, IPv4 configuration, routing, and RX
//! frame dispatch.
//!
//! The stack is transport-agnostic: a driver hands raw Ethernet frames to
//! [`NetworkStack::receive`] and gets [`StackEvent`]s back; outbound
//! packets returned by build helpers are the driver's to transmit.

use crate::arp::{ArpCache, ArpOperation, ArpPacket};
use crate::ethernet::{EthernetFrame, EtherType, MacAddress};
use crate::icmp::{IcmpKind, IcmpMessage};
use crate::ipv4::{protocol, Ipv4Address, Ipv4Header};
use crate::nic::{NicDriverKind, NicInfo};
use crate::tcp::TcpSegment;
use crate::udp::UdpDatagram;
use orbita_std::{String, Vec, format};

/// Interface medium.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum InterfaceKind {
    Loopback,
    Ethernet,
}

/// One configured network interface.
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    pub name: String,
    pub kind: InterfaceKind,
    pub mac: MacAddress,
    pub address: Ipv4Address,
    pub gateway: Option<Ipv4Address>,
    pub prefix: u8,
    pub up: bool,
}

impl NetworkInterface {
    pub fn loopback() -> Self {
        Self {
            name: String::from("lo"),
            kind: InterfaceKind::Loopback,
            mac: MacAddress::ZERO,
            address: Ipv4Address::LOCALHOST,
            gateway: None,
            prefix: 8,
            up: true,
        }
    }

    /// Builds an Ethernet interface from a NIC inventory record.
    pub fn from_nic(nic: &NicInfo, address: Ipv4Address, gateway: Option<Ipv4Address>, prefix: u8) -> Self {
        let name = match nic.driver {
            NicDriverKind::Loopback => String::from("lo"),
            _ => format!("eth-{}", nic.pci_address),
        };
        Self {
            name,
            kind: InterfaceKind::Ethernet,
            mac: nic.mac,
            address,
            gateway,
            prefix,
            up: nic.status.is_up(),
        }
    }

    /// Configured summary, e.g. `eth0 10.0.2.15/24 gw=10.0.2.2 up`.
    pub fn summary(&self) -> String {
        format!(
            "{} {}/{} {}{}",
            self.name,
            self.address.text(),
            self.prefix,
            match self.gateway {
                Some(gw) => format!("gw={} ", gw.text()),
                None => String::new(),
            },
            if self.up { "up" } else { "down" }
        )
    }
}

/// Events surfaced by the stack to the rest of the kernel.
#[derive(Debug, Clone)]
pub enum StackEvent {
    ArpResolved { ip: Ipv4Address, mac: MacAddress },
    ArpRequestForUs { sender: Ipv4Address, target: Ipv4Address },
    IcmpEchoRequest { source: Ipv4Address, identifier: u16, sequence: u16 },
    IcmpEchoReply { source: Ipv4Address, sequence: u16 },
    UdpReceived { source: Ipv4Address, source_port: u16, destination_port: u16, payload_length: usize },
    TcpSegment { source: Ipv4Address, flags_text: String, destination_port: u16 },
    FrameDropped { reason: &'static str },
}

/// The kernel network stack.
#[derive(Debug)]
pub struct NetworkStack {
    pub interfaces: Vec<NetworkInterface>,
    pub arp: ArpCache,
    /// Echo replies awaiting transmission (destination MAC is resolved via
    /// the ARP cache by the driver).
    pub pending_tx: Vec<Vec<u8>>,
    next_ip_id: u16,
}

impl NetworkStack {
    pub fn new() -> Self {
        let mut stack = Self {
            interfaces: Vec::new(),
            arp: ArpCache::new(),
            pending_tx: Vec::new(),
            next_ip_id: 1,
        };
        stack.interfaces.push(NetworkInterface::loopback());
        stack
    }

    /// Adds an interface.
    pub fn add_interface(&mut self, interface: NetworkInterface) {
        self.interfaces.push(interface);
    }

    /// The interface holding `ip`, if any.
    pub fn interface_for(&self, ip: Ipv4Address) -> Option<&NetworkInterface> {
        self.interfaces.iter().find(|i| i.address.same_network(ip, i.prefix))
    }

    /// Routes a destination to the best interface + next hop.
    pub fn route(&self, destination: Ipv4Address) -> Option<(&NetworkInterface, Ipv4Address)> {
        for interface in &self.interfaces {
            if !interface.up {
                continue;
            }
            if interface.address.same_network(destination, interface.prefix) {
                return Some((interface, destination));
            }
        }
        // otherwise: default gateway of the first up interface
        for interface in &self.interfaces {
            if interface.up {
                if let Some(gw) = interface.gateway {
                    return Some((interface, gw));
                }
            }
        }
        None
    }

    /// Feeds one received Ethernet frame in; returns generated events and
    /// queues any replies in `pending_tx`.
    pub fn receive(&mut self, frame_data: &[u8]) -> Vec<StackEvent> {
        let mut events = Vec::new();
        let frame = match EthernetFrame::parse(frame_data) {
            Some(frame) => frame,
            None => {
                events.push(StackEvent::FrameDropped { reason: "short-frame" });
                return events;
            }
        };

        match frame.ether_type {
            EtherType::Arp => self.receive_arp(frame.payload, &mut events),
            EtherType::Ipv4 => self.receive_ipv4(frame.payload, &mut events),
            EtherType::Ipv6 => events.push(StackEvent::FrameDropped { reason: "ipv6-unhandled" }),
            EtherType::Unknown(_) => events.push(StackEvent::FrameDropped { reason: "ethertype" }),
        }
        events
    }

    fn receive_arp(&mut self, payload: &[u8], events: &mut Vec<StackEvent>) {
        let packet = match ArpPacket::parse(payload) {
            Some(packet) => packet,
            None => {
                events.push(StackEvent::FrameDropped { reason: "arp-malformed" });
                return;
            }
        };
        // Learn whatever the sender claims.
        self.arp.insert(packet.sender_ip, packet.sender_mac);
        events.push(StackEvent::ArpResolved {
            ip: Ipv4Address::new(packet.sender_ip),
            mac: packet.sender_mac,
        });

        let target = Ipv4Address::new(packet.target_ip);
        let for_us = self.interfaces.iter().any(|i| i.address == target);
        if packet.operation == ArpOperation::Request && for_us {
            events.push(StackEvent::ArpRequestForUs {
                sender: Ipv4Address::new(packet.sender_ip),
                target,
            });
            let interface = self
                .interfaces
                .iter()
                .find(|i| i.address == target)
                .expect("checked above");
            let reply = packet.make_reply(interface.mac, packet.target_ip);
            let mut buf = [0u8; crate::arp::PACKET_LEN];
            if let Some(len) = reply.build(&mut buf) {
                let mut frame_buf = [0u8; 14 + crate::arp::PACKET_LEN];
                if let Some(total) = EthernetFrame::build(
                    packet.sender_mac,
                    interface.mac,
                    EtherType::Arp,
                    &buf[..len],
                    &mut frame_buf,
                ) {
                    self.pending_tx.push(frame_buf[..total].to_vec());
                }
            }
        }
    }

    fn receive_ipv4(&mut self, payload: &[u8], events: &mut Vec<StackEvent>) {
        let header = match Ipv4Header::parse(payload) {
            Some(header) => header,
            None => {
                events.push(StackEvent::FrameDropped { reason: "ipv4-checksum" });
                return;
            }
        };
        let is_local = self
            .interfaces
            .iter()
            .any(|i| i.address == header.destination)
            || header.destination.is_broadcast()
            || header.destination.is_multicast();

        match header.protocol {
            protocol::ICMP => {
                if let Some(message) = IcmpMessage::parse(header.payload(payload)) {
                    match message.kind {
                        IcmpKind::EchoRequest if is_local => {
                            events.push(StackEvent::IcmpEchoRequest {
                                source: header.source,
                                identifier: message.identifier,
                                sequence: message.sequence,
                            });
                            if let Some(event) = self.build_echo_reply(&header, &message) {
                                events.push(event);
                            }
                        }
                        IcmpKind::EchoReply => events.push(StackEvent::IcmpEchoReply {
                            source: header.source,
                            sequence: message.sequence,
                        }),
                        _ => events.push(StackEvent::FrameDropped { reason: "icmp-kind" }),
                    }
                } else {
                    events.push(StackEvent::FrameDropped { reason: "icmp-malformed" });
                }
            }
            protocol::UDP => {
                if let Some(datagram) =
                    UdpDatagram::parse(header.payload(payload), header.source, header.destination, false)
                {
                    events.push(StackEvent::UdpReceived {
                        source: header.source,
                        source_port: datagram.source_port,
                        destination_port: datagram.destination_port,
                        payload_length: datagram.payload.len(),
                    });
                } else {
                    events.push(StackEvent::FrameDropped { reason: "udp-malformed" });
                }
            }
            protocol::TCP => {
                if let Some(segment) = TcpSegment::parse(header.payload(payload), header.source, header.destination) {
                    events.push(StackEvent::TcpSegment {
                        source: header.source,
                        flags_text: segment.flags.text(),
                        destination_port: segment.destination_port,
                    });
                } else {
                    events.push(StackEvent::FrameDropped { reason: "tcp-checksum" });
                }
            }
            _ => events.push(StackEvent::FrameDropped { reason: "protocol" }),
        }
    }

    fn build_echo_reply(&mut self, request_header: &Ipv4Header, message: &IcmpMessage<'_>) -> Option<StackEvent> {
        let (ident, seq, payload) = IcmpMessage::echo_reply_for(message);
        let mut icmp_buf = [0u8; 8 + 64];
        let icmp_len = IcmpMessage::build_echo(IcmpKind::EchoReply, ident, seq, payload, &mut icmp_buf)?;

        let mut ip_buf = [0u8; 20];
        let ip_len = Ipv4Header::build(
            request_header.destination,
            request_header.source,
            protocol::ICMP,
            icmp_len,
            self.next_ip_id,
            64,
            &mut ip_buf,
        )?;
        self.next_ip_id = self.next_ip_id.wrapping_add(1);

        let dst_mac = self
            .arp
            .lookup(request_header.source.0)
            .unwrap_or(MacAddress::BROADCAST);
        let src_mac = self
            .interface_for(request_header.destination)
            .map(|i| i.mac)
            .unwrap_or(MacAddress::ZERO);

        let mut frame_buf = [0u8; 14 + 20 + 8 + 64];
        frame_buf[14..14 + ip_len].copy_from_slice(&ip_buf[..ip_len]);
        let end = 14 + ip_len + icmp_len;
        frame_buf[14 + ip_len..end].copy_from_slice(&icmp_buf[..icmp_len]);

        // Copy the assembled packet out first: `build` borrows the output
        // buffer mutably, so it cannot source from it in place.
        let packet = frame_buf[14..end].to_vec();
        let total = EthernetFrame::build(
            dst_mac,
            src_mac,
            EtherType::Ipv4,
            &packet,
            &mut frame_buf,
        )?;
        self.pending_tx.push(frame_buf[..total].to_vec());
        Some(StackEvent::IcmpEchoReply {
            source: request_header.destination,
            sequence: seq,
        })
    }

    /// Queue an ARP request for `target` on the interface routing to it.
    pub fn send_arp_request(&mut self, target: Ipv4Address) -> bool {
        let Some((interface, _next_hop)) = self.route(target) else {
            return false;
        };
        let our_mac = interface.mac;
        let our_ip = interface.address;
        let request = ArpPacket {
            operation: ArpOperation::Request,
            sender_mac: our_mac,
            sender_ip: our_ip.0,
            target_mac: MacAddress::ZERO,
            target_ip: target.0,
        };
        let mut buf = [0u8; crate::arp::PACKET_LEN];
        let Some(len) = request.build(&mut buf) else {
            return false;
        };
        let mut frame_buf = [0u8; 14 + crate::arp::PACKET_LEN];
        let Some(total) =
            EthernetFrame::build(MacAddress::BROADCAST, our_mac, EtherType::Arp, &buf[..len], &mut frame_buf)
        else {
            return false;
        };
        self.pending_tx.push(frame_buf[..total].to_vec());
        true
    }

    /// Queue an ICMP echo request to `target` (requires the target MAC in
    /// the ARP cache; call [`NetworkStack::send_arp_request`] first).
    pub fn send_icmp_echo_request(&mut self, target: Ipv4Address, identifier: u16, sequence: u16) -> bool {
        let Some(dst_mac) = self.arp.lookup(target.0) else {
            return false;
        };
        let Some((src_ip, src_mac)) = self.route(target).map(|(i, _)| (i.address, i.mac)) else {
            return false;
        };

        let mut icmp_buf = [0u8; 8 + 32];
        let icmp_len = match IcmpMessage::build_echo(
            IcmpKind::EchoRequest,
            identifier,
            sequence,
            b"orbita-ping-0123456789abcdef",
            &mut icmp_buf,
        ) {
            Some(len) => len,
            None => return false,
        };

        let mut ip_buf = [0u8; 20];
        let ip_len = match Ipv4Header::build(
            src_ip,
            target,
            protocol::ICMP,
            icmp_len,
            self.next_ip_id,
            64,
            &mut ip_buf,
        ) {
            Some(len) => len,
            None => return false,
        };
        self.next_ip_id = self.next_ip_id.wrapping_add(1);

        let mut frame_buf = [0u8; 14 + 20 + 8 + 32];
        let end = 14 + ip_len + icmp_len;
        frame_buf[14..14 + ip_len].copy_from_slice(&ip_buf[..ip_len]);
        frame_buf[14 + ip_len..end].copy_from_slice(&icmp_buf[..icmp_len]);
        let packet = frame_buf[14..end].to_vec();
        let Some(total) = EthernetFrame::build(dst_mac, src_mac, EtherType::Ipv4, &packet, &mut frame_buf) else {
            return false;
        };
        self.pending_tx.push(frame_buf[..total].to_vec());
        true
    }

    /// Take one queued TX frame (caller transmits it through the NIC).
    pub fn take_tx_frame(&mut self) -> Option<Vec<u8>> {
        if self.pending_tx.is_empty() {
            None
        } else {
            Some(self.pending_tx.remove(0))
        }
    }

    /// Boot summary for the kernel log.
    pub fn summary(&self) -> String {
        let mut out = format!("net: interfaces={}", self.interfaces.len());
        for interface in &self.interfaces {
            out.push_str(&format!(" [{}]", interface.summary()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn nic() -> NicInfo {
        NicInfo {
            pci_address: String::from("00:02.0"),
            driver: NicDriverKind::IntelE1000,
            mac: MacAddress::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]),
            status: crate::nic::NicStatus::Up { speed_mbps: 1000 },
        }
    }

    fn stack_with_nic() -> NetworkStack {
        let mut stack = NetworkStack::new();
        stack.add_interface(NetworkInterface::from_nic(
            &nic(),
            Ipv4Address::new([10, 0, 2, 15]),
            Some(Ipv4Address::new([10, 0, 2, 2])),
            24,
        ));
        stack
    }

    /// Builds a full ARP request frame aimed at our IP.
    fn arp_request_frame() -> Vec<u8> {
        let request = ArpPacket {
            operation: ArpOperation::Request,
            sender_mac: MacAddress::new([0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA]),
            sender_ip: [10, 0, 2, 2],
            target_mac: MacAddress::ZERO,
            target_ip: [10, 0, 2, 15],
        };
        let mut arp_buf = [0u8; crate::arp::PACKET_LEN];
        let arp_len = request.build(&mut arp_buf).unwrap();
        let mut frame_buf = vec![0u8; 14 + crate::arp::PACKET_LEN];
        let total = EthernetFrame::build(
            MacAddress::BROADCAST,
            request.sender_mac,
            EtherType::Arp,
            &arp_buf[..arp_len],
            &mut frame_buf,
        )
        .unwrap();
        frame_buf.truncate(total);
        frame_buf
    }

    #[test]
    fn arp_request_generates_reply() {
        let mut stack = stack_with_nic();
        let events = stack.receive(&arp_request_frame());
        assert!(events
            .iter()
            .any(|e| matches!(e, StackEvent::ArpRequestForUs { .. })));
        assert_eq!(stack.pending_tx.len(), 1);
        // The queued reply must parse back as ARP reply.
        let reply_frame = EthernetFrame::parse(&stack.pending_tx[0]).unwrap();
        assert_eq!(reply_frame.ether_type, EtherType::Arp);
        let reply = ArpPacket::parse(reply_frame.payload).unwrap();
        assert_eq!(reply.operation, ArpOperation::Reply);
        assert_eq!(reply.target_ip, [10, 0, 2, 2]);
        assert_eq!(reply.sender_mac.0, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    }

    #[test]
    fn routing_prefers_on_link_then_gateway() {
        let stack = stack_with_nic();
        let (interface, next) = stack.route(Ipv4Address::new([10, 0, 2, 99])).unwrap();
        assert_eq!(interface.name.as_str(), "eth-00:02.0");
        assert_eq!(next, Ipv4Address::new([10, 0, 2, 99]));
        let (_, next) = stack.route(Ipv4Address::new([8, 8, 8, 8])).unwrap();
        assert_eq!(next, Ipv4Address::new([10, 0, 2, 2]));
    }

    #[test]
    fn summary_lists_interfaces() {
        let stack = stack_with_nic();
        let s = stack.summary();
        assert!(s.contains("interfaces=2"));
        assert!(s.contains("lo 127.0.0.1/8"));
        assert!(s.contains("10.0.2.15/24 gw=10.0.2.2"));
    }
}
