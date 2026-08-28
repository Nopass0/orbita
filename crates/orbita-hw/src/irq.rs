use orbita_arch_x86_64::{
    cpu,
    tables::{DescriptorTablePointer, load_idt},
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IrqEdge {
    Level,
    Edge,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IrqPolarity {
    ActiveHigh,
    ActiveLow,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InterruptLine(pub u8);

pub const IRQ_BASE_VECTOR: u8 = 32;
pub const TIMER_VECTOR: u8 = IRQ_BASE_VECTOR;
pub const KEYBOARD_VECTOR: u8 = IRQ_BASE_VECTOR + 1;
pub const SPURIOUS_VECTOR: u8 = 0xFF;
/// Double fault (#DF) — error-code CPU exception.
pub const DOUBLE_FAULT_VECTOR: u8 = 8;
/// General protection fault (#GP) — error-code CPU exception.
pub const GENERAL_PROTECTION_VECTOR: u8 = 13;
/// Page fault (#PF) — error-code CPU exception; faulting address in CR2.
pub const PAGE_FAULT_VECTOR: u8 = 14;

type IrqHandler = fn(u8);

#[derive(Debug, Copy, Clone)]
pub struct IdtInstallReport {
    pub vectors_installed: usize,
    pub timer_vector: u8,
    pub keyboard_vector: u8,
    pub spurious_vector: u8,
    /// CPU-fault vectors with diagnostic handlers (#DF/#GP/#PF).
    pub fault_vectors: &'static [u8],
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    fn set_handler(&mut self, handler: u64) {
        self.offset_low = handler as u16;
        self.selector = 0x08;
        self.ist = 0;
        self.attributes = 0x8E;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.reserved = 0;
    }
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];
static mut HANDLERS: [Option<IrqHandler>; 256] = [None; 256];

pub fn install_bootstrap_idt() -> IdtInstallReport {
    let irq_stub = cpu::irq_stub_addr();
    let timer_stub = cpu::timer_irq_stub_addr();
    let spurious_stub = cpu::spurious_irq_stub_addr();
    let fault_vectors: &[u8] = &[
        DOUBLE_FAULT_VECTOR,
        GENERAL_PROTECTION_VECTOR,
        PAGE_FAULT_VECTOR,
    ];

    unsafe {
        let idt_ptr = core::ptr::addr_of_mut!(IDT) as *mut IdtEntry;
        for index in 0..256 {
            (*idt_ptr.add(index)).set_handler(irq_stub);
        }

        (*idt_ptr.add(TIMER_VECTOR as usize)).set_handler(timer_stub);
        (*idt_ptr.add(KEYBOARD_VECTOR as usize)).set_handler(irq_stub);
        (*idt_ptr.add(SPURIOUS_VECTOR as usize)).set_handler(spurious_stub);
        // CPU exceptions print diagnostics (rip/CR2/error) instead of
        // iretq-looping into a silent triple fault.
        (*idt_ptr.add(DOUBLE_FAULT_VECTOR as usize)).set_handler(cpu::double_fault_stub_addr());
        (*idt_ptr.add(GENERAL_PROTECTION_VECTOR as usize))
            .set_handler(cpu::general_protection_stub_addr());
        (*idt_ptr.add(PAGE_FAULT_VECTOR as usize)).set_handler(cpu::page_fault_stub_addr());

        let pointer = DescriptorTablePointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
        };
        load_idt(&pointer);
    }

    IdtInstallReport {
        vectors_installed: 256,
        timer_vector: TIMER_VECTOR,
        keyboard_vector: KEYBOARD_VECTOR,
        spurious_vector: SPURIOUS_VECTOR,
        fault_vectors,
    }
}

pub fn register_handler(vector: u8, handler: IrqHandler) {
    unsafe {
        HANDLERS[vector as usize] = Some(handler);
    }
}

pub fn dispatch(vector: u8) -> bool {
    let handler = unsafe { HANDLERS[vector as usize] };
    if let Some(handler) = handler {
        handler(vector);
        true
    } else {
        false
    }
}
