//! Kernel-side implementation of the Orbita application ABI
//! ([`orbita_abi::OrbAbi`]) and the native ELF loader/exec path.
//!
//! An installed application is an ORBEXEC container whose payload is a
//! statically linked ELF64 x86-64 executable built against `orbita-sdk`.
//! `exec_native`:
//!
//! 1. installs the ABI globals (filesystem pointer, stdout buffer, info
//!    strings) for the duration of the run,
//! 2. parses and loads the ELF program headers (identity-mapped ring-0
//!    execution — hardware paging/user mode is a roadmap milestone),
//! 3. calls `orb_main(&ABI_TABLE)` and returns the exit code plus the
//!    captured stdout lines.

use core::alloc::Layout;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, Ordering};
use core::time::Duration;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use orbita_abi::{AbiStatus, AbiStr, OrbAbi, ABI_VERSION};
use orbita_fs::MemoryVolume;
use orbita_std::format;

/// Disable the firmware's NX policy: UEFI maps loader-data pages (where
/// native applications are loaded) as non-executable. After
/// `ExitBootServices` the kernel owns the machine; clearing `EFER.NXE`
/// makes the whole identity map executable, which v1 ring-0 applications
/// rely on. User-mode processes (roadmap) restore per-range protection.
pub(crate) fn disable_nx() {
    const EFER_MSR: u32 = 0xC000_0080;
    const EFER_NXE: u64 = 1 << 11;
    // SAFETY: single WRMSR on the current CPU with interrupts off.
    unsafe {
        let efer = orbita_arch_x86_64::msr::read(EFER_MSR);
        if efer & EFER_NXE != 0 {
            orbita_arch_x86_64::msr::write(EFER_MSR, efer & !EFER_NXE);
        }
    }
}

/// Fixed load base for native applications — must match the orbita-build
/// linker script (`apps` are statically linked here, identity-mapped).
pub(crate) const APP_LOAD_BASE: u64 = 0x1000_0000;

/// Milliseconds per timer tick; initialized from the boot timer plan.
static TIME_MS_PER_TICK: AtomicU64 = AtomicU64::new(0);

/// Set the tick→milliseconds scale used by `time_ms` (boot init).
pub(crate) fn set_time_scale(ms_per_tick: u64) {
    TIME_MS_PER_TICK.store(ms_per_tick, Ordering::Relaxed);
}

/// OS summary line reported through `os_info` (boot init).
pub(crate) fn set_os_summary(summary: String) {
    *OS_INFO.lock() = summary;
}

// --- ABI globals (valid for the duration of one application run) ----------

static ABI_FS: AtomicPtr<MemoryVolume> = AtomicPtr::new(ptr::null_mut());
static STDOUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
static OS_INFO: Mutex<String> = Mutex::new(String::new());
static NET_INFO: Mutex<String> = Mutex::new(String::new());

fn abi_fs() -> Option<&'static mut MemoryVolume> {
    let raw = ABI_FS.load(Ordering::Relaxed) as *mut MemoryVolume;
    if raw.is_null() {
        None
    } else {
        // SAFETY: the pointer is installed by `exec_native` and removed
        // before it returns; the kernel is single-threaded during runs.
        Some(unsafe { &mut *raw })
    }
}

fn status_of(result: Result<(), orbita_fs::FsError>) -> i32 {
    match result {
        Ok(()) => AbiStatus::Ok as i32,
        Err(_) => AbiStatus::IoError as i32,
    }
}

extern "sysv64" fn abi_stdout_write(line: AbiStr) {
    let text = unsafe { line.as_str() };
    // Mirror to the serial console immediately (plain write — no format
    // machinery on the application stack); the terminal sees the drained
    // buffer after the application exits.
    orbita_platform::log_line_fmt(format_args!("[app] {text}"));
    STDOUT.lock().push(String::from(text));
}

extern "sysv64" fn abi_fs_read(path: AbiStr, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    let Some(fs) = abi_fs() else {
        return AbiStatus::Unsupported as i32;
    };
    let path = unsafe { path.as_str() };
    match fs.read_file_path(path) {
        Ok(bytes) => {
            unsafe { *out_len = bytes.len() };
            if bytes.len() > cap {
                return AbiStatus::BufferTooSmall as i32;
            }
            // SAFETY: caller-provided buffer of `cap` bytes.
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len()) };
            AbiStatus::Ok as i32
        }
        Err(_) => {
            unsafe { *out_len = 0 };
            AbiStatus::NotFound as i32
        }
    }
}

extern "sysv64" fn abi_fs_write(path: AbiStr, data: AbiStr) -> i32 {
    let Some(fs) = abi_fs() else {
        return AbiStatus::Unsupported as i32;
    };
    let path = unsafe { path.as_str() };
    let data = unsafe { data.as_str() };
    status_of(fs.create_file_path(path, data.as_bytes()))
}

extern "sysv64" fn abi_fs_list(path: AbiStr, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    let Some(fs) = abi_fs() else {
        return AbiStatus::Unsupported as i32;
    };
    let path = unsafe { path.as_str() };
    match fs.list_path(path) {
        Ok(listing) => {
            let mut text = String::new();
            for entry in listing.entries {
                text.push_str(&entry.name);
                if entry.metadata.is_directory() {
                    text.push('/');
                }
                text.push('\n');
            }
            unsafe { *out_len = text.len() };
            if text.len() > cap {
                return AbiStatus::BufferTooSmall as i32;
            }
            // SAFETY: caller-provided buffer of `cap` bytes.
            unsafe { ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len()) };
            AbiStatus::Ok as i32
        }
        Err(_) => {
            unsafe { *out_len = 0 };
            AbiStatus::NotFound as i32
        }
    }
}

extern "sysv64" fn abi_fs_delete(path: AbiStr) -> i32 {
    let Some(fs) = abi_fs() else {
        return AbiStatus::Unsupported as i32;
    };
    let path = unsafe { path.as_str() };
    status_of(fs.remove_path(path))
}

extern "sysv64" fn abi_mem_alloc(size: usize, align: usize) -> *mut u8 {
    orbita_platform::log_line_fmt(format_args!("abi mem_alloc size={size} align={align}"));
    let Ok(layout) = Layout::from_size_align(size.max(1), align.max(1)) else {
        return ptr::null_mut();
    };
    // SAFETY: the kernel global allocator backs this.
    unsafe { alloc::alloc::alloc(layout) }
}

extern "sysv64" fn abi_mem_free(block: *mut u8, size: usize, align: usize) {
    if block.is_null() {
        return;
    }
    let Ok(layout) = Layout::from_size_align(size.max(1), align.max(1)) else {
        return;
    };
    // SAFETY: paired with `abi_mem_alloc` on the same layout.
    unsafe { alloc::alloc::dealloc(block, layout) }
}

extern "sysv64" fn abi_time_ms() -> u64 {
    let ticks = crate::TIMER_IRQ_COUNT.load(Ordering::Relaxed) as u64;
    ticks.saturating_mul(TIME_MS_PER_TICK.load(Ordering::Relaxed))
}

extern "sysv64" fn abi_os_info(buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    let text = OS_INFO.lock().clone();
    unsafe { *out_len = text.len() };
    if text.len() > cap {
        return AbiStatus::BufferTooSmall as i32;
    }
    // SAFETY: caller-provided buffer of `cap` bytes.
    unsafe { ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len()) };
    AbiStatus::Ok as i32
}

extern "sysv64" fn abi_net_interfaces(buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    let text = NET_INFO.lock().clone();
    unsafe { *out_len = text.len() };
    if text.len() > cap {
        return AbiStatus::BufferTooSmall as i32;
    }
    // SAFETY: caller-provided buffer of `cap` bytes.
    unsafe { ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len()) };
    AbiStatus::Ok as i32
}

/// The kernel's ABI table instance handed to every application.
pub(crate) static ABI_TABLE: OrbAbi = OrbAbi {
    abi_version: ABI_VERSION,
    stdout_write: abi_stdout_write,
    fs_read: abi_fs_read,
    fs_write: abi_fs_write,
    fs_list: abi_fs_list,
    fs_delete: abi_fs_delete,
    mem_alloc: abi_mem_alloc,
    mem_free: abi_mem_free,
    time_ms: abi_time_ms,
    os_info: abi_os_info,
    net_interfaces: abi_net_interfaces,
    report_exit: abi_report_exit,
};

// --- ELF64 loader ----------------------------------------------------------

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3E;
const PT_LOAD: u32 = 1;

/// Errors from [`load_elf`].
#[derive(Debug)]
pub(crate) enum LoadError {
    NotElf,
    UnsupportedElf,
    NoEntry,
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

/// Loads a statically linked ELF64 executable at its link-time addresses
/// (identity mapping) and returns the entry point.
///
/// v1 constraint: applications are linked at `0x1000_0000` (orbita-build
/// default) which stays clear of the kernel image and its heap.
pub(crate) fn load_elf(image: &[u8]) -> Result<u64, LoadError> {
    if image.len() < 64 || image[..4] != ELF_MAGIC {
        return Err(LoadError::NotElf);
    }
    if image[4] != 2 || image[5] != 1 {
        return Err(LoadError::UnsupportedElf); // 64-bit little-endian only
    }
    let e_type = u16_at(image, 16);
    let e_machine = u16_at(image, 18);
    if e_type != ET_EXEC || e_machine != EM_X86_64 {
        return Err(LoadError::UnsupportedElf);
    }
    let entry = u64_at(image, 24);
    let phoff = u64_at(image, 32) as usize;
    let phentsize = u16_at(image, 54) as usize;
    let phnum = u16_at(image, 56) as usize;
    if entry == 0 {
        return Err(LoadError::NoEntry);
    }

    for index in 0..phnum {
        let ph = phoff + index * phentsize;
        if ph + phentsize > image.len() {
            return Err(LoadError::UnsupportedElf);
        }
        if u32_at(image, ph) != PT_LOAD {
            continue;
        }
        let p_offset = u64_at(image, ph + 8) as usize;
        let p_paddr = u64_at(image, ph + 24) as usize;
        let p_filesz = u64_at(image, ph + 32) as usize;
        let p_memsz = u64_at(image, ph + 40) as usize;
        if p_offset + p_filesz > image.len() {
            return Err(LoadError::UnsupportedElf);
        }
        // SAFETY: identity-mapped ring-0 execution; destination addresses
        // come from the link script and are owned by the loader.
        unsafe {
            ptr::copy_nonoverlapping(image.as_ptr().add(p_offset), p_paddr as *mut u8, p_filesz);
            // Zero the .bss tail beyond the file image.
            ptr::write_bytes((p_paddr + p_filesz) as *mut u8, 0, p_memsz - p_filesz);
        }
    }
    Ok(entry)
}

/// Execution result of a native application.
pub(crate) struct NativeRun {
    pub code: i32,
    pub stdout: Vec<String>,
}

/// Runs one ORBEXEC payload (ELF64) with the ABI installed.
pub(crate) fn exec_native(
    fs: &mut MemoryVolume,
    net_info: String,
    payload: &[u8],
) -> Result<NativeRun, String> {
    let entry = load_elf(payload).map_err(|err| format!("run: elf loader rejected image ({err:?})"))?;

    ABI_FS.store(fs as *mut MemoryVolume, Ordering::Relaxed);
    *NET_INFO.lock() = net_info;
    STDOUT.lock().clear();

    // Applications run on a dedicated kernel-heap stack: the boot UEFI
    // stack is small and already deep at this point, and overflowing it
    // corrupts kernel fmt structures.
    const APP_STACK_SIZE: usize = 256 * 1024;
    let stack_layout =
        Layout::from_size_align(APP_STACK_SIZE, 16).expect("app stack layout");
    // SAFETY: kernel global allocator backs the stack.
    let stack = unsafe { alloc::alloc::alloc(stack_layout) };
    if stack.is_null() {
        return Err(String::from("run: cannot allocate app stack"));
    }
    let stack_top = unsafe { stack.add(APP_STACK_SIZE) };

    // SAFETY: the entry point is the ELF's `orb_main` with the C ABI
    // signature `fn(*const OrbAbi) -> i32`; segments were loaded above.
    // The call switches RSP to the fresh stack and restores it after.
    REPORTED_EXIT.store(i32::MIN, Ordering::Relaxed);
    let _rax_code: i32 = unsafe { call_with_stack(entry, &ABI_TABLE, stack_top as u64) };
    let code = REPORTED_EXIT.load(Ordering::Relaxed);
    if code == i32::MIN {
        return Err(String::from("run: application did not report an exit code"));
    }

    // SAFETY: paired with the allocation above.
    unsafe { alloc::alloc::dealloc(stack, stack_layout) };

    ABI_FS.store(ptr::null_mut(), Ordering::Relaxed);
    let lines = STDOUT.lock().drain(..).collect();
    Ok(NativeRun { code, stdout: lines })
}

/// Calls `entry(arg)` on a fresh stack, restoring the original RSP after.
///
/// # Safety
/// `entry` must be a valid `extern "C" fn(*const OrbAbi) -> i32`; `stack_top`
/// must point to the top of a writable, 16-byte-aligned stack region that
/// stays alive for the whole call.
#[unsafe(naked)]
unsafe extern "C" fn call_with_stack(entry: u64, arg: *const OrbAbi, stack_top: u64) -> i32 {
    // Win64 ABI (x86_64-unknown-uefi): rcx = entry, rdx = arg, r8 = stack_top.
    core::arch::naked_asm!(
        // Bridge the Win64 (kernel) ↔ SysV (application) calling
        // conventions: rdi/rsi are callee-saved in Win64 but volatile in
        // SysV, so the application may clobber them — save them across
        // the call or the kernel resumes with garbage registers.
        "push rbx",
        "push rdi",
        "push rsi",
        "mov rbx, rsp",
        "mov r11, rcx",       // entry address (Win64 arg 1)
        "mov rsp, r8",        // switch stacks (Win64 arg 3)
        "and rsp, -16",       // ABI stack alignment
        "mov rdi, rdx",       // the ABI table — SysV first parameter
        "call r11",           // orb_main(table)
        "mov rsp, rbx",       // restore the kernel stack
        "pop rsi",
        "pop rdi",
        "pop rbx",
        "ret",
    )
}

static REPORTED_EXIT: AtomicI32 = AtomicI32::new(i32::MIN);

extern "sysv64" fn abi_report_exit(code: i32) {
    REPORTED_EXIT.store(code, Ordering::Relaxed);
}

/// Formats a duration in whole milliseconds (host-side helper reuse).
pub(crate) fn _unused_duration_helper(d: Duration) -> String {
    format!("{}ms", d.as_millis())
}
