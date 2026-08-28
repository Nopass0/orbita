//! `syscall`/`sysret` gate (stage A, roadmap A.5).
//!
//! v1 contract (single CPU, no per-CPU GS yet — documented limitation):
//!
//! * user → kernel: `rax` = syscall number, args in `rdi`, `rsi`
//!   (SysV side of the future SDK); `rcx`/`r11` carry the return
//!   `rip`/`rflags` (hardware convention).
//! * kernel → user: result in `rax`.
//! * STAR: kernel CS `0x08`, user base `0x18` → SYSRET loads
//!   `CS=0x2B`, `SS=0x23` (matches [`crate::gdt`]).
//! * FMASK clears IF for the whole kernel side of the gate.
//!
//! The gate installs a dedicated kernel stack (TSS.rsp0 points at the
//! same buffer, so interrupts from ring 3 land on a valid stack).

use core::arch::global_asm;

use crate::msr;

/// MSR addresses for the syscall gate.
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_FMASK: u32 = 0xC000_0084;
const MSR_EFER: u32 = 0xC000_0080;
/// EFER bit 0: SYSCALL/SYSRET enable.
const EFER_SCE: u64 = 1;
/// Flags cleared on syscall entry: IF + the always-1 bit.
const SYSCALL_FLAG_MASK: u64 = 0x202;

/// Sentinel returned by the dispatcher for "ring-3 roundtrip finished" —
/// the asm epilogue unwinds to the saved kernel context instead of
/// executing `sysret`.
const SYSCALL_DONE_RESULT: u64 = 0xFFFF_FFFF_FFFF_F000;

/// Test syscall: echoes `arg1 | 1` back to ring 3.
pub const SYSCALL_ECHO: u64 = 0x1000;
/// Test syscall: finishes the ring-3 self-test, resumes the kernel.
pub const SYSCALL_DONE: u64 = 0x1001;

global_asm!(
    r#"
    .section .bss
    .align 16
orbita_syscall_kernel_stack:
    .zero 16384
orbita_syscall_kernel_stack_end:
    .align 8
orbita_syscall_user_rsp:
    .zero 8
    .align 8
orbita_ring3_saved_krsp:
    .zero 8
    .section .text

    .global orbita_x86_64_syscall_entry
orbita_x86_64_syscall_entry:
    # rax = nr, rdi/rsi = args (SysV), rcx = user rip, r11 = user rflags.
    mov [rip + orbita_syscall_user_rsp], rsp
    lea rsp, [rip + orbita_syscall_kernel_stack_end]
    push rcx
    push r11
    mov rcx, rax        # nr    -> Win64 arg1
    mov rdx, rdi        # arg1  -> Win64 arg2
    mov r8,  rsi        # arg2  -> Win64 arg3
    sub rsp, 32         # shadow space (keeps 16-byte alignment)
    call orbita_x86_64_syscall_dispatch
    add rsp, 32
    cmp rax, -4096
    je orbita_syscall_done_path
    pop r11
    pop rcx
    mov rsp, [rip + orbita_syscall_user_rsp]
    sysretq
orbita_syscall_done_path:
    # Ring-3 self-test finished: resume the kernel context that entered
    # the test. Callee-saved registers were never clobbered on this path.
    mov rsp, [rip + orbita_ring3_saved_krsp]
    xor eax, eax
    ret

    .global orbita_x86_64_enter_ring3
orbita_x86_64_enter_ring3:
    # rcx = user rip, rdx = user rsp (Win64 args). Never returns to its
    # caller directly - control comes back through SYSCALL_DONE.
    mov [rip + orbita_ring3_saved_krsp], rsp
    push 0x23           # user SS
    push rdx            # user RSP
    push 0x202          # RFLAGS: interrupts enabled
    push 0x2B           # user CS
    push rcx            # user RIP
    iretq
    "#,
);

unsafe extern "C" {
    fn orbita_x86_64_syscall_entry() -> !;
    fn orbita_x86_64_enter_ring3(user_rip: u64, user_rsp: u64) -> u64;
}

/// Number of syscalls dispatched (telemetry).
static SYSCALL_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// v1 syscall dispatcher: the ring-3 self-test pair. The SDK table
/// migration (read/write/mem/time/spawn/exit) lands with user ELF loading
/// (roadmap A.5/A.6 continuation).
#[unsafe(no_mangle)]
unsafe extern "C" fn orbita_x86_64_syscall_dispatch(nr: u64, arg1: u64, _arg2: u64) -> u64 {
    SYSCALL_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    match nr {
        SYSCALL_ECHO => {
            let mut serial = crate::serial::SerialPort::com1();
            serial.write_line("ring3: syscall echo received (kernel side)");
            arg1.wrapping_add(1)
        }
        SYSCALL_DONE => {
            let mut serial = crate::serial::SerialPort::com1();
            serial.write_line("ring3: done syscall — resuming kernel context");
            SYSCALL_DONE_RESULT
        }
        _ => {
            let mut serial = crate::serial::SerialPort::com1();
            serial.write_line("ring3: unknown syscall ignored");
            u64::MAX
        }
    }
}

/// Programs STAR/LSTAR/FMASK and enables EFER.SCE.
pub fn install_syscall_gate() {
    // SAFETY: MSR writes during early single-CPU boot; EFER is modified
    // read-modify-write to keep the NXE state `abi::disable_nx` left.
    unsafe {
        msr::write(MSR_STAR, (crate::gdt::KERNEL_CODE_SELECTOR as u64) << 32
            | (crate::gdt::USER_CODE32_SELECTOR as u64));
        msr::write(MSR_LSTAR, orbita_x86_64_syscall_entry as *const () as u64);
        msr::write(MSR_FMASK, SYSCALL_FLAG_MASK);
        let efer = msr::read(MSR_EFER);
        msr::write(MSR_EFER, efer | EFER_SCE);
    }
}

/// Enters ring 3 at `user_rip` with `user_rsp`. Returns only when a ring-3
/// `SYSCALL_DONE` hands control back (the self-test protocol); 0 = ok.
///
/// # Safety
/// Caller guarantees USER-mapped, executable code at `user_rip` and a
/// mapped, 16-byte aligned `user_rsp`, with the GDT/TSS installed.
pub unsafe fn ring3_roundtrip(user_rip: u64, user_rsp: u64) -> u64 {
    // SAFETY: see contract above; the DONE path resumes this frame's caller.
    unsafe { orbita_x86_64_enter_ring3(user_rip, user_rsp) }
}
/// Syscalls dispatched since boot (boot log telemetry).
pub fn syscall_count() -> u64 {
    SYSCALL_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}
