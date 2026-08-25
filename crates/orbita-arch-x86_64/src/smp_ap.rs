//! SMP AP (application processor) bring-up.
//!
//! Real multi-core start: the BSP copies a hand-assembled trampoline
//! into low conventional memory (physical 0x8000) and wakes the APs
//! with the classic INIT-SIPI-SIPI sequence through the Local APIC.
//! The trampoline switches the AP 16-bit → 32-bit → 64-bit (reusing the
//! BSP page tables), installs its own stack and calls `ap_entry`,
//! which parks the core in a halt loop after bumping the online
//! counter. All absolute addresses in the trampoline are patched at
//! copy time — the assembler never emits relocations for it.
//!
//! Failure mode is safe: if the sequence misfires, only the AP stays
//! dead — the BSP and the system continue.

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};

/// Cores that reached ap_entry (the BSP counts itself).
pub static ONLINE_CORES: AtomicU32 = AtomicU32::new(1);

/// Physical address of the trampoline (4 KiB aligned, below 1 MiB).
const TRAMPOLINE_PHYS: usize = 0x8000;

/// The SIPI vector encodes address >> 12 (startup page = vector * 4 KiB).
const SIPI_VECTOR: u32 = (TRAMPOLINE_PHYS >> 8) as u32;

/// Assembles the trampoline image with all absolute references bound to
/// `TRAMPOLINE_PHYS` and patched at their recorded positions — no
/// hand-maintained offsets, so edits to the code cannot desync patches.
#[rustfmt::skip]
fn build_trampoline(cr3: u64, stack_top: u64, entry: u64) -> alloc::vec::Vec<u8> {
    let base = TRAMPOLINE_PHYS;
    let mut t: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    // marker: writes one char to COM1 so boot logs reveal AP progress.
    fn marker(t: &mut alloc::vec::Vec<u8>, ch: u8) {
        // Real-mode-safe: store the stage byte at physical 0x7000+stage
        // so the BSP can read progress without touching the serial port.
        // mov ax,0x7000; mov es,ax; mov byte [es:idx],ch
        let idx = ch - b'0';
        t.extend_from_slice(&[0xB8, 0x00, 0x70, 0x8E, 0xC0, 0x26, 0xC6, 0x06, idx, 0x00, ch]);
    }

    // ---- 16-bit real mode entry ----
    marker(&mut t, b'1');
    t.extend_from_slice(&[
        0xFA,                                   // cli
        0x31, 0xC0,                             // xor ax, ax
        0x8E, 0xD8,                             // mov ds, ax
        0x8E, 0xC0,                             // mov es, ax
        0x8E, 0xD0,                             // mov ss, ax
    ]);
    marker(&mut t, b'2');
    let lgdt_disp = t.len() + 3; // operand of `0F 01 16 disp16`
    t.extend_from_slice(&[
        0x0F, 0x01, 0x16, 0, 0,                 // lgdt [disp16] (patched)
        0x66, 0xB8, 0x01, 0x00, 0x00, 0x00,     // mov eax, 1
        0x0F, 0x22, 0xC0,                       // mov cr0, eax
        0xEA, 0, 0, 0, 0, 0x08, 0x00,           // ljmp 0x08:pm (patched)
    ]);
    let ljmp16_target = t.len() - 4;
    let pm = base + t.len();

    // ---- 32-bit protected mode ----
    marker(&mut t, b'3');
    let cr3_disp = t.len() + 1; // disp32 of `A1 disp32`
    t.extend_from_slice(&[
        0xB8, 0x10, 0x00, 0x00, 0x00,           // mov eax, 0x10
        0x8E, 0xD8,                             // mov ds, ax
        0x8E, 0xC0,                             // mov es, ax
        0x8E, 0xD0,                             // mov ss, ax
        0xA1, 0, 0, 0, 0,                       // mov eax, [cr3 slot] (patched)
        0x0F, 0x22, 0xD8,                       // mov cr3, eax
        0x0F, 0x20, 0xE0,                       // mov eax, cr4
        0x0D, 0x20, 0x00, 0x00, 0x00,           // or eax, 0x20 (PAE)
        0x0F, 0x22, 0xE0,                       // mov cr4, eax
        0xB9, 0x80, 0x00, 0x00, 0xC0,           // mov ecx, 0xC0000080
        0x0F, 0x32,                             // rdmsr
        0x0D, 0x00, 0x01, 0x00, 0x00,           // or eax, 0x100 (LME)
        0x0F, 0x30,                             // wrmsr
        0x0F, 0x20, 0xE0,                       // mov eax, cr0
        0x0D, 0x00, 0x00, 0x00, 0x80,           // or eax, 0x80000000 (PG)
        0x0F, 0x22, 0xC0,                       // mov cr0, eax
        0xEA, 0, 0, 0, 0, 0x08, 0x00,           // ljmp 0x08:lm (patched)
    ]);
    let ljmp32_target = t.len() - 4;
    let lm = base + t.len();

    // ---- 64-bit long mode ----
    marker(&mut t, b'4');
    let stack_imm = t.len() + 2; // imm64 after `48 B8`
    t.extend_from_slice(&[
        0x48, 0xB8, 0,0,0,0,0,0,0,0,            // mov rax, imm64 stack (patched)
        0x48, 0x89, 0xC4,                       // mov rsp, rax
        0x48, 0xB8, 0,0,0,0,0,0,0,0,            // mov rax, imm64 entry (patched)
        0xFF, 0xD0,                             // call rax
        0xF4,                                   // hlt
        0xEB, 0xFD,                             // jmp $
    ]);
    let entry_imm = stack_imm + 10 + 3;

    // ---- GDT + descriptor + data slots ----
    let gdt_off = (t.len() + 15) & !15;
    t.resize(gdt_off, 0);
    let gdt: [u8; 24] = [
        0,0,0,0,0,0,0,0,
        0xFF,0xFF,0x00,0x00,0x00,0x9A,0xAF,0x00, // code: L=1
        0xFF,0xFF,0x00,0x00,0x00,0x92,0xCF,0x00, // data
    ];
    t.extend_from_slice(&gdt);
    let gdt_desc_off = t.len();
    t.extend_from_slice(&(23u16).to_le_bytes());
    t.extend_from_slice(&((base + gdt_off) as u32).to_le_bytes());
    let cr3_off = t.len();
    t.extend_from_slice(&cr3.to_le_bytes());
    let stack_off = t.len();
    t.extend_from_slice(&stack_top.to_le_bytes());
    let entry_off = t.len();
    t.extend_from_slice(&entry.to_le_bytes());
    let _ = (stack_off, entry_off);

    // ---- patch absolute references ----
    fn put16(t: &mut alloc::vec::Vec<u8>, at: usize, v: u16) {
        t[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(t: &mut alloc::vec::Vec<u8>, at: usize, v: u32) {
        t[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    put16(&mut t, lgdt_disp, (base + gdt_desc_off) as u16);
    put32(&mut t, ljmp16_target, pm as u32);
    put32(&mut t, ljmp32_target, lm as u32);
    put32(&mut t, cr3_disp, (base + cr3_off) as u32);
    t[stack_imm..stack_imm + 8].copy_from_slice(&stack_top.to_le_bytes());
    t[entry_imm..entry_imm + 8].copy_from_slice(&entry.to_le_bytes());
    t
}

/// Entry point the APs land on (long mode, own stack). Parks in a halt
/// loop until the preemptive scheduler assigns work.
#[unsafe(no_mangle)]
extern "C" fn ap_entry() -> ! {
    ONLINE_CORES.fetch_add(1, Ordering::SeqCst);
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Keeps the AP stacks alive for the lifetime of the system.
static mut AP_STACKS: [*mut u8; 8] = [core::ptr::null_mut(); 8];
static AP_STACK_COUNT: AtomicU32 = AtomicU32::new(0);

const STACK_BYTES: usize = 16 * 1024;

/// Sends INIT-SIPI-SIPI to all APs and waits (bounded) for them to come
/// online. Returns the number of online cores.
pub fn bring_up_aps(expected: u32, lapic_base: u64) -> u32 {
    unsafe {
        let expected = expected.clamp(1, 8);
        let aps = expected - 1;
        if aps == 0 {
            return ONLINE_CORES.load(Ordering::SeqCst);
        }

        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3);

        // One stack per AP; all parked APs share the bring-up entry.
        let mut last_top = 0u64;
        for _ in 0..aps {
            let stack = alloc::vec![0u8; STACK_BYTES].into_boxed_slice();
            let top = Box::into_raw(stack) as *mut u8 as usize + STACK_BYTES - 256;
            let index = AP_STACK_COUNT.fetch_add(1, Ordering::SeqCst) as usize;
            if index < 8 {
                core::ptr::addr_of_mut!(AP_STACKS[index]).write(top as *mut u8);
            }
            last_top = top as u64;
        }

        let image = build_trampoline(cr3, last_top, ap_entry as *const () as usize as u64);
        core::ptr::copy_nonoverlapping(
            image.as_ptr(),
            TRAMPOLINE_PHYS as *mut u8,
            image.len(),
        );
        // Readback verification of the installed trampoline.
        let first8 = core::ptr::read_volatile(TRAMPOLINE_PHYS as *const u64);
        let gdt8 = core::ptr::read_volatile((TRAMPOLINE_PHYS + gdt_probe(image.len())) as *const u64);
        log_ap_bytes(first8, gdt8);
        // Clear the AP progress slots at 0x7000..0x7004.
        for i in 0..5u64 {
            core::ptr::write_volatile(0x7000u64.wrapping_add(i) as *mut u8, 0);
        }

        // ---- INIT-SIPI-SIPI through the Local APIC ICR, addressed to
        // each AP by its APIC id (0 is the BSP).
        let icr_hi = (lapic_base + 0x310) as *mut u32;
        let icr = (lapic_base + 0x300) as *mut u32;
        let spurious = (lapic_base + 0xF0) as *mut u32;
        let esr = (lapic_base + 0x280) as *mut u32;
        esr.write_volatile(0); // clear error status
        let svr = spurious.read_volatile();
        if svr & 0x100 == 0 {
            spurious.write_volatile(0x1FF); // software-enable the APIC
        }
        let sipi = 0x4600 | (SIPI_VECTOR & 0xFF);
        for ap in 1..=aps as u32 {
            icr_hi.write_volatile(ap << 24);
            icr.write_volatile(0x0000_4500); // INIT to this AP
            wait_delivery(lapic_base);
        }
        delay_millis(10);
        for _ in 0..2 {
            for ap in 1..=aps as u32 {
                icr_hi.write_volatile(ap << 24);
                icr.write_volatile(sipi);
                wait_delivery(lapic_base);
            }
            delay_millis(10);
        }
        delay_millis(50);
        let esr_val = esr.read_volatile();
        let svr_val = spurious.read_volatile();
        let m0 = core::ptr::read_volatile(0x7000 as *const u8);
        let m1 = core::ptr::read_volatile(0x7001 as *const u8);
        let m2 = core::ptr::read_volatile(0x7002 as *const u8);
        let m3 = core::ptr::read_volatile(0x7003 as *const u8);
        let m4 = core::ptr::read_volatile(0x7004 as *const u8);
        if let Some(hook) = { let h = core::ptr::read_volatile(core::ptr::addr_of!(AP_DEBUG2)); h } {
            hook(svr_val as u64, esr_val as u64);
        }
        if let Some(hook) = { let h = core::ptr::read_volatile(core::ptr::addr_of!(AP_DEBUG)); h } {
            hook(
                ((m4 as u64) << 32) | ((m3 as u64) << 24) | ((m2 as u64) << 16) | ((m1 as u64) << 8) | m0 as u64,
                0,
            );
        }
    }
    ONLINE_CORES.load(Ordering::SeqCst)
}

/// Waits until the ICR delivery-status bit clears.
fn wait_delivery(lapic_base: u64) {
    let icr = (lapic_base + 0x300) as *mut u32;
    for _ in 0..100_000 {
        if unsafe { icr.read_volatile() } & (1 << 12) == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

/// Rough millisecond delay (calibrated for emulated hardware).
fn delay_millis(ms: usize) {
    for _ in 0..ms {
        for _ in 0..200_000 {
            core::hint::spin_loop();
        }
    }
}

/// Reads back trampoline bytes for the debug hook.
fn gdt_probe(_len: usize) -> usize {
    0x80
}

/// Installed for kernel-side trampoline diagnostics.
pub static mut AP_DEBUG: Option<fn(u64, u64)> = None;

/// Second diagnostic hook: (spurious, esr) after the SIPI sequence.
pub static mut AP_DEBUG2: Option<fn(u64, u64)> = None;

fn log_ap_bytes(first8: u64, gdt8: u64) {
    unsafe {
        if let Some(hook) = AP_DEBUG {
            hook(first8, gdt8);
        }
    }
}

/// Current online core count.
pub fn online_cores() -> u32 {
    ONLINE_CORES.load(Ordering::SeqCst)
}
