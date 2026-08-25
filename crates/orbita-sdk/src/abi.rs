//! ABI table plumbing for the application side.
//!
//! [`install`] is called by the generated `orb_main` before anything
//! else; every [`crate::sys`] call afterwards reads the table through
//! [`table`].

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

pub use orbita_abi::{AbiStatus, AbiStr, OrbAbi, ABI_VERSION};

static TABLE: AtomicPtr<OrbAbi> = AtomicPtr::new(core::ptr::null_mut());
static EXIT_CODE: AtomicI32 = AtomicI32::new(i32::MIN);

/// Store the table pointer handed to `orb_main`.
///
/// # Safety
/// `abi` must point to a table that stays alive for the whole run.
pub unsafe fn install(abi: *const OrbAbi) {
    TABLE.store(abi as *mut OrbAbi, Ordering::Relaxed);
}

/// The installed table (panics if called before [`install`] — i.e. only
/// reachable from broken hosts).
pub fn table() -> &'static OrbAbi {
    let ptr = TABLE.load(Ordering::Relaxed);
    assert!(!ptr.is_null(), "orbita abi table not installed");
    // SAFETY: installed by the generated entry point and never freed.
    unsafe { &*ptr }
}

/// Emit one stdout line through the ABI.
pub fn stdout_line(text: &str) {
    let line = AbiStr {
        ptr: text.as_ptr(),
        len: text.len(),
    };
    (table().stdout_write)(line);
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

/// The application heap: backed by the kernel allocator through the ABI
/// `mem_alloc`/`mem_free` entries.
pub struct AbiHeap;

unsafe impl GlobalAlloc for AbiHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        (table().mem_alloc)(layout.size(), layout.align())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        (table().mem_free)(ptr, layout.size(), layout.align())
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

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            let new = GlobalAlloc::alloc(self, new_layout);
            if !new.is_null() {
                let copy = layout.size().min(new_size);
                core::ptr::copy_nonoverlapping(ptr, new, copy);
                GlobalAlloc::dealloc(self, ptr, layout);
            }
            new
        }
    }
}

/// The application heap, installed for every binary linking the SDK.
#[global_allocator]
static HEAP: AbiHeap = AbiHeap;
