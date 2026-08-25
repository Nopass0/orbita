#![no_std]
#![no_main]

use orbita_sdk::println;
use orbita_sdk::sys::{fs, net, os, time};

orbita_sdk::entry! {
    fn main() -> i32 {
        println!("== orbita sysinfo ==");
        println!("{}", os::info());
        println!("boot ms: {}", time::now_ms());
        for interface in net::interfaces() {
            println!("net: {}", interface.summary);
        }
        match fs::list_dir("/") {
            Ok(entries) => println!("root entries: {}", entries.join(", ")),
            Err(err) => println!("root listing failed: {err:?}"),
        }
        0
    }
}
