//! GDT + TSS for kernel/user segmentation (stage A, roadmap A.4).
//!
//! Selector layout (matches the hardcoded `0x08` in the bootstrap IDT so
//! existing interrupt entries keep working, and matches the
//! `SYSENTER/SYSRET` STAR convention used by [`crate::syscall`]):
//!
//! | selector | segment |
//! |---|---|
//! | `0x08` | kernel code64 (L=1) |
//! | `0x10` | kernel data |
//! | `0x18` | user code32 (SYSRET base) |
//! | `0x20` | user data (ring 3: `0x23`) |
//! | `0x28` | user code64 (ring 3: `0x2B`) |
//! | `0x30` | 64-bit TSS descriptor |
//!
//! `install_kernel_gdt` loads the table, reloads the data segments and CS
//! (far return, selector-compatible with whatever the firmware used) and
//! sets TR to the TSS. The TSS carries `rsp0` (interrupt/syscall stack for
//! ring-3→ring-0 transitions) and IST1..IST3.

use core::arch::asm;

// ---------------------------------------------------------------------------
// Descriptor encoding (pure logic, host-testable).
// ---------------------------------------------------------------------------

/// Segment access byte: type (bits 0..4), S (bit 4), DPL (bits 5..6), P (bit 7).
pub mod access {
    pub const ACCESSED: u8 = 1 << 0;
    /// Data: writable / code: readable.
    pub const READ_WRITE: u8 = 1 << 1;
    /// Code vs data segment.
    pub const CODE: u8 = 1 << 3;
    /// Code/data (not system) segment.
    pub const SYSTEM: u8 = 1 << 4;
    /// TSS64 available (system descriptor, S=0).
    pub const TSS_AVAILABLE: u8 = 0b1001;
    /// Descriptor privilege level ring 3.
    pub const RING3: u8 = 3 << 5;
    /// Present.
    pub const PRESENT: u8 = 1 << 7;
}

/// Grandularity/size flags (upper 4 bits of the flags byte).
pub mod flags {
    /// 4 KiB granularity.
    pub const GRANULARITY: u8 = 1 << 3;
    /// Long mode (L bit, 64-bit code).
    pub const LONG_MODE: u8 = 1 << 1;
}

/// Encodes one 8-byte segment descriptor. Only the low 32 bits of `base`
/// fit here; 64-bit system descriptors (TSS) carry the rest in the
/// following table entry (the TSS descriptor occupies two slots).
pub const fn encode_segment(base: u64, limit: u64, access: u8, flags: u8) -> u64 {
    let limit_low = (limit & 0xFFFF) as u64;
    let base_low = (base & 0xFF_FFFF) as u64;
    let base_mid = ((base >> 24) & 0xFF) as u64;
    let limit_high = ((limit >> 16) & 0xF) as u64;
    let flags_high = (flags & 0xF) as u64;
    limit_low
        | (base_low << 16)
        | ((access as u64) << 40)
        | (limit_high << 48)
        | (flags_high << 52)
        | (base_mid << 56)
}

// ---------------------------------------------------------------------------
// TSS.
// ---------------------------------------------------------------------------

/// 64-bit task state segment (hardware layout).
#[repr(C, packed)]
pub struct TaskStateSegment {
    pub reserved0: u32,
    /// Ring 0/1/2 stacks (RSP0 is what interrupts from ring 3 use).
    pub privilege_stack_table: [u64; 3],
    pub reserved1: u64,
    /// IST1..IST7 stacks (index 0 = IST1).
    pub interrupt_stack_table: [u64; 7],
    pub reserved3: u64,
    pub reserved2: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const EMPTY: TaskStateSegment = TaskStateSegment {
        reserved0: 0,
        privilege_stack_table: [0; 3],
        reserved1: 0,
        interrupt_stack_table: [0; 7],
        reserved3: 0,
        reserved2: 0,
        iomap_base: core::mem::size_of::<TaskStateSegment>() as u16,
    };
}

/// Kernel CS selector (bootstrap IDT entries use it).
pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
/// Kernel DS/ES/SS selector.
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
/// SYSRET user base (code32; +8 data, +16 code64).
pub const USER_CODE32_SELECTOR: u16 = 0x18;
/// User data selector with RPL 3.
pub const USER_DATA_SELECTOR: u16 = 0x20 | 3;
/// User 64-bit code selector with RPL 3.
pub const USER_CODE64_SELECTOR: u16 = 0x28 | 3;
/// TSS selector (system descriptor at index 6).
pub const TSS_SELECTOR: u16 = 0x30;

const GDT_ENTRIES: usize = 8; // 6 segments + 16-byte TSS descriptor

static mut GDT: [u64; GDT_ENTRIES] = [0; GDT_ENTRIES];
static mut TSS: TaskStateSegment = TaskStateSegment::EMPTY;

/// Kernel stack for ring-3→ring-0 transitions (TSS.rsp0 + the syscall gate).
/// The array is the stack storage itself — addressed, never "read" as a field.
#[repr(align(16))]
#[allow(dead_code)]
struct KernelStack([u8; 16 * 1024]);
static mut RING0_STACK: KernelStack = KernelStack([0; 16 * 1024]);
/// IST1 — NMI/double-fault stack.
#[repr(align(16))]
#[allow(dead_code)]
struct IstStack([u8; 4 * 1024]);
static mut IST1_STACK: IstStack = IstStack([0; 4 * 1024]);
/// IST2 — page-fault stack.
#[allow(dead_code)]
static mut IST2_STACK: IstStack = IstStack([0; 4 * 1024]);
/// IST3 — syscall (future per-CPU) stack.
#[allow(dead_code)]
static mut IST3_STACK: IstStack = IstStack([0; 4 * 1024]);

#[derive(Debug, Copy, Clone)]
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Report of [`install_kernel_gdt`] (boot log telemetry).
#[derive(Debug, Copy, Clone)]
pub struct GdtReport {
    pub selectors: usize,
    pub tss_address: u64,
    pub rsp0: u64,
}

/// Loads the kernel/user GDT + TSS. Must run before any ring-3 code and
/// before interrupts are expected from ring 3 (TSS.rsp0 becomes the
/// interrupt stack). Selector numbers match the firmware conventions the
/// bootstrap IDT already relies on, so the same call site works before or
/// after `install_bootstrap_idt`.
pub fn install_kernel_gdt() -> GdtReport {
    // SAFETY: single-threaded early boot; the statics below are only
    // touched here before any concurrent access can exist.
    unsafe {
        let tss_ptr = core::ptr::addr_of_mut!(TSS);
        let tss_addr = tss_ptr as u64;
        let ring0_top = core::ptr::addr_of!(RING0_STACK) as u64 + core::mem::size_of::<KernelStack>() as u64;
        TSS.privilege_stack_table[0] = ring0_top;
        TSS.interrupt_stack_table[0] =
            core::ptr::addr_of!(IST1_STACK) as u64 + core::mem::size_of::<IstStack>() as u64;
        TSS.interrupt_stack_table[1] =
            core::ptr::addr_of!(IST2_STACK) as u64 + core::mem::size_of::<IstStack>() as u64;
        TSS.interrupt_stack_table[2] =
            core::ptr::addr_of!(IST3_STACK) as u64 + core::mem::size_of::<IstStack>() as u64;

        let kernel_code = access::PRESENT | access::SYSTEM | access::CODE | access::READ_WRITE | access::ACCESSED;
        let kernel_data = access::PRESENT | access::SYSTEM | access::READ_WRITE | access::ACCESSED;
        let user_code32 = access::PRESENT | access::SYSTEM | access::RING3 | access::CODE | access::READ_WRITE;
        let user_data = access::PRESENT | access::SYSTEM | access::RING3 | access::READ_WRITE;
        let user_code64 = access::PRESENT | access::SYSTEM | access::RING3 | access::CODE | access::READ_WRITE;

        GDT[0] = 0; // null
        GDT[1] = encode_segment(0, 0xFFFF_FFFF, kernel_code, flags::LONG_MODE);
        GDT[2] = encode_segment(0, 0xFFFF_FFFF, kernel_data, flags::GRANULARITY);
        GDT[3] = encode_segment(0, 0xFFFF_FFFF, user_code32, 0);
        GDT[4] = encode_segment(0, 0xFFFF_FFFF, user_data, flags::GRANULARITY);
        GDT[5] = encode_segment(0, 0xFFFF_FFFF, user_code64, flags::LONG_MODE);
        // TSS: 16-byte system descriptor (low = base/limit, high = base high).
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        GDT[6] = encode_segment(tss_addr, tss_limit, access::PRESENT | access::TSS_AVAILABLE, 0);
        GDT[7] = tss_addr >> 32;

        let pointer = GdtPointer {
            limit: (core::mem::size_of::<[u64; GDT_ENTRIES]>() - 1) as u16,
            base: core::ptr::addr_of!(GDT) as u64,
        };
        asm!("lgdt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));

        // Reload data segments, then CS via a far return so the running
        // selectors are decoded from OUR table (selector values are
        // firmware-compatible, but the descriptors must be ours).
        let data = KERNEL_DATA_SELECTOR;
        asm!(
            "mov ds, {0:x}",
            "mov es, {0:x}",
            "mov ss, {0:x}",
            in(reg) data,
            options(nostack, preserves_flags)
        );
        asm!(
            "push {0}",     // qword push: retfq pops 8 bytes of selector
            "lea rax, [rip + 5f]",
            "push rax",
            "retfq",
            "5:",
            in(reg) KERNEL_CODE_SELECTOR as u64,
            out("rax") _,
            options(preserves_flags)
        );

        let tss_sel = TSS_SELECTOR;
        asm!("ltr {0:x}", in(reg) tss_sel, options(nostack, preserves_flags));

        GdtReport {
            selectors: GDT_ENTRIES,
            tss_address: tss_addr,
            rsp0: ring0_top,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_descriptor_is_zero() {
        assert_eq!(encode_segment(0, 0, 0, 0), 0);
    }

    #[test]
    fn code64_descriptor_has_long_bit_and_present() {
        let entry = encode_segment(
            0,
            0xFFFF_FFFF,
            access::PRESENT | access::SYSTEM | access::CODE,
            flags::LONG_MODE,
        );
        // Present bit (byte 5 bit 7).
        assert_eq!(
            (entry >> 40) & 0xFF,
            (access::PRESENT | access::SYSTEM | access::CODE) as u64
        );
        // L flag lives in byte 6 bit 1.
        assert_eq!((entry >> 52) & 0xF, flags::LONG_MODE as u64);
        // Limit fully expanded.
        assert_eq!(entry & 0xFFFF, 0xFFFF);
        assert_eq!((entry >> 48) & 0xF, 0xF);
    }

    #[test]
    fn user_ring3_encoded_in_dpl() {
        let entry = encode_segment(
            0,
            0xFFFF_FFFF,
            access::PRESENT | access::SYSTEM | access::RING3 | access::READ_WRITE,
            flags::GRANULARITY,
        );
        assert_eq!(((entry >> 40) & 0x60) >> 5, 3, "DPL must be ring 3");
        assert_eq!((entry >> 52) & 0xF, flags::GRANULARITY as u64);
    }

    #[test]
    fn tss_descriptor_carries_base_and_limit() {
        let base = 0x00AA_BBCD_D000_1000u64;
        let limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        let low = encode_segment(base, limit, access::PRESENT | access::TSS_AVAILABLE, 0);
        // Limit split across low word and nybble at byte 6.
        assert_eq!(low & 0xFFFF, limit & 0xFFFF);
        assert_eq!((low >> 48) & 0xF, (limit >> 16) & 0xF);
        // Base split across three fields.
        assert_eq!((low >> 16) & 0xFF_FFFF, base & 0xFF_FFFF);
        assert_eq!((low >> 56) & 0xFF, (base >> 24) & 0xFF);
    }

    #[test]
    fn tss_layout_matches_hardware_offsets() {
        let tss = TaskStateSegment::EMPTY;
        let base = &tss as *const TaskStateSegment as usize;
        let rsp0 = core::ptr::addr_of!(tss.privilege_stack_table) as usize;
        let ist1 = core::ptr::addr_of!(tss.interrupt_stack_table) as usize;
        let iomap = core::ptr::addr_of!(tss.iomap_base) as usize;
        assert_eq!(rsp0 - base, 4, "RSP0 at offset 4");
        assert_eq!(ist1 - base, 4 + 8 * 4, "IST1 at offset 36");
        assert_eq!(iomap - base, 4 + 8 * 4 + 7 * 8 + 8 + 2, "IOPB at offset 102");
        assert_eq!(core::mem::size_of::<TaskStateSegment>(), 104);
    }
}
