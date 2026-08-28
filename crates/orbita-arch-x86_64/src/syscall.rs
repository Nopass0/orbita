//! `syscall`/`sysret` gate (stage A, roadmap A.5).
//!
//! v1 contract (single CPU, no per-CPU GS yet — documented limitation):
//!
//! * user → kernel: `rax` = syscall number, args in `rdi`, `rsi`, `rdx`
//!   (SysV side of the SDK); `rcx`/`r11` carry the return
//!   `rip`/`rflags` (hardware convention).
//! * kernel → user: result in `rax`.
//! * STAR: kernel CS `0x08`, user base `0x18` → SYSRET loads
//!   `CS=0x2B`, `SS=0x23` (matches [`crate::gdt`]).
//! * FMASK clears IF for the whole kernel side of the gate.
//!
//! The gate installs a dedicated kernel stack (TSS.rsp0 points at the
//! same buffer, so interrupts from ring 3 land on a valid stack).
//! Ring-3 termination: the dispatcher calls [`finish_ring3`] on the EXIT
//! syscall; the asm epilogue then unwinds to the kernel context saved by
//! [`enter_ring3`] instead of executing `sysret` (the exit code travels
//! back in `rax`).

use core::arch::global_asm;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

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

/// Test syscall: echoes `a1 + 1` back to ring 3.
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
    .align 8
    .global orbita_ring3_finished
orbita_ring3_finished:
    .zero 8
    .align 8
    .global orbita_syscall_ring0
orbita_syscall_ring0:
    .zero 1
    .align 8
orbita_ring3_saved_rdi:
    .zero 8
    .align 8
orbita_ring3_saved_rsi:
    .zero 8
    .align 8
orbita_ring3_saved_rbx:
    .zero 8
    .align 8
orbita_ring3_saved_rbp:
    .zero 8
    .align 8
orbita_ring3_saved_r12:
    .zero 8
    .align 8
orbita_ring3_saved_r13:
    .zero 8
    .align 8
orbita_ring3_saved_r14:
    .zero 8
    .align 8
orbita_ring3_saved_r15:
    .zero 8
    .align 16
orbita_xmm_save:
    .zero 160
    .section .text

    # Kill path for user-mode CPU faults (roadmap A.7): the fault handler
    # detects CS.RPL=3 with a ring-3 execution active and tail-calls here
    # instead of halting. Same context restore as the EXIT unwind, with
    # the fault sentinel as the returned exit code. The full Win64
    # callee-saved set is restored explicitly: the unwind skips the fault
    # handler's own epilogue, so its working registers must not leak.
    .global orbita_x86_64_ring3_kill_restore
orbita_x86_64_ring3_kill_restore:
    mov qword ptr [rip + orbita_ring3_finished], 0
    mov rdi, [rip + orbita_ring3_saved_rdi]
    mov rsi, [rip + orbita_ring3_saved_rsi]
    mov rbx, [rip + orbita_ring3_saved_rbx]
    mov rbp, [rip + orbita_ring3_saved_rbp]
    mov r12, [rip + orbita_ring3_saved_r12]
    mov r13, [rip + orbita_ring3_saved_r13]
    mov r14, [rip + orbita_ring3_saved_r14]
    mov r15, [rip + orbita_ring3_saved_r15]
    movaps xmm6, [rip + orbita_xmm_save + 0]
    movaps xmm7, [rip + orbita_xmm_save + 16]
    movaps xmm8, [rip + orbita_xmm_save + 32]
    movaps xmm9, [rip + orbita_xmm_save + 48]
    movaps xmm10, [rip + orbita_xmm_save + 64]
    movaps xmm11, [rip + orbita_xmm_save + 80]
    movaps xmm12, [rip + orbita_xmm_save + 96]
    movaps xmm13, [rip + orbita_xmm_save + 112]
    movaps xmm14, [rip + orbita_xmm_save + 128]
    movaps xmm15, [rip + orbita_xmm_save + 144]
    mov rsp, [rip + orbita_ring3_saved_krsp]
    mov rax, 0xCAFEF139
    ret

    .global orbita_x86_64_syscall_entry
orbita_x86_64_syscall_entry:
    # rax = nr, rdi/rsi/rdx = args (SysV), rcx = user rip, r11 = user rflags.
    mov [rip + orbita_syscall_user_rsp], rsp
    lea rsp, [rip + orbita_syscall_kernel_stack_end]
    push rcx
    push r11
    mov rcx, rax        # nr    -> Win64 arg1
    mov r9,  rdx        # arg3  -> Win64 arg4 (set first: rdx is next)
    mov rdx, rdi        # arg1  -> Win64 arg2
    mov r8,  rsi        # arg2  -> Win64 arg3
    sub rsp, 32         # shadow space (keeps 16-byte alignment)
    call orbita_x86_64_syscall_dispatch
    add rsp, 32
    cmp byte ptr [rip + orbita_syscall_ring0], 0
    jne orbita_syscall_ring0_return
    cmp qword ptr [rip + orbita_ring3_finished], 0
    jne orbita_syscall_done_path
    pop r11
    pop rcx
    mov rsp, [rip + orbita_syscall_user_rsp]
    sysretq
orbita_syscall_ring0_return:
    # Ring-0 caller (pre-switch autoruns): SYSRET always lands in ring 3,
    # so resume via flags restore + jump instead. No privilege machinery
    # is involved (same CPL), the saved RSP carries the caller's stack.
    pop r11             # rflags
    pop rcx             # rip
    mov rsp, [rip + orbita_syscall_user_rsp]
    push r11
    popfq
    jmp rcx
orbita_syscall_done_path:
    # Ring-3 execution finished (EXIT syscall / self-test DONE): resume
    # the kernel context that entered ring 3. RDI/RSI and XMM6-15 are
    # Win64-callee-saved but SysV-volatile (the application clobbers
    # them); the GPR callee-saved set is restored explicitly too —
    # symmetric with the kill path and robust against any handler
    # codegen (rax = exit code).
    mov qword ptr [rip + orbita_ring3_finished], 0
    mov rdi, [rip + orbita_ring3_saved_rdi]
    mov rsi, [rip + orbita_ring3_saved_rsi]
    mov rbx, [rip + orbita_ring3_saved_rbx]
    mov rbp, [rip + orbita_ring3_saved_rbp]
    mov r12, [rip + orbita_ring3_saved_r12]
    mov r13, [rip + orbita_ring3_saved_r13]
    mov r14, [rip + orbita_ring3_saved_r14]
    mov r15, [rip + orbita_ring3_saved_r15]
    movaps xmm6, [rip + orbita_xmm_save + 0]
    movaps xmm7, [rip + orbita_xmm_save + 16]
    movaps xmm8, [rip + orbita_xmm_save + 32]
    movaps xmm9, [rip + orbita_xmm_save + 48]
    movaps xmm10, [rip + orbita_xmm_save + 64]
    movaps xmm11, [rip + orbita_xmm_save + 80]
    movaps xmm12, [rip + orbita_xmm_save + 96]
    movaps xmm13, [rip + orbita_xmm_save + 112]
    movaps xmm14, [rip + orbita_xmm_save + 128]
    movaps xmm15, [rip + orbita_xmm_save + 144]
    mov rsp, [rip + orbita_ring3_saved_krsp]
    ret

    .global orbita_x86_64_enter_ring3
orbita_x86_64_enter_ring3:
    # rcx = user rip, rdx = user rsp (Win64 args). Returns to its caller
    # only when a kernel-side finish_ring3() ends ring-3 execution.
    mov [rip + orbita_ring3_saved_krsp], rsp
    mov [rip + orbita_ring3_saved_rdi], rdi
    mov [rip + orbita_ring3_saved_rsi], rsi
    mov [rip + orbita_ring3_saved_rbx], rbx
    mov [rip + orbita_ring3_saved_rbp], rbp
    mov [rip + orbita_ring3_saved_r12], r12
    mov [rip + orbita_ring3_saved_r13], r13
    mov [rip + orbita_ring3_saved_r14], r14
    mov [rip + orbita_ring3_saved_r15], r15
    movaps [rip + orbita_xmm_save + 0], xmm6
    movaps [rip + orbita_xmm_save + 16], xmm7
    movaps [rip + orbita_xmm_save + 32], xmm8
    movaps [rip + orbita_xmm_save + 48], xmm9
    movaps [rip + orbita_xmm_save + 64], xmm10
    movaps [rip + orbita_xmm_save + 80], xmm11
    movaps [rip + orbita_xmm_save + 96], xmm12
    movaps [rip + orbita_xmm_save + 112], xmm13
    movaps [rip + orbita_xmm_save + 128], xmm14
    movaps [rip + orbita_xmm_save + 144], xmm15
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
    fn orbita_x86_64_ring3_kill_restore() -> !;
    /// Finish flag shared with the syscall asm epilogue — MUST be the
    /// same storage the asm reads (a Rust-side twin stays invisible to
    /// the asm and silently breaks the unwind; this bit us in portion 6).
    static mut orbita_ring3_finished: u64;
    /// Ring-0 syscall-return selector shared with the asm epilogue.
    static mut orbita_syscall_ring0: u8;
}

/// Sentinel returned by [`enter_ring3`] when the process was killed by a
/// user-mode CPU fault (see `kill_ring3_from_fault`). Low 32 bits map to
/// the conventional "killed by signal 11" exit code 139.
pub const FAULT_KILL_SENTINEL: u64 = 0xCAFE_F139;

/// Whether a ring-3 execution is currently active (Rust-side only — set
/// and cleared by the [`enter_ring3`] wrapper around the asm call).
static RING3_ACTIVE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Whether ring-3 code is executing right now (fault-handler input).
pub fn ring3_active() -> bool {
    RING3_ACTIVE.load(Ordering::SeqCst)
}

/// Kills the faulting ring-3 process: unwinds to the kernel context that
/// entered ring 3 with [`FAULT_KILL_SENTINEL`] as the exit code. Never
/// returns — called by the CPU-fault handler on user-mode faults.
///
/// # Safety
/// A ring-3 execution must be active (saved context exists).
pub unsafe fn kill_ring3_from_fault() -> ! {
    // SAFETY: caller guarantees an active ring-3 context to unwind to.
    unsafe { orbita_x86_64_ring3_kill_restore() }
}

/// Number of syscalls dispatched (telemetry).
static SYSCALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Kernel-registered syscall dispatcher (ABI v2 ops); the built-in
/// handler only covers the ring-3 self-test pair.
pub type Dispatch = fn(u64, u64, u64, u64) -> u64;
static DISPATCHER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Installs the kernel's syscall dispatcher (boot init). Passing `None`
/// restores the built-in self-test handler.
pub fn set_dispatcher(handler: Option<Dispatch>) {
    DISPATCHER.store(
        handler.map_or(core::ptr::null_mut(), |f| f as *mut ()),
        Ordering::SeqCst,
    );
}

/// Marks ring-3 execution as finished; the current syscall unwinds to the
/// kernel context that entered ring 3, with `exit_code` in `rax`.
pub fn finish_ring3(exit_code: u64) {
    // SAFETY: single-CPU boot; written here, read by the asm epilogue.
    unsafe { orbita_ring3_finished = exit_code | 1 };
}

/// Selects the ring-0 return path for syscalls: `SYSRET` always returns
/// to ring 3, so callers executing at CPL0 (pre-switch autoruns) must be
/// resumed through an IRETQ-to-CPL0 epilogue instead. The kernel brackets
/// every ring-0 exec with this flag.
pub fn set_ring0_syscalls(on: bool) {
    // SAFETY: single-CPU boot; written here, read by the asm epilogue.
    unsafe { orbita_syscall_ring0 = on as u8 };
}

/// v1 syscall dispatcher: delegates to the kernel dispatcher when
/// installed (ABI v2), otherwise serves the ring-3 self-test pair.
#[unsafe(no_mangle)]
unsafe extern "C" fn orbita_x86_64_syscall_dispatch(nr: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    SYSCALL_COUNT.fetch_add(1, Ordering::Relaxed);
    let raw = DISPATCHER.load(Ordering::SeqCst);
    if !raw.is_null() {
        let handler: Dispatch = unsafe { core::mem::transmute(raw) };
        return handler(nr, a1, a2, a3);
    }
    match nr {
        SYSCALL_ECHO => {
            let mut serial = crate::serial::SerialPort::com1();
            serial.write_line("ring3: syscall echo received (kernel side)");
            a1.wrapping_add(1)
        }
        SYSCALL_DONE => {
            let mut serial = crate::serial::SerialPort::com1();
            serial.write_line("ring3: done syscall — resuming kernel context");
            finish_ring3(0);
            0
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

/// Enters ring 3 at `user_rip` with `user_rsp`. Returns only when a
/// kernel-side [`finish_ring3`] ends ring-3 execution (exit code) or the
/// process is killed by a user-mode CPU fault ([`FAULT_KILL_SENTINEL`]).
///
/// # Safety
/// Caller guarantees USER-mapped, executable code at `user_rip` and a
/// mapped, 16-byte aligned `user_rsp`, with the GDT/TSS installed.
pub unsafe fn enter_ring3(user_rip: u64, user_rsp: u64) -> u64 {
    RING3_ACTIVE.store(true, Ordering::SeqCst);
    // SAFETY: see contract above; the finish/kill path resumes this frame.
    let code = unsafe { orbita_x86_64_enter_ring3(user_rip, user_rsp) };
    RING3_ACTIVE.store(false, Ordering::SeqCst);
    code
}

/// Compatibility alias for the stage-A self-test entry point.
pub unsafe fn ring3_roundtrip(user_rip: u64, user_rsp: u64) -> u64 {
    unsafe { enter_ring3(user_rip, user_rsp) }
}

/// Syscalls dispatched since boot (boot log telemetry).
pub fn syscall_count() -> u64 {
    SYSCALL_COUNT.load(Ordering::Relaxed)
}
