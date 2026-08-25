use crate::irq::InterruptLine;
use core::arch::x86_64::__cpuid;
use orbita_arch_x86_64::msr;

const IA32_APIC_BASE: u32 = 0x1B;
const DEFAULT_IOAPIC_BASE: u64 = 0xFEC0_0000;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ApicKind {
    Local,
    Io,
    Unknown,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ApicId(pub u8);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CpuLocalId(pub u32);

#[derive(Debug, Copy, Clone)]
pub struct InterruptController {
    pub kind: ApicKind,
    pub id: ApicId,
}

impl InterruptController {
    pub const fn new(kind: ApicKind, id: ApicId) -> Self {
        Self { kind, id }
    }

    pub fn route(&self, line: InterruptLine) -> InterruptLine {
        line
    }
}

#[derive(Debug, Copy, Clone)]
pub struct LocalApicInfo {
    pub present: bool,
    pub x2apic: bool,
    pub enabled: bool,
    pub bootstrap_processor: bool,
    pub physical_base: u64,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum InterruptDeliveryMode {
    Fixed,
    LowestPriority,
}

#[derive(Debug, Copy, Clone)]
pub struct IoApicInfo {
    pub physical_base: u64,
    pub gsi_base: u32,
    pub max_redirection_entries: u8,
}

#[derive(Debug, Copy, Clone)]
pub struct IoApicRoute {
    pub line: InterruptLine,
    pub vector: u8,
    pub destination_apic_id: u8,
    pub delivery_mode: InterruptDeliveryMode,
    pub masked: bool,
    pub active_low: bool,
    pub level_triggered: bool,
}

#[derive(Debug, Copy, Clone)]
pub struct InterruptBootstrap {
    pub local_apic: LocalApicInfo,
    pub io_apic: IoApicInfo,
    pub keyboard_route: IoApicRoute,
}

pub fn probe_local_apic() -> LocalApicInfo {
    let cpuid = __cpuid(1);
    let present = (cpuid.edx & (1 << 9)) != 0;
    let x2apic = (cpuid.ecx & (1 << 21)) != 0;

    if !present {
        return LocalApicInfo {
            present: false,
            x2apic,
            enabled: false,
            bootstrap_processor: false,
            physical_base: 0,
        };
    }

    let apic_base = unsafe { msr::read(IA32_APIC_BASE) };
    LocalApicInfo {
        present,
        x2apic,
        enabled: (apic_base & (1 << 11)) != 0,
        bootstrap_processor: (apic_base & (1 << 8)) != 0,
        physical_base: apic_base & 0xFFFF_F000,
    }
}

pub fn probe_io_apic() -> IoApicInfo {
    let version = unsafe { ioapic_read(DEFAULT_IOAPIC_BASE, 0x01) };
    let max_redirection_entries = ((version >> 16) & 0xFF) as u8;
    IoApicInfo {
        physical_base: DEFAULT_IOAPIC_BASE,
        gsi_base: 0,
        max_redirection_entries,
    }
}

pub fn bootstrap_interrupts(local_apic: LocalApicInfo, keyboard_vector: u8) -> InterruptBootstrap {
    let io_apic = probe_io_apic();
    let keyboard_route = IoApicRoute {
        line: InterruptLine(1),
        vector: keyboard_vector,
        destination_apic_id: 0,
        delivery_mode: InterruptDeliveryMode::Fixed,
        masked: true,
        active_low: false,
        level_triggered: false,
    };

    if io_apic.max_redirection_entries >= keyboard_route.line.0 {
        unsafe {
            ioapic_write_redirection(io_apic.physical_base, keyboard_route);
        }
    }

    InterruptBootstrap {
        local_apic,
        io_apic,
        keyboard_route,
    }
}

unsafe fn ioapic_select(base: u64, register: u32) {
    let select = base as *mut u32;
    unsafe { select.write_volatile(register) };
}

unsafe fn ioapic_window(base: u64) -> *mut u32 {
    (base + 0x10) as *mut u32
}

unsafe fn ioapic_read(base: u64, register: u32) -> u32 {
    unsafe {
        ioapic_select(base, register);
        ioapic_window(base).read_volatile()
    }
}

unsafe fn ioapic_write(base: u64, register: u32, value: u32) {
    unsafe {
        ioapic_select(base, register);
        ioapic_window(base).write_volatile(value);
    }
}

unsafe fn ioapic_write_redirection(base: u64, route: IoApicRoute) {
    let index = 0x10 + (route.line.0 as u32) * 2;
    let mut low = route.vector as u32;
    if matches!(route.delivery_mode, InterruptDeliveryMode::LowestPriority) {
        low |= 1 << 8;
    }
    if route.level_triggered {
        low |= 1 << 15;
    }
    if route.active_low {
        low |= 1 << 13;
    }
    if route.masked {
        low |= 1 << 16;
    }
    let high = (route.destination_apic_id as u32) << 24;
    unsafe {
        ioapic_write(base, index, low);
        ioapic_write(base, index + 1, high);
    }
}
