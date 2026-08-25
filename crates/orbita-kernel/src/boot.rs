//! Early boot: UEFI GOP mode selection, framebuffer handoff, and memory-map ingestion.


extern crate alloc;

use core::fmt::Write;
use orbita_mm::{MemoryRegion, MemoryRegionKind, PAGE_SIZE, PhysAddr};
use orbita_platform as platform;
use orbita_std::{String, println};
use orbita_video::{FramebufferInfo, PixelFormat};
use uefi::boot::{self, ScopedProtocol};
use uefi::mem::memory_map::MemoryType as UefiMemoryType;
use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, Mode, PixelFormat as UefiPixelFormat};

pub(crate) fn open_graphics_output() -> Result<ScopedProtocol<GraphicsOutput>, Status> {
    let handle = boot::get_handle_for_protocol::<GraphicsOutput>().map_err(|err| err.status())?;
    boot::open_protocol_exclusive::<GraphicsOutput>(handle).map_err(|err| err.status())
}

pub(crate) fn select_best_mode(gop: &mut GraphicsOutput) {
    log_gop_modes(gop);

    let current = gop.current_mode_info().resolution();
    let current_aspect = aspect_ratio(current.0, current.1);

    let best_mode = gop
        .modes()
        .filter(|mode| is_direct_framebuffer_mode(mode))
        .max_by_key(|mode| mode_score(mode, current_aspect));

    if let Some(mode) = best_mode {
        let (width, height) = mode.info().resolution();
        println!(
            "Orbita OS: selected GOP mode {}x{}",
            width, height
        );
        let _ = gop.set_mode(&mode);
    }
}

pub(crate) fn log_gop_modes(gop: &GraphicsOutput) {
    platform::log_line("Orbita OS: available GOP modes:");
    for mode in gop.modes() {
        let (width, height) = mode.info().resolution();
        let format = match mode.info().pixel_format() {
            UefiPixelFormat::Rgb => "rgb",
            UefiPixelFormat::Bgr => "bgr",
            UefiPixelFormat::Bitmask => "bitmask",
            UefiPixelFormat::BltOnly => "blt-only",
        };
        println!(
            "  {}x{} stride={} format={}",
            width,
            height,
            mode.info().stride(),
            format
        );
    }
}

pub(crate) fn is_direct_framebuffer_mode(mode: &Mode) -> bool {
    matches!(mode.info().pixel_format(), UefiPixelFormat::Rgb | UefiPixelFormat::Bgr)
}

pub(crate) fn mode_score(mode: &Mode, current_aspect: (usize, usize)) -> (u8, u8, u8, usize, usize) {
    let (width, height) = mode.info().resolution();
    let aspect = aspect_ratio(width, height);
    let full_hd = if width == 1920 && height == 1080 { 1 } else { 0 };
    let widescreen_16_9 = if aspect == (16, 9) { 1 } else { 0 };
    let aspect_match = if aspect == current_aspect { 1 } else { 0 };
    let non_square_area = if width != height { area_or_zero(width, height) } else { 0 };
    (full_hd, widescreen_16_9, aspect_match, non_square_area, width)
}

pub(crate) fn area_or_zero(width: usize, height: usize) -> usize {
    width.saturating_mul(height)
}

pub(crate) fn aspect_ratio(width: usize, height: usize) -> (usize, usize) {
    let gcd = gcd(width.max(1), height.max(1));
    (width / gcd, height / gcd)
}

pub(crate) fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let rem = a % b;
        a = b;
        b = rem;
    }
    a.max(1)
}

pub(crate) fn framebuffer_from_gop(gop: &mut GraphicsOutput) -> Result<FramebufferInfo, Status> {
    let info = gop.current_mode_info();
    let format = match info.pixel_format() {
        UefiPixelFormat::Rgb => PixelFormat::Rgb,
        UefiPixelFormat::Bgr => PixelFormat::Bgr,
        _ => return Err(Status::UNSUPPORTED),
    };

    let mut buffer = gop.frame_buffer();
    let (width, height) = info.resolution();

    Ok(FramebufferInfo {
        base: buffer.as_mut_ptr(),
        size_bytes: buffer.size(),
        width,
        height,
        stride: info.stride(),
        bytes_per_pixel: 4,
        format,
    })
}

pub(crate) fn map_memory_region(descriptor: &uefi::mem::memory_map::MemoryDescriptor) -> MemoryRegion {
    let kind = match descriptor.ty {
        UefiMemoryType::CONVENTIONAL => MemoryRegionKind::Usable,
        UefiMemoryType::LOADER_CODE | UefiMemoryType::LOADER_DATA => MemoryRegionKind::Kernel,
        UefiMemoryType::BOOT_SERVICES_CODE | UefiMemoryType::BOOT_SERVICES_DATA => MemoryRegionKind::Usable,
        UefiMemoryType::ACPI_NON_VOLATILE | UefiMemoryType::ACPI_RECLAIM => MemoryRegionKind::Acpi,
        UefiMemoryType::MMIO | UefiMemoryType::MMIO_PORT_SPACE => MemoryRegionKind::Mmio,
        _ => MemoryRegionKind::Reserved,
    };

    MemoryRegion {
        start: PhysAddr(descriptor.phys_start),
        len_bytes: descriptor.page_count.saturating_mul(PAGE_SIZE as u64),
        kind,
    }
}

pub(crate) fn halt_with_status(status: Status) -> ! {
    let mut message = String::new();
    let _ = write!(&mut message, "Orbita OS halted with status {:?}", status);
    platform::log_line(&message);
    platform::halt_forever()
}
