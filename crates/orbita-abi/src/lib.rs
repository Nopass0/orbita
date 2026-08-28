#![no_std]

//! The Orbita application ABI.
//!
//! This crate is the *single source of truth* shared by two sides:
//!
//! - the **kernel** implements the function-pointer table [`OrbAbi`] over
//!   its live services (filesystem, memory, time, network inventory) and
//!   passes a `*const OrbAbi` to every native application at entry;
//! - **applications** (via `orbita-sdk`) call through that table — the
//!   compiled app contains no kernel symbols, only C-ABI indirect calls.
//!
//! Everything crossing the boundary is `#[repr(C)]` with C ABI function
//! types, so kernel and app can be compiled in independent crates (and
//! later: different privileges). Bumping [`ABI_VERSION`] rejects binaries
//! built against an incompatible kernel at load time.
//!
//! # v1 scope
//!
//! stdout / exit-code · fs read/write/list/delete · memory alloc/free ·
//! monotonic time · OS info · network inventory. Graphics presentation,
//! sockets and process spawning arrive in later ABI revisions (see
//! `docs/roadmap.md`).

/// Version of the ABI table layout. Applications are rejected when their
/// compiled-in version differs from the kernel's.
pub const ABI_VERSION: u32 = 1;

/// Borrowed read-only byte string passed across the ABI.
#[repr(C)]
pub struct AbiStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl AbiStr {
    /// View the borrowed bytes as a `str` slice (kernel-validated input).
    ///
    /// # Safety
    /// `ptr` must point to `len` initialized, readable bytes for the
    /// duration of the call.
    pub unsafe fn as_str(&self) -> &str {
        unsafe {
            let bytes = core::slice::from_raw_parts(self.ptr, self.len);
            core::str::from_utf8_unchecked(bytes)
        }
    }
}

/// Status codes shared by all ABI calls (0 = success).
#[repr(i32)]
pub enum AbiStatus {
    Ok = 0,
    /// A buffer passed by the caller was too small.
    BufferTooSmall = 1,
    /// The requested path does not exist.
    NotFound = 2,
    /// An I/O or internal kernel error.
    IoError = 3,
    /// The call is not implemented by this kernel build.
    Unsupported = 4,
    /// Malformed argument (invalid path, bad handle...).
    InvalidArgument = 5,
}

/// The Orbita application ABI table.
///
/// The kernel fills one static instance and passes `*const OrbAbi` to the
/// application entry point `orb_main(abi: *const OrbAbi) -> i32`.
///
/// Every entry uses the **`sysv64`** calling convention: applications are
/// built for `x86_64-unknown-none` (SysV), while the kernel target
/// (`x86_64-unknown-uefi`) defaults to Win64 — pinning the convention in
/// the table type keeps both sides in agreement without wrapper thunks.
#[repr(C)]
pub struct OrbAbi {
    /// [`ABI_VERSION`] of the kernel implementation.
    pub abi_version: u32,

    /// Append a line to the application's stdout (terminal + serial log).
    pub stdout_write: extern "sysv64" fn(line: AbiStr),

    /// Read a whole file into `buf` (cap bytes); writes the actual length
    /// through `out_len`. Returns an [`AbiStatus`] value.
    pub fs_read: extern "sysv64" fn(path: AbiStr, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32,
    /// Write (create/replace) a file with the given bytes.
    pub fs_write: extern "sysv64" fn(path: AbiStr, data: AbiStr) -> i32,
    /// List a directory as newline-separated names into `buf`.
    pub fs_list: extern "sysv64" fn(path: AbiStr, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32,
    /// Delete a file or (empty) directory.
    pub fs_delete: extern "sysv64" fn(path: AbiStr) -> i32,

    /// Allocate `size` bytes with `align` alignment for the application.
    pub mem_alloc: extern "sysv64" fn(size: usize, align: usize) -> *mut u8,
    /// Free a block previously returned by `mem_alloc`.
    pub mem_free: extern "sysv64" fn(ptr: *mut u8, size: usize, align: usize),

    /// Monotonic milliseconds since boot.
    pub time_ms: extern "sysv64" fn() -> u64,

    /// Human-readable OS/kernel summary into `buf` (version, renderer,
    /// memory, CPUs). Writes actual length through `out_len`.
    pub os_info: extern "sysv64" fn(buf: *mut u8, cap: usize, out_len: *mut usize) -> i32,

    /// Network interface summary (one line per interface, `\n`-separated).
    pub net_interfaces: extern "sysv64" fn(buf: *mut u8, cap: usize, out_len: *mut usize) -> i32,

    /// Report the application's exit code before returning from `orb_main`.
    /// The kernel reads this (more robust than the raw rax return across
    /// the two calling conventions).
    pub report_exit: extern "sysv64" fn(code: i32),
}

// The table is plain shared data; both sides only read function pointers.
unsafe impl Send for OrbAbi {}
unsafe impl Sync for OrbAbi {}

// ---------------------------------------------------------------------------
// ABI v2: syscall transport (stage A, roadmap A.5).
//
// Ring-3 applications reach the kernel through the `syscall` instruction
// instead of the function table (which is inherently a ring-0 construct:
// it hands kernel code pointers to untrusted code). One register argument
// travels per syscall: `rdi` = pointer to a [`SyscallReq`] block owned by
// the caller; the kernel fills `ret`. The block must live in the
// application's user-mapped region — the kernel validates the pointer
// range before dereferencing.
// ---------------------------------------------------------------------------

/// Syscall operation numbers (ABI v2 transport).
pub mod nr {
    /// Emit one stdout line: `a1` = bytes ptr, `a2` = len.
    pub const STDOUT_WRITE: u64 = 1;
    /// Read a file: `a1`/`a2` = path ptr/len, `a3` = buf, `a4` = cap.
    /// `ret` = byte length, or negative status.
    pub const FS_READ: u64 = 2;
    /// Write a file: `a1`/`a2` = path ptr/len, `a3` = data ptr, `a4` = len.
    pub const FS_WRITE: u64 = 3;
    /// List a directory: like [`FS_READ`], entries `\n`-separated.
    pub const FS_LIST: u64 = 4;
    /// Delete a path: `a1`/`a2` = path ptr/len.
    pub const FS_DELETE: u64 = 5;
    /// Monotonic milliseconds since boot. `ret` = value.
    pub const TIME_MS: u64 = 8;
    /// OS summary: `a1` = buf, `a2` = cap. `ret` = length or negative status.
    pub const OS_INFO: u64 = 9;
    /// Network interface summary: `a1` = buf, `a2` = cap.
    pub const NET_INTERFACES: u64 = 10;
    /// Terminate the application: `a1` = exit code. Does not return.
    pub const EXIT: u64 = 11;
}

/// Register-level syscall request block (one per call, on the caller stack).
#[repr(C)]
pub struct SyscallReq {
    /// Operation: one of [`nr`].
    pub nr: u64,
    /// Operation arguments (pointers are user-region addresses).
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    /// Return value filled by the kernel.
    pub ret: u64,
}

impl SyscallReq {
    /// Fresh request for `op`.
    pub const fn new(op: u64) -> Self {
        Self { nr: op, a1: 0, a2: 0, a3: 0, a4: 0, ret: 0 }
    }
}

/// Ceiling of the linked application image inside the user load region
/// (`0x1000_0000 .. APP_IMAGE_LIMIT`): the SDK's bump heap starts exactly
/// here, so the kernel loader must reject any ELF segment crossing it
/// (an image that big would silently corrupt the application's own heap
/// statics).
///
/// Shared contract between `orbita-build` (linker base), the kernel ELF
/// loader and `orbita-sdk` (`RegionHeap::BASE`).
pub const APP_IMAGE_LIMIT: u64 = 0x1008_0000;
