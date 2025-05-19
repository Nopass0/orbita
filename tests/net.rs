#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(orbita::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use orbita::net::ethernet::{EthernetFrame, MacAddress};
use orbita::net::ipv4::{Ipv4Addr, Ipv4Packet};
use alloc::vec::Vec;
use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    orbita::test_panic_handler(info)
}

#[test_case]
fn test_ethernet_parse() {
    let bytes = [
        0xff,0xff,0xff,0xff,0xff,0xff,
        1,2,3,4,5,6,
        0x08,0x00,
        1,2,3,4
    ];
    let frame = EthernetFrame::from_bytes(&bytes).expect("parse");
    assert_eq!(frame.destination, MacAddress::BROADCAST);
    assert_eq!(frame.source, MacAddress([1,2,3,4,5,6]));
    assert_eq!(frame.ethertype, 0x0800);
    assert_eq!(frame.payload, &[1,2,3,4]);
}

#[test_case]
fn test_ipv4_serialize() {
    let payload = [1u8,2,3];
    let packet = Ipv4Packet {
        source: Ipv4Addr([192,168,0,1]),
        destination: Ipv4Addr([192,168,0,2]),
        protocol: 17,
        payload: &payload,
        identification: 0x1234,
        flags_fragment: 0,
        ttl: 64,
    };
    let mut out = Vec::new();
    packet.serialize(&mut out);
    assert!(out.len() >= 20 + payload.len());
}
