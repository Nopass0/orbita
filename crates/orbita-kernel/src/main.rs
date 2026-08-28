#![no_main]
#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use core::alloc::Layout;
use core::fmt::Write;
use core::sync::atomic::{AtomicU64, Ordering};
use orbita_desktop::{DesktopRenderer, RedrawScope};
use orbita_core::{
    AppLaunchState, BootSummary, DesktopChromeState, DesktopPointerState, DesktopWorkspaceState, RuntimeEventBuffer,
};
use orbita_drivers::{DeviceManager, DriverRegistry, PciObservation, SystemDeviceKind};
use orbita_fs::{
    BlockAddress, BlockDeviceInfo, BlockSize, FilesystemRuntime, FsCapabilities, FsFeature,
    FsLayout, FsMountDescriptor, FsPartition, MemoryVolume, SpaceReservation, VolumeId,
    VolumeSpaceStats,
};
use orbita_fs::diskfs::OrbitaDiskFs;
use orbita_hw::{
    KEYBOARD_VECTOR, PciInventory, TIMER_VECTOR, bootstrap_interrupts, bootstrap_plan, dispatch,
    install_bootstrap_idt, poll_ps2_data, prepare_lapic_timer, probe_local_apic, probe_smp,
    register_handler,
};
use orbita_mm::{BootstrapFrameAllocator, KernelAllocator, PAGE_SIZE};
use orbita_net::{Ipv4Address, NetworkInterface, NetworkStack, NicDriverKind};
use orbita_process::{OrbExecBuilder, ProcessEngine};
use orbita_platform as platform;
use orbita_proto::{BootInfo, PlatformKind};
use orbita_runtime::{LocalExecutor, TaskSpec};
use orbita_shell::{ShellEnvironment, ShellOutput, ShellRuntime, ShellSystemInfo};
use orbita_std::{BTreeMap, String, VecDeque, diagnostics, memory, println};
use orbita_video::{FrameCompositor, Framebuffer};
use uefi::boot::{self as uefi_boot, AllocateType, MemoryType};
use uefi::mem::memory_map::{MemoryMap, MemoryType as UefiMemoryType};
use uefi::prelude::*;

mod abi;
mod boot;
mod console;
mod config;
mod disk;
mod drivers;
mod seed;
mod ui;
mod input;
mod paging_setup;
mod hosts;

use boot::*;
use console::*;
use config::*;
use disk::*;
use seed::*;
use ui::*;
use input::*;

pub(crate) const HEAP_PAGES: usize = 8192;

pub(crate) const NEBULAFS_FEATURES: &[FsFeature] = &[
    FsFeature::Journaling,
    FsFeature::CopyOnWrite,
    FsFeature::Extents,
    FsFeature::Compression,
    FsFeature::Checksums,
];

pub(crate) static TIMER_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

pub(crate) static KEYBOARD_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator::new();

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    platform::init_early_console();
    platform::log_line("Orbita OS: UEFI entry reached");

    let mut gop = open_graphics_output().unwrap_or_else(|status| {
        platform::log_line("Orbita OS: GOP unavailable");
        halt_with_status(status)
    });

    select_best_mode(&mut gop);

    let framebuffer = framebuffer_from_gop(&mut gop).unwrap_or_else(|status| {
        platform::log_line("Orbita OS: framebuffer mode unsupported");
        halt_with_status(status)
    });

    println!(
        "Orbita OS: framebuffer {}x{} stride={} bpp={}",
        framebuffer.width,
        framebuffer.height,
        framebuffer.stride,
        framebuffer.bytes_per_pixel * 8
    );

    // Reserve the native-application load region (matches the orbita-build
    // linker base) BEFORE the heap allocation, so UEFI never hands these
    // pages to the kernel heap or anything else. 1 MiB is plenty for v1
    // statically linked apps.
    let app_region = uefi_boot::allocate_pages(
        AllocateType::Address(crate::abi::APP_LOAD_BASE),
        MemoryType::LOADER_DATA,
        256,
    );
    if app_region.is_err() {
        platform::log_line_fmt(format_args!(
            "Orbita OS: app load region 0x{:x} unavailable — native apps disabled",
            crate::abi::APP_LOAD_BASE
        ));
    }

    let heap_ptr = uefi_boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        HEAP_PAGES,
    )
    .unwrap_or_else(|_| {
        platform::log_line("Orbita OS: failed to allocate heap pages");
        halt_with_status(Status::OUT_OF_RESOURCES)
    });

    drop(gop);

    // Boot services become invalid after this point. We convert every firmware
    // structure we still need into Orbita-owned data before continuing.
    let memory_map = unsafe { uefi_boot::exit_boot_services(Some(UefiMemoryType::LOADER_DATA)) };

    let mut boot_info = BootInfo::new(PlatformKind::X86_64Uefi, framebuffer);
    for descriptor in memory_map.entries() {
        let region = map_memory_region(descriptor);
        let _ = boot_info.push_memory_region(region);
    }

    unsafe {
        ALLOCATOR.init(heap_ptr, HEAP_PAGES * PAGE_SIZE);
    }
    // Native applications execute from loader-data pages — drop NX.
    abi::disable_nx();

    kernel_main(boot_info)
}

fn kernel_main(boot_info: BootInfo) -> ! {
    platform::log_line("Orbita OS: boot services exited");
    platform::log_line("Orbita OS: GUI desktop build active");

    let summary = BootSummary::from_boot_info(&boot_info);
    let mut framebuffer = Framebuffer::new(boot_info.framebuffer);
    platform::log_line("Orbita OS: drawing framebuffer scene");
    draw_boot_scene(&mut framebuffer, &summary);
    platform::log_line("Orbita OS: framebuffer scene ready");

    let mut frame_allocator = BootstrapFrameAllocator::new(boot_info.memory_regions());
    let mut sample_frames = Vec::new();
    for _ in 0..8 {
        if let Some(frame) = frame_allocator.allocate_frame() {
            sample_frames.push(frame.number());
        }
    }

    let mut heap_text = String::new();
    let _ = write!(
        &mut heap_text,
        "usable={} total={}",
        diagnostics::format_bytes(summary.usable_memory_bytes),
        diagnostics::format_bytes(summary.total_memory_bytes)
    );

    let scratch = memory::boxed_slice(128, 0x3C);
    let scratch_sum: usize = scratch.iter().map(|byte| *byte as usize).sum();

    // std-facade smoke: ordered map + deque + formatting, all on the
    // in-tree heap allocator.
    let mut boot_phases: BTreeMap<&str, u32> = BTreeMap::new();
    boot_phases.insert("runtime", 3);
    boot_phases.insert("memory", 1);
    boot_phases.insert("graphics", 2);
    let mut phase_queue = VecDeque::from(vec![
        String::from("memory"),
        String::from("graphics"),
        String::from("runtime"),
    ]);
    let first_phase = phase_queue.pop_front().unwrap_or_default();
    println!(
        "Orbita OS: std smoke phases={} first={} order={:?}",
        boot_phases.len(),
        first_phase,
        boot_phases.keys().collect::<Vec<_>>()
    );

    let driver_registry = DriverRegistry::new();
    let summary_drivers = driver_registry.summary();
    println!(
        "Orbita OS: driver catalog total={} gpu={} input={} net={} sound={} storage={}",
        summary_drivers.total,
        summary_drivers.gpu,
        summary_drivers.input,
        summary_drivers.net,
        summary_drivers.sound,
        summary_drivers.storage
    );

    // --- Runtime: real cooperative async executor smoke test. ---
    let mut executor = LocalExecutor::new();
    let _ = executor.spawn_with_spec(TaskSpec::new("boot-smoke"), orbita_runtime::yield_now());
    executor.run_until_idle();
    println!(
        "Orbita OS: async runtime smoke: tasks={} completed={}",
        executor.len(),
        executor.completed_tasks()
    );

    let pci_inventory = PciInventory::scan();
    let gpu_identity = detect_primary_gpu(&pci_inventory);
    let pci_inventory_lines = build_pci_inventory_report(&pci_inventory, gpu_identity.as_deref());
    for line in &pci_inventory_lines {
        platform::log_line(line);
    }

    // --- Device manager: classify every PCI device, list monitors. ---
    let mut device_manager = DeviceManager::new();
    let mut observations = Vec::new();
    for device in pci_inventory.devices() {
        observations.push(PciObservation {
            bus: device.address.bus,
            device: device.address.device,
            function: device.address.function,
            vendor_id: device.vendor_id.0,
            device_id: device.device_id.0,
            class: device.class_code.class,
            subclass: device.class_code.subclass,
            programming_interface: device.class_code.programming_interface,
        });
    }
    device_manager.observe_all(&observations);
    device_manager
        .monitors
        .push_firmware(summary.framebuffer_width as u32, summary.framebuffer_height as u32);
    if let Some(gpu) = gpu_identity.as_deref() {
        device_manager
            .monitors
            .push_virtual(gpu, summary.framebuffer_width as u32, summary.framebuffer_height as u32);
    }
    for line in &device_manager.report_lines() {
        platform::log_line(line);
    }

    // --- Driver platform: bind registered drivers to discovered devices. ---
    let mut device_probes = drivers::pci_probes(&pci_inventory);
    device_probes.push(orbita_drivers::DeviceProbe::legacy("ps2-keyboard"));
    let (mut driver_manager, bind_report) = drivers::bind_builtin_drivers(&device_probes);
    drivers::log_bind_report(&bind_report);
    println!(
        "Orbita OS: drivers registered={} bound={} failed={}",
        driver_manager.len(),
        bind_report.bound(),
        bind_report.records.len() - bind_report.bound()
    );

    // --- Network stack: loopback + the live e1000 NIC (QEMU user-net). ---
    let mut net_stack = NetworkStack::new();
    let mut live_nic = driver_manager
        .by_name_any("e1000")
        .and_then(|any| any.downcast_mut::<drivers::E1000NetDriver>())
        .and_then(|driver| driver.take_nic());
    if let Some(nic) = live_nic.as_ref() {
        net_stack.add_interface(NetworkInterface::from_nic(
            &orbita_net::NicInfo {
                pci_address: String::from("e1000"),
                driver: NicDriverKind::IntelE1000,
                mac: orbita_net::MacAddress::new(nic.mac()),
                status: orbita_net::NicStatus::Up { speed_mbps: 1000 },
            },
            Ipv4Address::new([10, 0, 2, 15]),
            Some(Ipv4Address::new([10, 0, 2, 2])),
            24,
        ));
    }
    println!("Orbita OS: {}", net_stack.summary());
    let net_nics = device_manager.count(SystemDeviceKind::Network);
    println!(
        "Orbita OS: network inventory pci_nics={} interfaces={} arp_cache={}",
        net_nics,
        net_stack.interfaces.len(),
        net_stack.arp.len()
    );
    let local_apic = probe_local_apic();
    let idt = install_bootstrap_idt();
    // Stage-A portion 6: kernel/user GDT + TSS (rsp0/IST). Selectors are
    // layout-compatible with the bootstrap IDT (0x08), so installing the
    // GDT right after the IDT keeps every existing entry valid.
    let gdt = orbita_arch_x86_64::gdt::install_kernel_gdt();
    // Stage-A portion 7: the syscall gate serves ABI v2 for both ring-0
    // and ring-3 execution; the kernel dispatcher owns the ops.
    orbita_arch_x86_64::syscall::install_syscall_gate();
    orbita_arch_x86_64::syscall::set_dispatcher(Some(abi::syscall_entry));
    register_handler(TIMER_VECTOR, on_timer_irq);
    register_handler(KEYBOARD_VECTOR, on_keyboard_irq);
    let interrupt_bootstrap = bootstrap_interrupts(local_apic, KEYBOARD_VECTOR);
    println!(
        "Orbita OS: apic present={} enabled={} x2apic={} bsp={} base=0x{:x}",
        local_apic.present,
        local_apic.enabled,
        local_apic.x2apic,
        local_apic.bootstrap_processor,
        local_apic.physical_base
    );
    println!(
        "Orbita OS: idt installed vectors={} timer_vector={} keyboard_vector={} spurious_vector={} fault_handlers={:?}",
        idt.vectors_installed,
        idt.timer_vector,
        idt.keyboard_vector,
        idt.spurious_vector,
        idt.fault_vectors
    );
    println!(
        "Orbita OS: gdt installed selectors={} tss=0x{:x} rsp0=0x{:x} user_cs=0x{:x}",
        gdt.selectors,
        gdt.tss_address,
        gdt.rsp0,
        orbita_arch_x86_64::gdt::USER_CODE64_SELECTOR
    );
    println!(
        "Orbita OS: ioapic base=0x{:x} redirs={} keyboard_irq_line={} keyboard_vector={} masked={}",
        interrupt_bootstrap.io_apic.physical_base,
        interrupt_bootstrap.io_apic.max_redirection_entries,
        interrupt_bootstrap.keyboard_route.line.0,
        interrupt_bootstrap.keyboard_route.vector,
        interrupt_bootstrap.keyboard_route.masked
    );

    let smp = probe_smp();
    println!(
        "Orbita OS: smp logical_cpus={} initial_apic_id={} hyperthreading={}",
        smp.logical_cpus,
        smp.initial_apic_id,
        smp.hyperthreading
    );
    // Real AP bring-up: wake the other cores via INIT-SIPI-SIPI now that
    // the local APIC is enabled.
    unsafe {
        orbita_arch_x86_64::smp_ap::AP_DEBUG2 = Some(|svr, esr| {
            println!("Orbita OS: lapic svr=0x{:x} esr=0x{:x}", svr as u32, esr as u32);
        });
        orbita_arch_x86_64::smp_ap::AP_DEBUG = Some(|first8, _gdt8| {
            println!(
                "Orbita OS: ap progress bytes = 0x{:08x} (stages 1-4)",
                first8 as u32
            );
        });
    }

    let timer_plan = bootstrap_plan();
    abi::set_time_scale(1000 / timer_plan.quantum_hz.max(1) as u64);
    abi::set_os_summary(format!(
        "Orbita OS 0.1.0 x86_64-uefi | renderer=software-framebuffer | cpus={} | heap_pages={}",
        smp.logical_cpus, HEAP_PAGES
    ));
    let lapic_timer = prepare_lapic_timer(&local_apic, TIMER_VECTOR);
    let timer_dispatch_smoke = dispatch(TIMER_VECTOR);
    println!(
        "Orbita OS: timer source={} quantum_hz={} preemptive_ready={}",
        timer_plan.source,
        timer_plan.quantum_hz,
        timer_plan.preemptive_ready
    );
    println!(
        "Orbita OS: lapic timer configured={} vector={} initial_count={} masked={} dispatch_smoke={}",
        lapic_timer.configured,
        lapic_timer.vector,
        lapic_timer.initial_count,
        lapic_timer.masked,
        timer_dispatch_smoke
    );

    let fs_caps = FsCapabilities {
        block_size: BlockSize(4096),
        features: NEBULAFS_FEATURES,
    };
    println!(
        "Orbita OS: nebulafs block_size={} features={}",
        fs_caps.block_size.0,
        fs_caps.features.len()
    );

    let mut fs_runtime = FilesystemRuntime::new();
    let ramdisk = BootstrapRamDisk::new(BlockSize(4096), 256);
    let mount_descriptor = FsMountDescriptor::new(
        VolumeId(0x0B17_AF5_0001),
        FsLayout {
            partition: FsPartition {
                volume: VolumeId(0x0B17_AF5_0001),
                superblock: BlockAddress(1),
                inode_table: BlockAddress(8),
                journal_start: BlockAddress(128),
            },
            capacity_blocks: ramdisk.geometry().block_count,
            reserved: SpaceReservation {
                data_blocks: 64,
                metadata_blocks: 16,
            },
            capabilities: fs_caps,
        },
    );
    let (mounted_volume, mounted_flag, replay_required) = {
        let mounted = fs_runtime.mount(mount_descriptor, &ramdisk).unwrap_or_else(|_| {
            platform::log_line("Orbita OS: nebulafs mount failed");
            platform::halt_forever()
        });
        (
            mounted.descriptor.volume.0,
            mounted.mounted,
            mounted.replay_required,
        )
    };
    let runtime_mounts = fs_runtime.len();
    let volume_space = VolumeSpaceStats::from_layout(ramdisk.geometry(), mount_descriptor.layout);
    println!(
        "Orbita OS: nebulafs mounted volume={} mounted={} replay_required={} runtime_mounts={}",
        mounted_volume,
        mounted_flag,
        replay_required,
        runtime_mounts
    );
    println!(
        "Orbita OS: nebulafs capacity={} free={} used={}%",
        diagnostics::format_bytes(volume_space.total_bytes()),
        diagnostics::format_bytes(volume_space.available_bytes()),
        volume_space.used_percent()
    );

    platform::log_line("Orbita OS: memory and graphics initialized");
    platform::log_line(&heap_text);
    platform::log_line("Orbita OS: first usable frames:");
    for frame in sample_frames {
        let mut line = String::new();
        let _ = write!(&mut line, "  frame #{frame}");
        platform::log_line(&line);
    }
    let mut line = String::new();
    let _ = write!(&mut line, "  scratch checksum={scratch_sum}");
    platform::log_line(&line);

    let mut shell_fs = MemoryVolume::new(
        VolumeId(0x0B17_AF5_1001),
        BlockSize(4096),
        262_144,
        fs_caps,
    );
    seed_shell_volume(&mut shell_fs).unwrap_or_else(|err| {
        let mut message = String::new();
        let _ = write!(&mut message, "Orbita OS: memory volume seed failed: {:?}", err);
        platform::log_line(&message);
        platform::halt_forever()
    });
    seed_runtime_inventory(&mut shell_fs, &pci_inventory, gpu_identity.as_deref()).unwrap_or_else(|err| {
        let mut message = String::new();
        let _ = write!(
            &mut message,
            "Orbita OS: runtime inventory seed failed: {:?}",
            err
        );
        platform::log_line(&message);
        platform::halt_forever()
    });

    let mut console = BootConsole::new();

    let mut persistent_fs: Option<(OrbitaDiskFs, AhciSectorDisk, String)> = None;
    let orbita_disk = driver_manager
        .by_name_any("ahci-storage")
        .and_then(|any| any.downcast_mut::<drivers::AhciStorageDriver>())
        .and_then(|driver| driver.take_disk());
    // The ESP FAT drive (firmware boot volume) is the host→OS delivery
    // channel: .orbpkg bundles staged by `dm` land in /pkg via read-only
    // FAT, no kernel rebuild needed.
    let mut esp_disk = driver_manager
        .by_name_any("ahci-storage")
        .and_then(|any| any.downcast_mut::<drivers::AhciStorageDriver>())
        .and_then(|driver| driver.take_esp_disk());
    if let Some(esp) = esp_disk.as_mut() {
        match orbita_fs::fat::FatVolume::mount(esp) {
            Ok(mut fat) => {
                println!("Orbita OS: esp fat mounted kind={:?}", fat.kind());
                if let Ok(entries) = fat.list_dir("/pkg") {
                    let _ = shell_fs.create_dir_all("/pkg");
                    let mut staged = 0usize;
                    for entry in &entries {
                        if entry.is_dir {
                            continue;
                        }
                        if let Ok(bytes) = fat.read_file(&alloc::format!("/pkg/{}", entry.name)) {
                            let target = alloc::format!("/pkg/{}", entry.name);
                            if shell_fs.create_file_path(&target, &bytes).is_ok() {
                                staged += 1;
                            }
                        }
                    }
                    println!("Orbita OS: pkg delivery staged {staged} bundle(s) from esp");

                    // Auto-install + run demo bundles marked auto=1 in
                    // their manifest (proves the whole native pipeline
                    // boots-headless: build -> /pkg -> install -> exec).
                    if let Ok(entries) = shell_fs.list_path("/pkg") {
                        for entry in entries.entries {
                            let path = alloc::format!("/pkg/{}", entry.name);
                            let Ok(bytes) = shell_fs.read_file_path(&path) else {
                                println!("Orbita OS: autorun skip {path}: unreadable");
                                continue;
                            };
                            println!("Orbita OS: autorun candidate {path} ({} bytes)", bytes.len());
                            let Ok(binary) = orbita_process::OrbExec::parse(&bytes) else {
                                println!(
                                    "Orbita OS: autorun skip {path}: parse failed magic={:?} hdr={:02x?}",
                                    &bytes.get(..8),
                                    &bytes.get(8..24).map(|s| s.to_vec()).unwrap_or_default()
                                );
                                continue;
                            };
                            println!("Orbita OS: autorun parsed {} auto={:?}", binary.name(), binary.manifest().get("auto"));
                            if binary.manifest().get("auto").map(|v| v == "1") != Some(true) {
                                continue;
                            }
                            let _ = shell_fs.create_dir_all("/apps");
                            let target = alloc::format!("/apps/{}", entry.name);
                            let _ = shell_fs.create_file_path(&target, &bytes);
                            let net_info = net_stack.summary();
                            match abi::exec_native(&mut shell_fs, net_info, binary.payload(), false) {
                                Ok(run) => println!(
                                    "Orbita OS: autorun {} exit={}",
                                    binary.name(),
                                    run.code
                                ),
                                Err(err) => println!("Orbita OS: autorun failed: {err}"),
                            }
                        }
                    }
                    println!("Orbita OS: autorun loop done");
                }
            }
            Err(err) => {
                let mut sector = [0u8; 512];
                if orbita_fs::diskfs::SectorDevice::read_sector(esp, 0, &mut sector) {
                    println!(
                        "Orbita OS: esp fat mount failed: {err:?} bps={} spc={} reserved={} fats={} rootents={} fat16={} fstype={}",
                        u16::from_le_bytes([sector[11], sector[12]]),
                        sector[13],
                        u16::from_le_bytes([sector[14], sector[15]]),
                        sector[16],
                        u16::from_le_bytes([sector[17], sector[18]]),
                        u16::from_le_bytes([sector[22], sector[23]]),
                        String::from_utf8_lossy(&sector[54..62])
                    );
                } else {
                    println!("Orbita OS: esp fat mount failed: {err:?} (sector read failed too)");
                }
            }
        }
    }
    println!("Orbita OS: init persistent disk");
    match orbita_disk.and_then(init_persistent_disk) {
        Some((mut diskfs, mut disk, boots)) => {
            seed_system_layout(&mut diskfs, &mut disk);
            // Load the REAL configuration from disk and apply it live.
            let conf_text = String::from_utf8(
                diskfs.read_file(&mut disk, ORBITA_CONF).unwrap_or_default(),
            )
            .unwrap_or_default();
            let conf_text = if conf_text.is_empty() {
                String::from(orbita_conf_default())
            } else {
                conf_text
            };
            apply_orbita_conf(&conf_text, &mut console, &mut shell_fs);
            // Stage-A: prove the frame supply + mapper end-to-end (no CR3 switch).
            paging_setup::maybe_run_dry_run(
                &mut frame_allocator,
                boot_info.memory_regions(),
                &conf_text,
                1 << 30, // dry-run covers low 1 GiB
            );
            // Stage-A portion 4: switch CR3 to the kernel identity map.
            let fb = framebuffer.info;
            let switched = paging_setup::maybe_switch_cr3(
                &mut frame_allocator,
                boot_info.memory_regions(),
                Some((fb.base as u64, fb.size_bytes as u64)),
                &[(local_apic.physical_base, 0x1000), (0xFEC0_0000, 0x1000)],
                &conf_text,
            );
            // Stage-A portion 6: ring-3 + syscall/sysret roundtrip.
            paging_setup::maybe_ring3_selftest(&mut frame_allocator, &conf_text, switched);
            // Stage-A portion 7: re-run every installed app as a ring-3
            // user process (the first autorun pass ran on ring 0 before
            // the kernel tables existed). Proves `run` isolation per boot.
            if switched && config::wants_apps_ring3(&conf_text) {
                if let Ok(apps) = shell_fs.list_path("/apps") {
                    for entry in apps.entries {
                        let path = alloc::format!("/apps/{}", entry.name);
                        let Ok(bytes) = shell_fs.read_file_path(&path) else {
                            continue;
                        };
                        let Ok(binary) = orbita_process::OrbExec::parse(&bytes) else {
                            continue;
                        };
                        let net_info = net_stack.summary();
                        match abi::exec_native(&mut shell_fs, net_info, binary.payload(), true) {
                            Ok(run) => println!(
                                "Orbita OS: autorun3 {} exit={} ring3",
                                binary.name(),
                                run.code
                            ),
                            Err(err) => println!("Orbita OS: autorun3 {} failed: {err}", binary.name()),
                        }
                    }
                }
            }
            let kind = disk.inner.storage_kind().label();
            println!(
                "Orbita OS: orbitafs medium={} capacity={} used={} ({}.{:02}%) boots={boots} files={} dirs={}",
                kind,
                diagnostics::format_bytes(diskfs.capacity_bytes()),
                diagnostics::format_bytes(diskfs.used_bytes()),
                diskfs.usage_percent_hundredths() / 100,
                diskfs.usage_percent_hundredths() % 100,
                diskfs.file_count(),
                diskfs.dir_count()
            );
            if let Some(bin) = diskfs.list_dir("/bin") {
                println!("Orbita OS: /bin: {}", bin.join(" "));
            }
            if let Some(lib) = diskfs.list_dir("/lib") {
                println!("Orbita OS: /lib: {}", lib.join(" "));
            }
            if let Some(boot) = diskfs.list_dir("/boot") {
                println!("Orbita OS: /boot: {}", boot.join(" "));
            }
            println!("Orbita OS: config from disk: hostname={}", console.hostname);
    // AP bring-up is config-gated (experimental on OVMF/QEMU): enable
    // with  in /etc/orbita.conf. See docs/processes.md.
    let smp_enabled = persistent_conf_flag("smp");
    if smp_enabled {
        let cores_online = orbita_arch_x86_64::smp_ap::bring_up_aps(
            smp.logical_cpus as u32,
            local_apic.physical_base,
        );
        println!(
            "Orbita OS: smp online_cores={} expected={} (AP bring-up {})",
            cores_online,
            smp.logical_cpus,
            if cores_online as u8 == smp.logical_cpus { "ok" } else { "partial" }
        );
    }

            persistent_fs = Some((diskfs, disk, conf_text));
        }
        None => println!("Orbita OS: persistent disk unavailable"),
    }

    // ------------------------------------------------------------------
    // Compile the shell into an ORBEXEC binary BY OS RULES, install it
    // into the filesystem and spawn it as a real root process with
    // stdin/stdout/stderr — the console becomes its terminal.
    // ------------------------------------------------------------------
    let shell_binary = OrbExecBuilder::new("orbita-shell", "shell_main")
        .with_root()
        .manifest_line("commands=help,ls,cat,write,env,apps,launch,pkg,apt,sh")
        .manifest_line("terminal=framebuffer-console")
        .payload(b"orbita shell service payload v1\n")
        .build();
    if let Some((diskfs, disk, _)) = persistent_fs.as_mut() {
        if diskfs.write_file(disk, "/bin/orbita-shell.orbexec", &shell_binary) {
            println!("Orbita OS: compiled /bin/orbita-shell.orbexec ({} bytes)", shell_binary.len());
        }
    }
    let _ = shell_fs.create_file_path("/bin/orbita-shell.orbexec", &shell_binary);
    let mut process_engine = Some(ProcessEngine::new());
    process_engine.as_mut().unwrap().set_logical_cpus(smp.logical_cpus as u32);
    let shell_exec = orbita_process::OrbExec::parse(&shell_binary).expect("valid orbexec");
    let shell_pid = process_engine.as_mut().unwrap().spawn(shell_exec).expect("spawn shell");
    if let Some(process) = process_engine.as_ref().unwrap().process(shell_pid) {
        println!(
            "Orbita OS: process spawned name={} pid={} uid={} cpu={} fds=stdin,stdout,stderr",
            process.name(),
            process.pid().0,
            process.privileges().label(),
            process.cpu_affinity
        );
    }

    // Make the shell filesystem-real: mount the persistent volume into
    // the RAM workspace so ls/cat/write operate on on-disk state.
    if let Some((diskfs, disk, _)) = persistent_fs.as_mut() {
        load_persistent_into_ram(diskfs, disk, &mut shell_fs);
        let entries = diskfs.list_dir("/bin").map(|v| v.len()).unwrap_or(0);
        println!("Orbita OS: vfs bridge up, /bin visible to shell ({} entries)", entries);
    }

    console.push_line("Orbita Console ready.");
    console.push_line("Type help to see available commands.");
    console.push_line("Commands: help, ls, cat, pipes, redirects, env, apps, launch <app-id>, sh /etc/profile.sh.");
    console.push_line("Linux-like installs: pkg install python3 nodejs rust build-essential  |  apt install python3 nodejs rust build-essential");
    console.push_line("Desktop shortcuts: Tab cycles apps, F1..F4 launch terminal/files/settings/monitor.");
    console.push_line("Pointer shortcuts: arrows move cursor, alt activates hovered surface.");
    for line in &pci_inventory_lines {
        console.push_line(line);
    }
    // The ps2-keyboard driver bound during the driver-platform pass; the
    // console only reflects its state here.
    if driver_manager.by_name("ps2-keyboard").is_some() {
        console.set_status("keyboard: ps/2 polling active");
    } else {
        console.set_status("keyboard: ps/2 unavailable");
    }

    let shell_runtime = ShellRuntime::new();
    let mut shell_env = ShellEnvironment::new(ShellSystemInfo::new(
        "Orbita OS x86_64-uefi",
        gpu_identity.as_deref().unwrap_or("UEFI GOP framebuffer"),
        format!(
            "memory usable={} total={}",
            diagnostics::format_bytes(summary.usable_memory_bytes),
            diagnostics::format_bytes(summary.total_memory_bytes)
        ),
        format!("{}x{}", summary.framebuffer_width, summary.framebuffer_height),
        smp.logical_cpus.into(),
    ));
    let mut keyboard = Ps2KeyboardDecoder::new();
    let mut workspace = DesktopWorkspaceState::new();
    if let Some((_, _, conf)) = persistent_fs.as_ref() {
        workspace.set_graphics_backend(config::preferred_graphics_backend(conf));
    }
    let mut desktop_compositor = FrameCompositor::new(
        framebuffer.size(),
        orbita_video::create_backend(workspace.graphics_backend().label(), framebuffer.info),
    );
    let desktop_renderer = DesktopRenderer::new();
    let mut app_launch = AppLaunchState::new();
    let mut chrome = DesktopChromeState::new();
    let mut pointer = DesktopPointerState::new(
        framebuffer.width().saturating_sub(180),
        framebuffer.height().saturating_sub(160),
    );
    let mut runtime_events = RuntimeEventBuffer::new(24);
    runtime_events.push("boot: orbita runtime initialized");
    runtime_events.push(format!(
        "boot: framebuffer={}x{}",
        summary.framebuffer_width, summary.framebuffer_height
    ));
    runtime_events.push(format!("boot: active app={}", app_launch.active_app().id));
    let mut frame_counter: u32 = 0;

    let mut pending_scope = Some(RedrawScope::Chrome);
    let mut blink_scope: Option<RedrawScope> = None;
    let mut panels = PanelCache {
        total_text: String::new(),
        free_text: String::new(),
        chrome_body: String::new(),
        files_cwd: String::new(),
        files_listing: String::new(),
        files_selected_name: String::new(),
        files_preview_text: String::new(),
        toolchains_combined_text: String::new(),
        network_combined_text: String::new(),
        services_text: String::new(),
        runtime_services_text: String::new(),
        events_text: String::new(),
    };
    panels.refresh(&mut shell_fs, &chrome, &workspace, &shell_env);

    loop {
        // Network: drain received frames into the stack and push queued
        // replies (ARP answers, ICMP echoes) out through the e1000 NIC.
        if let Some(nic) = live_nic.as_mut() {
            for frame in nic.poll_rx() {
                for event in net_stack.receive(&frame) {
                    match event {
                        orbita_net::StackEvent::IcmpEchoRequest { source, .. } => {
                            println!("Orbita OS: net icmp echo request from {}", source.text());
                        }
                        orbita_net::StackEvent::ArpResolved { ip, mac } => {
                            println!("Orbita OS: net arp {} -> {}", ip.text(), mac.text());
                        }
                        _ => {}
                    }
                }
            }
            while let Some(frame) = net_stack.take_tx_frame() {
                nic.send(&frame);
            }
        }

        // Drain the whole PS/2 queue each iteration so typed characters
        // are handled back-to-back instead of one scancode per spin.
        let mut scancode_budget = 32;
        while scancode_budget > 0 {
            let Some(scancode) = poll_ps2_data() else {
                break;
            };
            scancode_budget -= 1;
            if let Some(action) = keyboard.feed(scancode) {
                match handle_console_action(
                    &mut console,
                    &mut shell_fs,
                    &mut process_engine,
                    &mut net_stack,
                    &mut live_nic,
                    &shell_runtime,
                    &mut shell_env,
                    &mut app_launch,
                    &mut chrome,
                    &mut workspace,
                    &mut pointer,
                    framebuffer.width(),
                    framebuffer.height(),
                    &mut runtime_events,
                    action,
                ) {
                    RedrawKind::None => {}
                    RedrawKind::PromptOnly => {
                        // A typed character only changes the prompt line:
                        // repaint the smallest possible region.
                        pending_scope = Some(match pending_scope {
                            Some(RedrawScope::Chrome) => RedrawScope::Chrome,
                            Some(RedrawScope::Full) => RedrawScope::Full,
                            _ => RedrawScope::Prompt,
                        });
                    }
                    RedrawKind::Full => {
                        pending_scope = Some(match pending_scope {
                            Some(RedrawScope::Chrome) => RedrawScope::Chrome,
                            _ => RedrawScope::Full,
                        });
                        // Live config sync: if the user rewrote
                        // /etc/orbita.conf in the shell, persist it to the
                        // real disk and re-apply the settings now.
                        if let Some((diskfs, disk, persisted)) = persistent_fs.as_mut() {
                            if let Ok(bytes) = shell_fs.read_file_path("/etc/orbita.conf") {
                                let text = String::from_utf8_lossy(&bytes).into_owned();
                                if text != *persisted {
                                    if diskfs.write_file(disk, ORBITA_CONF, text.as_bytes()) {
                                        apply_orbita_conf(&text, &mut console, &mut shell_fs);
                                        *persisted = text;
                                        println!("Orbita OS: /etc/orbita.conf saved to disk, hostname={}", console.hostname);
                                    }
                                }
                            }
                        }
                        // Full VFS sync: persist every changed user file.
                        if let Some((diskfs, disk, _)) = persistent_fs.as_mut() {
                            let written = sync_ram_to_disk(diskfs, disk, &mut shell_fs);
                            if written > 0 {
                                println!("Orbita OS: vfs sync wrote {written} file(s) to disk");
                            }
                        }
                    }
                }
            }
        }
        let timer_ticks = TIMER_IRQ_COUNT.load(Ordering::Relaxed);
        let next_cursor_visible = ((timer_ticks / 48) & 1) == 0;
        if console.cursor_visible != next_cursor_visible {
            console.cursor_visible = next_cursor_visible;
            // Blink only repaints the prompt pill — never the whole scene.
            if pending_scope.is_none() {
                blink_scope = Some(RedrawScope::Prompt);
            }
        }
        if let Some(scope) = pending_scope.take().or(blink_scope.take()) {
            if scope != RedrawScope::Prompt {
                // Filesystem-visible state changes only on input, so sync
                // and panel refresh are skipped entirely for blink frames.
                let net_live = live_nic.as_ref().map(|nic| {
                    let stats = nic.stats();
                    (stats.rx_frames, stats.rx_bytes, stats.tx_frames, stats.tx_bytes, net_stack.arp.len())
                });
                let _ = sync_runtime_state(
                    &mut shell_fs,
                    &console,
                    &app_launch,
                    &chrome,
                    &workspace,
                    &pointer,
                    frame_counter,
                    &runtime_events,
                    net_live,
                );
                panels.refresh(&mut shell_fs, &chrome, &workspace, &shell_env);
            }
            draw_desktop_ui(
                &summary,
                &mut framebuffer,
                &mut desktop_compositor,
                &console,
                &panels,
                gpu_identity.as_deref().unwrap_or("UEFI GOP framebuffer"),
                smp.logical_cpus.into(),
                frame_counter,
                &mut app_launch,
                &chrome,
                &workspace,
                &pointer,
                &shell_env,
                &desktop_renderer,
                scope,
            );
            frame_counter = frame_counter.wrapping_add(1);
        }
        core::hint::spin_loop();
    }
}

impl orbita_fs::diskfs::SectorDevice for AhciSectorDisk {
    fn read_sector(&mut self, lba: u32, out: &mut [u8; 512]) -> bool {
        self.inner.read_sectors(lba as u64, 1, out)
    }

    fn write_sector(&mut self, lba: u32, data: &[u8; 512]) -> bool {
        self.inner.write_sectors(lba as u64, 1, data)
    }
}

/// Brings up the persistent disk: AHCI port 1 (the orbita-disk image),
/// mounts OrbitaFS, formats on first boot, and maintains a boot counter
/// that proves state survives reboots.

// ---------------------------------------------------------------------------
// Persistent system layout + live configuration.
//
// The disk carries the standard Orbita tree:
//   /bin   system binaries (loaded by the future module loader)
//   /lib   shared libraries
//   /boot  boot-time binaries and the loader config
//   /etc   REAL system configuration: edited inside the OS, applied
//          immediately and persisted to disk
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// VFS bridge: the shell operates on the RAM volume; this bridge makes
// those operations REAL by loading the persistent OrbitaFS into the RAM
// volume at boot and syncing user-writable trees back to disk after
// every shell command.
// ---------------------------------------------------------------------------

fn detect_primary_gpu(pci_inventory: &PciInventory) -> Option<String> {
    pci_inventory.primary_gpu().map(|device| device.summary_line())
}

fn build_pci_inventory_report(
    pci_inventory: &PciInventory,
    gpu_identity: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("Orbita OS: {}", pci_inventory.inventory_summary()));

    match gpu_identity {
        Some(identity) => lines.push(format!(
            "Orbita OS: gpu summary primary={} framebuffer={}",
            identity, "UEFI GOP"
        )),
        None => lines.push(String::from(
            "Orbita OS: gpu summary primary=none framebuffer=UEFI GOP",
        )),
    }

    for gpu in pci_inventory.gpu_devices() {
        lines.push(format!("  gpu candidate {}", gpu.summary_line()));
    }

    for line in pci_inventory.device_lines() {
        lines.push(format!("  pci {line}"));
    }

    lines
}

impl ShellOutput for BootConsole {
    fn write_line(&mut self, line: &str) {
        self.push_line(line);
    }

    fn set_status(&mut self, status: &str) {
        self.set_status(status);
    }

    fn clear(&mut self) {
        self.clear();
    }
}

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    let mut message = String::new();
    let _ = write!(
        &mut message,
        "Orbita OS: allocation failure size={} align={}",
        layout.size(),
        layout.align()
    );
    platform::log_line(&message);
    platform::halt_forever()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    let mut message = String::new();
    let _ = write!(&mut message, "Orbita OS panic: {info}");
    platform::log_line(&message);
    platform::halt_forever()
}

fn on_timer_irq(_: u8) {
    TIMER_IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
}

fn on_keyboard_irq(_: u8) {
    KEYBOARD_IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
}


