#![no_std]
#![no_main]

use orbita_sdk::println;
use orbita_sdk::sys::fs;

orbita_sdk::entry! {
    fn main() -> i32 {
        println!("hello from a native rust app on orbita");
        if fs::write("/home/hello-note.txt", b"written by native app hello").is_ok() {
            println!("wrote /home/hello-note.txt");
        }
        if let Ok(readback) = fs::read_text("/home/hello-note.txt") {
            println!("read back: {readback}");
        }
        0
    }
}
