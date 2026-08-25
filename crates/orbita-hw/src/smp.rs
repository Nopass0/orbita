use core::arch::x86_64::__cpuid;

#[derive(Debug, Copy, Clone)]
pub struct SmpInfo {
    pub logical_cpus: u8,
    pub initial_apic_id: u8,
    pub hyperthreading: bool,
}

pub fn probe() -> SmpInfo {
    let cpuid = __cpuid(1);
    SmpInfo {
        logical_cpus: ((cpuid.ebx >> 16) & 0xFF) as u8,
        initial_apic_id: ((cpuid.ebx >> 24) & 0xFF) as u8,
        hyperthreading: (cpuid.edx & (1 << 28)) != 0,
    }
}
