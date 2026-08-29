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
        // Stage-D portion 3: a real TCP roundtrip from user space —
        // connect to the kernel's echo service on 127.0.0.1:9090.
        match net::TcpStream::connect("127.0.0.1:9090") {
            Ok(stream) => {
                stream.write(b"orbita-net").ok();
                let mut buf = [0u8; 64];
                let mut got = 0usize;
                for _ in 0..8 {
                    match stream.read(&mut buf) {
                        Ok(n) if n > 0 => {
                            got = n;
                            break;
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                if got == b"orbita-net".len()
                    && &buf[..got] == b"orbita-net"
                {
                    println!("[app] net tcp echo ok: {}", core::str::from_utf8(&buf[..got]).unwrap_or("?"));
                } else {
                    println!("[app] net tcp echo mismatch ({} bytes)", got);
                }
                stream.close();
            }
            Err(err) => println!("[app] net tcp connect failed: {err:?}"),
        }
        match fs::list_dir("/") {
            Ok(entries) => println!("root entries: {}", entries.join(", ")),
            Err(err) => println!("root listing failed: {err:?}"),
        }
        0
    }
}
