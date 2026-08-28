//! Stage-A portion 3: page-table infrastructure over real frames.
//!
//! [`KernelFrameMemory`] implements [`orbita_mm::paging::FrameMemory`] on
//! top of the bootstrap frame allocator, addressing table frames directly
//! (identity mapping is still in effect while the firmware CR3 is live).
//!
//! [`dry_run_identity_map`] builds a complete identity map of low memory
//! in 2 MiB huge pages **without switching CR3** — it proves the frame
//! supply and the mapper end-to-end in the running OS and prints a
//! serial line for the boot log. The actual CR3 switch (kernel-half,
//! user-half, per-process tables) follows in portion 4.

use orbita_mm::paging::{
    FrameMemory, PageTableMapper, Phys, Virt, ENTRIES, PAGE_SIZE_2M, USER, WRITABLE,
};
use orbita_mm::{BootstrapFrameAllocator, MemoryRegion, MemoryRegionKind};

use crate::config;

/// `FrameMemory` over the bootstrap frame allocator.
///
/// # Safety contract
/// While the firmware identity mapping is active (before the first CR3
/// switch), physical frame addresses are directly dereferenceable. After
/// a kernel-half move, this must go through a direct-map window instead.
pub(crate) struct KernelFrameMemory<'a, 'b> {
    allocator: &'a mut BootstrapFrameAllocator<'b>,
}

impl<'a, 'b> KernelFrameMemory<'a, 'b> {
    pub(crate) fn new(allocator: &'a mut BootstrapFrameAllocator<'b>) -> Self {
        Self { allocator }
    }
}

impl FrameMemory for KernelFrameMemory<'_, '_> {
    fn alloc_frame(&mut self) -> Option<u64> {
        let frame = self.allocator.allocate_frame()?.start.0;
        // The FrameMemory contract requires fresh *zeroed* frames: UEFI
        // conventional memory carries firmware leftovers, and a page-table
        // walk through a dirty table treats garbage as present entries —
        // mapping into random physical frames (observed as `huge_pages=0`
        // on warm boots and as device MMIO writes landing in RAM).
        //
        // SAFETY: identity-mapped firmware CR3 — the frame is writable.
        let table = unsafe { &mut *(frame as *mut [u64; ENTRIES]) };
        *table = [0; ENTRIES];
        Some(frame)
    }

    fn frame_mut(&mut self, frame: u64) -> Option<&mut [u64; ENTRIES]> {
        if frame % orbita_mm::PAGE_SIZE as u64 != 0 {
            return None;
        }
        // SAFETY: identity-mapped firmware CR3 — the physical frame is
        // directly writable; alignment and page bounds checked above.
        Some(unsafe { &mut *(frame as *mut [u64; ENTRIES]) })
    }

    fn frame(&self, frame: u64) -> Option<&[u64; ENTRIES]> {
        if frame % orbita_mm::PAGE_SIZE as u64 != 0 {
            return None;
        }
        // SAFETY: shared read of an identity-mapped frame.
        Some(unsafe { &*(frame as *const [u64; ENTRIES]) })
    }
}

/// Highest usable address below `limit` across all usable regions —
/// the identity-map target for the dry run.
fn usable_top(regions: &[MemoryRegion], limit: u64) -> u64 {
    regions
        .iter()
        .filter(|r| r.kind == MemoryRegionKind::Usable)
        .map(|r| r.end().0.min(limit))
        .max()
        .unwrap_or(0)
}

/// Outcome of the dry run (serial log + stage-A telemetry).
pub(crate) struct DryRunReport {
    pub pml4_phys: u64,
    pub mapped_huge: usize,
    pub mapped_bytes: u64,
}

/// Build an identity map of low memory in 2 MiB huge pages without
/// touching CR3. Returns `None` when table frames ran out.
pub(crate) fn dry_run_identity_map(
    allocator: &mut BootstrapFrameAllocator<'_>,
    regions: &[MemoryRegion],
    limit: u64,
) -> Option<DryRunReport> {
    let pml4 = allocator.allocate_frame()?.start.0;
    let mut memory = KernelFrameMemory::new(allocator);
    let mut mapper = PageTableMapper::new(pml4);
    // SAFETY: fresh zeroed frame from the allocator.
    if let Some(table) = memory.frame_mut(pml4) {
        *table = [0; ENTRIES];
    }
    let top = usable_top(regions, limit);
    let mapped = mapper.map_identity_2mib(&mut memory, 0, top, WRITABLE).ok()?;
    // Spot-check an entry deep inside the map: translate must succeed and
    // point back at the same physical address (identity).
    let probe = Virt((top / 2) & !(PAGE_SIZE_2M - 1));
    match mapper.translate(&memory, probe) {
        Some((Phys(phys), _)) if phys == probe.0 => {}
        _ => return None,
    }
    Some(DryRunReport {
        pml4_phys: pml4,
        mapped_huge: mapped,
        mapped_bytes: mapped as u64 * PAGE_SIZE_2M,
    })
}

/// Run the dry run if `paging_dry_run=on` is set in the live config.
pub(crate) fn maybe_run_dry_run(
    allocator: &mut BootstrapFrameAllocator<'_>,
    regions: &[MemoryRegion],
    conf_text: &str,
    limit: u64,
) {
    if !config::wants_paging_dry_run(conf_text) {
        return;
    }
    match dry_run_identity_map(allocator, regions, limit) {
        Some(report) => orbita_platform::log_line_fmt(format_args!(
            "Orbita OS: paging dry-run ok: pml4=0x{:x} huge_pages={} span={}",
            report.pml4_phys,
            report.mapped_huge,
            orbita_std::diagnostics::format_bytes(report.mapped_bytes)
        )),
        None => orbita_platform::log_line(
            "Orbita OS: paging dry-run FAILED (frame exhaustion or broken map)",
        ),
    }
}

/// Outcome of the address-space build (serial log + stage-A telemetry).
pub(crate) struct AddressSpaceReport {
    pub pml4_phys: u64,
    pub mapped_huge: usize,
    pub mapped_bytes: u64,
}

/// Build the full kernel address space:
///
/// 1. **0..4 GiB mapped wholesale** in 2 MiB huge pages. The 32-bit PCI
///    hole is only partially described by the firmware memory map — PCI
///    ECAM, device BARs (AHCI ABAR, e1000 MMIO, VGA LFB) and the
///    LAPIC/IOAPIC pages live in gaps between descriptors, and missing
///    one of them faults the first driver access after the switch.
///    Mapping the whole low space costs 2048 huge pages (6 table frames)
///    and makes the kernel map at least as complete as the firmware's.
/// 2. **Every descriptor above 4 GiB** (high RAM, 64-bit MMIO windows)
///    rounded outward to 2 MiB bounds; already-mapped ranges are skipped.
/// 3. **Explicit extras** (GOP framebuffer, LAPIC/IOAPIC) — normally
///    covered by (1)/(2), kept as a belt-and-braces guarantee.
///
/// Returns the new PML4 physical frame, ready for [`switch_cr3`].
pub(crate) fn build_full_address_space(
    allocator: &mut BootstrapFrameAllocator<'_>,
    regions: &[MemoryRegion],
    framebuffer: Option<(u64, u64)>,
    extra_mmio: &[(u64, u64)],
) -> Option<AddressSpaceReport> {
    const LOW_SPACE_TOP: u64 = 0x1_0000_0000; // 4 GiB

    let mut memory = KernelFrameMemory::new(allocator);
    // alloc_frame returns zeroed frames (see impl), so the root needs no
    // extra initialization.
    let pml4 = memory.alloc_frame()?;
    let mut mapper = PageTableMapper::new(pml4);
    let mut mapped = 0usize;

    // (1) Low 4 GiB wholesale: RAM + every 32-bit MMIO hole.
    mapped += mapper
        .map_identity_2mib(&mut memory, 0, LOW_SPACE_TOP, WRITABLE)
        .ok()?;
    if mapped == 0 {
        return None;
    }

    // (2) Descriptors above 4 GiB: high RAM and 64-bit MMIO windows.
    for region in regions {
        if region.end().0 <= LOW_SPACE_TOP {
            continue;
        }
        let start = (region.start.0.max(LOW_SPACE_TOP) / PAGE_SIZE_2M) * PAGE_SIZE_2M;
        let end = region.end().0;
        if end > start {
            mapped += mapper
                .map_identity_2mib(&mut memory, start, end, WRITABLE)
                .unwrap_or(0);
        }
    }

    // (3) Explicit extras (skip-existing makes them no-ops when covered).
    if let Some((base, size)) = framebuffer {
        let start = (base / PAGE_SIZE_2M) * PAGE_SIZE_2M;
        let end = base + size;
        mapped += mapper
            .map_identity_2mib(&mut memory, start, end, WRITABLE)
            .unwrap_or(0);
    }
    for (base, size) in extra_mmio {
        let start = (base / PAGE_SIZE_2M) * PAGE_SIZE_2M;
        let end = base + size;
        mapped += mapper
            .map_identity_2mib(&mut memory, start, end, WRITABLE)
            .unwrap_or(0);
    }

    Some(AddressSpaceReport {
        pml4_phys: pml4,
        mapped_huge: mapped,
        mapped_bytes: mapped as u64 * PAGE_SIZE_2M,
    })
}


/// Load `pml4` into CR3 (the identity map makes this a no-op for all
/// existing translations).
///
/// # Safety
/// `pml4` must be a physical address of a valid, fully populated PML4
/// whose mappings cover everything the current execution touches.
pub(crate) unsafe fn switch_cr3(pml4: u64) {
    // SAFETY: caller guarantees a populated identity PML4 (see doc above).
    unsafe {
        core::arch::asm!("mov cr3, {0}", in(reg) pml4, options(nostack, preserves_flags));
    }
}

/// Gate + build + switch. Returns `true` when CR3 was switched.
pub(crate) fn maybe_switch_cr3(
    allocator: &mut BootstrapFrameAllocator<'_>,
    regions: &[MemoryRegion],
    framebuffer: Option<(u64, u64)>,
    extra_mmio: &[(u64, u64)],
    conf_text: &str,
) -> bool {
    if !config::wants_paging_cr3(conf_text) {
        return false;
    }
    match build_full_address_space(allocator, regions, framebuffer, extra_mmio) {
        Some(report) => {
            let cr4_before = orbita_arch_x86_64::cpu::read_cr4();
            // SAFETY: the map covers the whole low 4 GiB plus every
            // descriptor above it — a superset of everything the firmware
            // tables covered for the running kernel.
            unsafe { switch_cr3(report.pml4_phys) };
            let cr3_after = orbita_arch_x86_64::cpu::read_cr3();
            orbita_platform::log_line_fmt(format_args!(
                "paging: cr3 switched to 0x{:x} huge_pages={} span={} cr4=0x{:x} cr3_now=0x{:x}",
                report.pml4_phys,
                report.mapped_huge,
                orbita_std::diagnostics::format_bytes(report.mapped_bytes),
                cr4_before,
                cr3_after
            ));
            true
        }
        None => {
            orbita_platform::log_line("paging: cr3 build FAILED (staying on firmware tables)");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Stage-A portion 6: ring-3 self-test.
//
// Proves the whole user-mode machinery in the live OS before the SDK
// pipeline migrates to syscalls (roadmap A.4/A.5): GDT user segments +
// TSS.rsp0, the syscall/sysret gate, and USER-flagged pages in the
// kernel-built tables. A tiny position-independent stub runs in ring 3,
// issues SYSCALL_ECHO (kernel answers, sysret back), then SYSCALL_DONE
// which resumes the kernel context that entered the test.
// ---------------------------------------------------------------------------

/// Ring-3 stub machine code written to the app load base (25 bytes):
/// ```text
/// mov rax, SYSCALL_ECHO   ; 48 C7 C0 00 10 00 00
/// mov rdi, 0x5A3CC35A     ; 48 C7 C7 5A C3 3C 5A   (SysV arg1 = magic)
/// syscall                 ; 0F 05
/// mov rax, SYSCALL_DONE   ; 48 C7 C0 01 10 00 00
/// syscall                 ; 0F 05   (kernel resumes; never returns here)
/// ```
const RING3_STUB: [u8; 25] = [
    0x48, 0xC7, 0xC0, 0x00, 0x10, 0x00, 0x00, // mov rax, 0x1000 (ECHO)
    0x48, 0xC7, 0xC7, 0x5A, 0xC3, 0x3C, 0x5A, // mov rdi, magic
    0x0F, 0x05, // syscall
    0x48, 0xC7, 0xC0, 0x01, 0x10, 0x00, 0x00, // mov rax, 0x1001 (DONE)
    0x0F, 0x05, // syscall
];

/// Run the ring-3 roundtrip when `ring3_test=on` and the kernel owns CR3.
pub(crate) fn maybe_ring3_selftest(
    allocator: &mut BootstrapFrameAllocator<'_>,
    conf_text: &str,
    kernel_tables: bool,
) {
    if !config::wants_ring3_test(conf_text) {
        return;
    }
    if !kernel_tables {
        orbita_platform::log_line("ring3: self-test skipped (kernel tables off)");
        return;
    }
    const BASE: u64 = crate::abi::APP_LOAD_BASE; // 0x1000_0000, 1 MiB reserved
    const REGION_PAGES: u64 = 256; // 256 * 4 KiB = the reserved app area

    // Make the app region reachable from ring 3: replace the 2 MiB huge
    // entry with 4 KiB pages carrying USER.
    let mut memory = KernelFrameMemory::new(allocator);
    // The mapper edits the LIVE tables (CR3 == our PML4 since the switch).
    let mut mapper = PageTableMapper::new(orbita_arch_x86_64::cpu::read_cr3());
    match mapper.unmap_page(&mut memory, Virt(BASE)) {
        Ok(_) => {}
        Err(err) => {
            orbita_platform::log_line_fmt(format_args!(
                "ring3: self-test aborted (unmap app region failed: {err:?})"
            ));
            return;
        }
    }
    for page in 0..REGION_PAGES {
        let at = BASE + page * orbita_mm::PAGE_SIZE as u64;
        if mapper
            .map_page(&mut memory, Virt(at), Phys(at), WRITABLE | USER)
            .is_err()
        {
            orbita_platform::log_line("ring3: self-test aborted (USER remap failed)");
            return;
        }
    }
    orbita_platform::log_line_fmt(format_args!(
        "ring3: app region USER-mapped ({} pages at 0x{BASE:x})",
        REGION_PAGES
    ));

    // Write the stub + a user stack inside the region.
    // SAFETY: the region is reserved loader memory, identity-mapped and
    // now USER-accessible; the OS owns every byte of it.
    unsafe {
        core::ptr::copy_nonoverlapping(RING3_STUB.as_ptr(), BASE as *mut u8, RING3_STUB.len());
        let user_rsp = BASE + REGION_PAGES * orbita_mm::PAGE_SIZE as u64 - 0x100;
        orbita_platform::log_line("ring3: stub+stack ready, installing gate");
        orbita_arch_x86_64::syscall::install_syscall_gate();
        orbita_platform::log_line("ring3: gate installed, entering ring 3");
        let ok = orbita_arch_x86_64::syscall::ring3_roundtrip(BASE, user_rsp);
        orbita_platform::log_line("ring3: back in kernel after roundtrip");
        // The syscall gate runs with IF cleared (FMASK); restore the
        // kernel's pre-test interrupt state before continuing the boot.
        orbita_arch_x86_64::cpu::enable_interrupts();
        let count = orbita_arch_x86_64::syscall::syscall_count();
        orbita_platform::log_line_fmt(format_args!(
            "ring3: roundtrip ok={} syscalls={}",
            ok == 0,
            count
        ));
    }
}
