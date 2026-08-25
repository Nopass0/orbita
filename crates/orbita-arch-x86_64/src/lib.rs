#![no_std]

pub mod smp_ap;

pub mod cpu {
    use core::arch::{asm, global_asm};

    // The permanent halt path lives in assembly so panic and fatal boot
    // failures can jump into a minimal, deterministic stop loop.
    global_asm!(
        r#"
        .global orbita_x86_64_halt_forever
        .global orbita_x86_64_irq_stub
        .global orbita_x86_64_timer_irq_stub
        .global orbita_x86_64_spurious_irq_stub
    orbita_x86_64_halt_forever:
        cli
    2:
        hlt
        jmp 2b

    orbita_x86_64_irq_stub:
        iretq

    orbita_x86_64_timer_irq_stub:
        iretq

    orbita_x86_64_spurious_irq_stub:
        iretq
    "#
    );

    unsafe extern "C" {
        fn orbita_x86_64_halt_forever() -> !;
        fn orbita_x86_64_irq_stub() -> !;
        fn orbita_x86_64_timer_irq_stub() -> !;
        fn orbita_x86_64_spurious_irq_stub() -> !;
    }

    #[inline]
    pub fn pause() {
        unsafe {
            asm!("pause", options(nomem, nostack, preserves_flags));
        }
    }

    #[inline]
    pub fn disable_interrupts() {
        unsafe {
            asm!("cli", options(nomem, nostack, preserves_flags));
        }
    }

    #[inline]
    pub fn enable_interrupts() {
        unsafe {
            asm!("sti", options(nomem, nostack, preserves_flags));
        }
    }

    #[inline]
    pub fn halt_forever() -> ! {
        unsafe { orbita_x86_64_halt_forever() }
    }

    #[inline]
    pub fn irq_stub_addr() -> u64 {
        orbita_x86_64_irq_stub as *const () as usize as u64
    }

    #[inline]
    pub fn timer_irq_stub_addr() -> u64 {
        orbita_x86_64_timer_irq_stub as *const () as usize as u64
    }

    #[inline]
    pub fn spurious_irq_stub_addr() -> u64 {
        orbita_x86_64_spurious_irq_stub as *const () as usize as u64
    }
}

pub mod serial {
    use core::arch::asm;
    use core::fmt::{self, Write};

    const COM1_BASE: u16 = 0x3F8;
    const DATA: u16 = COM1_BASE;
    const INTERRUPT_ENABLE: u16 = COM1_BASE + 1;
    const FIFO_CONTROL: u16 = COM1_BASE + 2;
    const LINE_CONTROL: u16 = COM1_BASE + 3;
    const MODEM_CONTROL: u16 = COM1_BASE + 4;
    const LINE_STATUS: u16 = COM1_BASE + 5;

    #[inline]
    unsafe fn out8(port: u16, value: u8) {
        unsafe {
            asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags),
            );
        }
    }

    #[inline]
    unsafe fn in8(port: u16) -> u8 {
        let value: u8;
        unsafe {
            asm!(
                "in al, dx",
                in("dx") port,
                out("al") value,
                options(nomem, nostack, preserves_flags),
            );
        }
        value
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SerialPort {
        base: u16,
    }

    impl SerialPort {
        pub const fn com1() -> Self {
            Self { base: COM1_BASE }
        }

        pub fn init(&mut self) {
            unsafe {
                out8(INTERRUPT_ENABLE, 0x00);
                out8(LINE_CONTROL, 0x80);
                out8(DATA, 0x03);
                out8(INTERRUPT_ENABLE, 0x00);
                out8(LINE_CONTROL, 0x03);
                out8(FIFO_CONTROL, 0xC7);
                out8(MODEM_CONTROL, 0x0B);
            }
        }

        pub fn write_byte(&mut self, byte: u8) {
            while unsafe { in8(LINE_STATUS) } & 0x20 == 0 {}
            unsafe { out8(self.base, byte) };
        }

        pub fn write_line(&mut self, message: &str) {
            let _ = self.write_str(message);
            self.write_byte(b'\r');
            self.write_byte(b'\n');
        }
    }

    impl Write for SerialPort {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for byte in s.bytes() {
                if byte == b'\n' {
                    self.write_byte(b'\r');
                }
                self.write_byte(byte);
            }
            Ok(())
        }
    }
}

pub mod io {
    use core::arch::asm;
    use core::marker::PhantomData;

    pub trait PortValue: Copy {
        unsafe fn read(port: u16) -> Self;
        unsafe fn write(port: u16, value: Self);
    }

    impl PortValue for u8 {
        unsafe fn read(port: u16) -> Self {
            let value: u8;
            unsafe {
                asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
            }
            value
        }

        unsafe fn write(port: u16, value: Self) {
            unsafe {
                asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
            }
        }
    }

    impl PortValue for u16 {
        unsafe fn read(port: u16) -> Self {
            let value: u16;
            unsafe {
                asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack, preserves_flags));
            }
            value
        }

        unsafe fn write(port: u16, value: Self) {
            unsafe {
                asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
            }
        }
    }

    impl PortValue for u32 {
        unsafe fn read(port: u16) -> Self {
            let value: u32;
            unsafe {
                asm!("in eax, dx", in("dx") port, out("eax") value, options(nomem, nostack, preserves_flags));
            }
            value
        }

        unsafe fn write(port: u16, value: Self) {
            unsafe {
                asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
            }
        }
    }

    pub struct Port<T: PortValue> {
        port: u16,
        _marker: PhantomData<T>,
    }

    impl<T: PortValue> Port<T> {
        pub const fn new(port: u16) -> Self {
            Self {
                port,
                _marker: PhantomData,
            }
        }

        pub unsafe fn read(&mut self) -> T {
            unsafe { T::read(self.port) }
        }

        pub unsafe fn write(&mut self, value: T) {
            unsafe { T::write(self.port, value) }
        }
    }
}

pub mod msr {
    use core::arch::asm;

    pub unsafe fn read(msr: u32) -> u64 {
        let low: u32;
        let high: u32;
        unsafe {
            asm!(
                "rdmsr",
                in("ecx") msr,
                out("eax") low,
                out("edx") high,
                options(nomem, nostack, preserves_flags)
            );
        }
        ((high as u64) << 32) | low as u64
    }

    pub unsafe fn write(msr: u32, value: u64) {
        let low = value as u32;
        let high = (value >> 32) as u32;
        unsafe {
            asm!(
                "wrmsr",
                in("ecx") msr,
                in("eax") low,
                in("edx") high,
                options(nomem, nostack, preserves_flags)
            );
        }
    }
}

pub mod tables {
    use core::arch::asm;

    #[derive(Debug, Copy, Clone)]
    #[repr(C, packed)]
    pub struct DescriptorTablePointer {
        pub limit: u16,
        pub base: u64,
    }

    pub unsafe fn load_idt(pointer: &DescriptorTablePointer) {
        unsafe {
            asm!("lidt [{}]", in(reg) pointer, options(readonly, nostack, preserves_flags));
        }
    }
}
