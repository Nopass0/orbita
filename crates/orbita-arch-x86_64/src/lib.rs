#![no_std]

pub mod smp_ap;
pub mod gdt;
pub mod syscall;

pub mod cpu {
    use core::arch::{asm, global_asm};
    use core::fmt::Write as _;

    // The permanent halt path lives in assembly so panic and fatal boot
    // failures can jump into a minimal, deterministic stop loop.
    global_asm!(
        r#"
        .global orbita_x86_64_halt_forever
        .global orbita_x86_64_irq_stub
        .global orbita_x86_64_timer_irq_stub
        .global orbita_x86_64_spurious_irq_stub
        .global orbita_x86_64_double_fault_stub
        .global orbita_x86_64_general_protection_stub
        .global orbita_x86_64_page_fault_stub
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

    // CPU-fault stubs (vectors 8/13/14 — all push an error code).
    // Win64 kernel ABI: 1st integer argument travels in rcx, 2nd in rdx.
    // At stub entry the CPU frame is [err][rip][cs][rflags][rsp][ss] and
    // rsp is 16-byte aligned; reserve the 32-byte shadow store so the
    // call lands on a conventionally aligned stack.
    orbita_x86_64_double_fault_stub:
        mov rcx, 8
        mov rdx, rsp
        sub rsp, 32
        call orbita_x86_64_on_cpu_fault
        add rsp, 32
        iretq

    orbita_x86_64_general_protection_stub:
        mov rcx, 13
        mov rdx, rsp
        sub rsp, 32
        call orbita_x86_64_on_cpu_fault
        add rsp, 32
        iretq

    orbita_x86_64_page_fault_stub:
        mov rcx, 14
        mov rdx, rsp
        sub rsp, 32
        call orbita_x86_64_on_cpu_fault
        add rsp, 32
        iretq
    "#
    );

    unsafe extern "C" {
        fn orbita_x86_64_halt_forever() -> !;
        fn orbita_x86_64_irq_stub() -> !;
        fn orbita_x86_64_timer_irq_stub() -> !;
        fn orbita_x86_64_spurious_irq_stub() -> !;
        fn orbita_x86_64_double_fault_stub() -> !;
        fn orbita_x86_64_general_protection_stub() -> !;
        fn orbita_x86_64_page_fault_stub() -> !;
    }

    /// CPU fault frame as pushed by the CPU plus the error code
    /// (vectors 8/13/14). Order matches the hardware push sequence.
    #[derive(Debug, Copy, Clone)]
    #[repr(C, packed)]
    pub struct FaultFrame {
        pub error_code: u64,
        pub rip: u64,
        pub cs: u64,
        pub rflags: u64,
        pub rsp: u64,
        pub ss: u64,
    }

    /// Name for a CPU-fault vector (8/13/14; anything else is generic).
    const fn fault_name(vector: u64) -> &'static str {
        match vector {
            8 => "#DF double fault",
            13 => "#GP general protection",
            14 => "#PF page fault",
            _ => "fault",
        }
    }

    /// Writes `value` as `width` hexadecimal digits (zero padded).
    fn write_hex(out: &mut impl core::fmt::Write, value: u64) {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut buf = [0u8; 16];
        let mut at = buf.len();
        let mut rest = value;
        loop {
            at -= 1;
            buf[at] = DIGITS[(rest & 0xF) as usize];
            rest >>= 4;
            if rest == 0 {
                break;
            }
        }
        for byte in &buf[at..] {
            let _ = out.write_char(*byte as char);
        }
    }

    /// Entry called by the CPU-fault stubs. Prints the fault to the serial
    /// console (port I/O only — safe even with broken paging) and halts.
    /// Stage-A v1: any kernel-side fault is fatal; killing the faulting
    /// *process* instead arrives with ring-3 execution (roadmap A.7).
    #[unsafe(no_mangle)]
    unsafe extern "C" fn orbita_x86_64_on_cpu_fault(vector: u64, frame: *const FaultFrame) {
        let frame = unsafe { *frame };
        let mut serial = crate::serial::SerialPort::com1();
        let _ = serial.write_str("Orbita OS FAULT: ");
        let _ = serial.write_str(fault_name(vector));
        let _ = serial.write_str(" rip=0x");
        write_hex(&mut serial, frame.rip);
        if vector == 14 {
            let _ = serial.write_str(" cr2=0x");
            write_hex(&mut serial, read_cr2());
        }
        let _ = serial.write_str(" err=0x");
        write_hex(&mut serial, frame.error_code);
        let _ = serial.write_str(" cs=0x");
        write_hex(&mut serial, frame.cs & 0xFFFF);
        let _ = serial.write_str(" rsp=0x");
        write_hex(&mut serial, frame.rsp);
        serial.write_byte(b'\r');
        serial.write_byte(b'\n');
        halt_forever()
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

    #[inline]
    pub fn double_fault_stub_addr() -> u64 {
        orbita_x86_64_double_fault_stub as *const () as usize as u64
    }

    #[inline]
    pub fn general_protection_stub_addr() -> u64 {
        orbita_x86_64_general_protection_stub as *const () as usize as u64
    }

    #[inline]
    pub fn page_fault_stub_addr() -> u64 {
        orbita_x86_64_page_fault_stub as *const () as usize as u64
    }

    /// Faulting address of the last #PF (page-fault register).
    #[inline]
    pub fn read_cr2() -> u64 {
        let value: u64;
        unsafe {
            asm!("mov {0}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
        }
        value
    }

    /// Current page-table root (CR3).
    #[inline]
    pub fn read_cr3() -> u64 {
        let value: u64;
        unsafe {
            asm!("mov {0}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
        }
        value
    }

    /// Control register CR0 (protection/paging enable bits).
    #[inline]
    pub fn read_cr4() -> u64 {
        let value: u64;
        unsafe {
            asm!("mov {0}, cr4", out(reg) value, options(nomem, nostack, preserves_flags));
        }
        value
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
