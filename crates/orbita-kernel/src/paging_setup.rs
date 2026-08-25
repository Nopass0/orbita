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
    FrameMemory, PageTableMapper, Phys, Virt, ENTRIES, PAGE_SIZE_2M, WRITABLE,
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
        self.allocator.allocate_frame().map(|frame| frame.start.0)
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
