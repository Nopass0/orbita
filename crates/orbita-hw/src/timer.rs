use crate::LocalApicInfo;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ClockSource {
    Tsc,
    Apic,
    HpET,
    Pit,
    Unknown,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct TimerTick(pub u64);

#[derive(Debug, Copy, Clone)]
pub struct TimerTopology {
    pub source: ClockSource,
    pub tick_hz: u64,
}

#[derive(Debug, Copy, Clone)]
pub struct TimerPlan {
    pub quantum_hz: u32,
    pub source: &'static str,
    pub preemptive_ready: bool,
}

#[derive(Debug, Copy, Clone)]
pub struct LapicTimerState {
    pub configured: bool,
    pub vector: u8,
    pub initial_count: u32,
    pub divide: u32,
    pub masked: bool,
}

pub fn bootstrap_plan() -> TimerPlan {
    TimerPlan {
        quantum_hz: 1000,
        source: "pit->apic-transition",
        preemptive_ready: false,
    }
}

pub fn prepare_lapic_timer(apic: &LocalApicInfo, vector: u8) -> LapicTimerState {
    let state = LapicTimerState {
        configured: apic.present && apic.enabled && apic.physical_base != 0,
        vector,
        initial_count: 100_000,
        divide: 0b1011,
        masked: true,
    };

    if state.configured {
        unsafe {
            write_local_apic(apic.physical_base, 0x3E0, state.divide);
            write_local_apic(apic.physical_base, 0x320, (1 << 16) | vector as u32);
            write_local_apic(apic.physical_base, 0x380, state.initial_count);
        }
    }

    state
}

unsafe fn write_local_apic(base: u64, offset: u32, value: u32) {
    let register = (base + offset as u64) as *mut u32;
    unsafe {
        register.write_volatile(value);
    }
}
