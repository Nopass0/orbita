//! Syscall transport for the application side (ABI v2, stage A).
//!
//! v2: every [`crate::sys`] call goes through the `syscall` instruction
//! (`rax` = operation, `rdi` = pointer to a [`SyscallReq`] block owned by
//! the caller) instead of the legacy function table. The same transport
//! works for ring-0 execution (the kernel runs `syscall` at CPL0 too)
//! and for ring-3 user processes — the kernel side dispatches
//! identically. The kernel validates the request pointer against the
//! user load region before dereferencing.
//!
//! The application heap became a bump allocator inside the user load
//! region: kernel-heap memory would not be user-accessible from ring 3.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

pub use orbita_abi::{AbiStatus, AbiStr, OrbAbi, SyscallReq, ABI_VERSION};
pub use orbita_abi::nr;

/// Raw syscall: `rax` = `req.nr`, `rdi` = `&req`. Returns `rax` and
/// mirrors it into `req.ret`.
#[inline]
pub fn raw(req: &mut SyscallReq) -> u64 {
    let ret: u64;
    // SAFETY: the syscall instruction clobbers rcx/r11 by architecture
    // definition; the kernel side preserves everything else.
    unsafe {
        asm!(
            "syscall",
            in("rax") req.nr,
            in("rdi") req as *mut SyscallReq as u64,
            lateout("rax") ret,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    req.ret = ret;
    ret
}

/// One-shot syscall helper (the common case: fill a block on the stack).
pub fn call(op: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let mut req = SyscallReq::new(op);
    req.a1 = a1;
    req.a2 = a2;
    req.a3 = a3;
    req.a4 = a4;
    raw(&mut req)
}

/// Compatibility entry kept for the ring-0 exec path: the table pointer
/// is ignored (v2 transports everything via syscalls).
///
/// # Safety
/// Accepted for signature compatibility only.
pub unsafe fn install(_abi: *const OrbAbi) {}

/// Emit one stdout line through the syscall ABI.
pub fn stdout_line(text: &str) {
    let _ = call(nr::STDOUT_WRITE, text.as_ptr() as u64, text.len() as u64, 0, 0);
}

/// Record an exit code that overrides the value returned by `main`.
pub fn set_exit_code(code: i32) {
    EXIT_CODE.store(code, Ordering::Relaxed);
}

/// Take (once) the exit code recorded by [`set_exit_code`].
pub fn take_exit_code() -> Option<i32> {
    let code = EXIT_CODE.swap(i32::MIN, Ordering::Relaxed);
    (code != i32::MIN).then_some(code)
}

static EXIT_CODE: AtomicI32 = AtomicI32::new(i32::MIN);

/// The application heap: bump allocator over the user load region.
///
/// v1 constraint: memory is never returned (short-lived apps; the whole
/// region is reloaded on every run, so the leak does not accumulate).
/// The region sits above the app image and below the user stack.
pub struct RegionHeap;

impl RegionHeap {
    /// Bump region base (must stay clear of the linked app image).
    const BASE: usize = orbita_abi::APP_IMAGE_LIMIT as usize;
    /// Bump region size (256 KiB).
    const SIZE: usize = 0x4_0000;
}

static HEAP_CURSOR: AtomicUsize = AtomicUsize::new(RegionHeap::BASE);

unsafe impl GlobalAlloc for RegionHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(1);
        let align = layout.align().max(1).next_power_of_two();
        let mut cursor = HEAP_CURSOR.load(Ordering::Relaxed);
        loop {
            let aligned = (cursor + align - 1) & !(align - 1);
            let next = aligned + size;
            if next > RegionHeap::BASE + RegionHeap::SIZE {
                return core::ptr::null_mut();
            }
            if HEAP_CURSOR
                .compare_exchange_weak(cursor, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return aligned as *mut u8;
            }
            cursor = HEAP_CURSOR.load(Ordering::Relaxed);
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator: freed only implicitly on the next app run.
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe {
            let ptr = GlobalAlloc::alloc(self, layout);
            if !ptr.is_null() {
                core::ptr::write_bytes(ptr, 0, layout.size());
            }
            ptr
        }
    }
}

/// The application heap, installed for every binary linking the SDK.
///
/// Test binaries keep the host `std` allocator: the harness allocates
/// before `orb_main` could ever run.
#[cfg(not(test))]
#[global_allocator]
static HEAP: RegionHeap = RegionHeap;
