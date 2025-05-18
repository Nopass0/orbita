# Interrupts Implementation Guide

## Overview
The interrupt handling system manages hardware and software interrupts, including the Global Descriptor Table (GDT), Interrupt Descriptor Table (IDT), and interrupt handlers.

## Module Structure

### 1. Global Descriptor Table (gdt.rs)

```rust
//! Global Descriptor Table (GDT) implementation
//! Defines memory segments and privilege levels

use core::mem::size_of;
use x86_64::instructions::segmentation::{Segment, CS, SS};
use x86_64::instructions::tables::lgdt;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Size of the interrupt stack
pub const STACK_SIZE: usize = 16 * 1024; // 16KB

/// Number of interrupt stacks
pub const INTERRUPT_STACK_COUNT: usize = 7;

/// Static TSS instance
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// Static GDT instance
static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();

/// Kernel code segment selector
static mut KERNEL_CS: SegmentSelector = SegmentSelector::new(0, x86_64::PrivilegeLevel::Ring0);

/// Kernel data segment selector
static mut KERNEL_SS: SegmentSelector = SegmentSelector::new(0, x86_64::PrivilegeLevel::Ring0);

/// User code segment selector
static mut USER_CS: SegmentSelector = SegmentSelector::new(0, x86_64::PrivilegeLevel::Ring3);

/// User data segment selector
static mut USER_SS: SegmentSelector = SegmentSelector::new(0, x86_64::PrivilegeLevel::Ring3);

/// TSS selector
static mut TSS_SELECTOR: SegmentSelector = SegmentSelector::new(0, x86_64::PrivilegeLevel::Ring0);

/// Interrupt stacks
#[repr(align(4096))]
struct InterruptStack([u8; STACK_SIZE]);

static mut INTERRUPT_STACKS: [InterruptStack; INTERRUPT_STACK_COUNT] = 
    [InterruptStack([0; STACK_SIZE]); INTERRUPT_STACK_COUNT];

/// Initialize the GDT
pub fn init_gdt() {
    unsafe {
        // Create a new GDT
        let mut gdt = GlobalDescriptorTable::new();
        
        // Add null segment (required)
        gdt.add_entry(Descriptor::kernel_null());
        
        // Add kernel segments
        KERNEL_CS = gdt.add_entry(Descriptor::kernel_code_segment());
        KERNEL_SS = gdt.add_entry(Descriptor::kernel_data_segment());
        
        // Add user segments (for user-space processes)
        USER_CS = gdt.add_entry(Descriptor::user_code_segment());
        USER_SS = gdt.add_entry(Descriptor::user_data_segment());
        
        // Initialize TSS
        init_tss();
        
        // Add TSS to GDT
        TSS_SELECTOR = gdt.add_entry(Descriptor::tss_segment(&TSS));
        
        // Store the GDT
        GDT = gdt;
        
        // Load the GDT
        lgdt(&GDT_POINTER);
        
        // Reload segment registers
        CS::set_reg(KERNEL_CS);
        SS::set_reg(KERNEL_SS);
        
        // Load the TSS
        x86_64::instructions::tables::load_tss(TSS_SELECTOR);
    }
}

/// Initialize the Task State Segment (TSS)
unsafe fn init_tss() {
    // Set up interrupt stacks
    for (i, stack) in INTERRUPT_STACKS.iter_mut().enumerate() {
        let stack_end = stack.0.as_mut_ptr() as u64 + STACK_SIZE as u64;
        TSS.interrupt_stack_table[i] = VirtAddr::new(stack_end);
    }
    
    // Set up privilege stack for ring 0 (used when transitioning from user to kernel)
    static mut PRIVILEGE_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
    let privilege_stack_end = PRIVILEGE_STACK.as_mut_ptr() as u64 + STACK_SIZE as u64;
    TSS.privilege_stack_table[0] = VirtAddr::new(privilege_stack_end);
}

/// GDT pointer structure
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Static GDT pointer
static mut GDT_POINTER: GdtPointer = GdtPointer {
    limit: 0,
    base: 0,
};

/// Update the GDT pointer
unsafe fn update_gdt_pointer() {
    GDT_POINTER = GdtPointer {
        limit: (size_of::<GlobalDescriptorTable>() - 1) as u16,
        base: &GDT as *const _ as u64,
    };
}

/// Get kernel segment selectors
pub fn kernel_selectors() -> (SegmentSelector, SegmentSelector) {
    unsafe { (KERNEL_CS, KERNEL_SS) }
}

/// Get user segment selectors
pub fn user_selectors() -> (SegmentSelector, SegmentSelector) {
    unsafe { (USER_CS, USER_SS) }
}

/// Get TSS selector
pub fn tss_selector() -> SegmentSelector {
    unsafe { TSS_SELECTOR }
}
```

### 2. Interrupt Descriptor Table (interrupts.rs)

```rust
//! Interrupt handling and IDT implementation

use core::arch::asm;
use pic8259::ChainedPics;
use spin::{Lazy, Mutex};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

/// PIC controller offsets
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Interrupt vectors
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Cascade,
    COM2,
    COM1,
    LPT2,
    FloppyDisk,
    LPT1,
    RealTimeClock,
    Free1,
    Free2,
    Free3,
    Mouse,
    FPU,
    PrimaryATA,
    SecondaryATA,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
    
    fn as_usize(self) -> usize {
        self as usize
    }
}

/// Chained PIC controller
pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe {
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)
});

/// Static IDT instance
static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    
    // CPU exceptions
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.debug.set_handler_fn(debug_handler);
    idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.overflow.set_handler_fn(overflow_handler);
    idt.bound_range_exceeded.set_handler_fn(bound_range_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.device_not_available.set_handler_fn(device_not_available_handler);
    idt.double_fault.set_handler_fn(double_fault_handler);
    idt.invalid_tss.set_handler_fn(invalid_tss_handler);
    idt.segment_not_present.set_handler_fn(segment_not_present_handler);
    idt.stack_segment_fault.set_handler_fn(stack_segment_handler);
    idt.general_protection_fault.set_handler_fn(general_protection_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.x87_floating_point.set_handler_fn(x87_floating_point_handler);
    idt.alignment_check.set_handler_fn(alignment_check_handler);
    idt.machine_check.set_handler_fn(machine_check_handler);
    idt.simd_floating_point.set_handler_fn(simd_floating_point_handler);
    idt.virtualization.set_handler_fn(virtualization_handler);
    
    // Hardware interrupts
    idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
    idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
    idt[InterruptIndex::Mouse.as_usize()].set_handler_fn(mouse_interrupt_handler);
    
    // Set up double fault handler with its own stack
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
    }
    
    idt
});

/// Initialize the IDT
pub fn init_idt() {
    IDT.load();
}

/// Enable interrupts
pub fn enable() {
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
}

/// Disable interrupts
pub fn disable() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

/// Check if interrupts are enabled
pub fn are_enabled() -> bool {
    let flags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) flags, options(nomem));
    }
    flags & (1 << 9) != 0
}

/// Execute a closure with interrupts disabled
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let enabled = are_enabled();
    if enabled {
        disable();
    }
    let result = f();
    if enabled {
        enable();
    }
    result
}

// Exception handlers

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn debug_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: DEBUG\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn nmi_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: NMI\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: OVERFLOW\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn bound_range_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: BOUND RANGE EXCEEDED\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: DEVICE NOT AVAILABLE\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn invalid_tss_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!("EXCEPTION: INVALID TSS\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!("EXCEPTION: SEGMENT NOT PRESENT\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn stack_segment_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!("EXCEPTION: STACK SEGMENT FAULT\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn general_protection_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!("EXCEPTION: GENERAL PROTECTION FAULT\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    
    let accessed_address = Cr2::read();
    
    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", accessed_address);
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    
    // Handle the page fault (e.g., allocate a new page)
    // For now, we'll panic
    panic!("Page fault at {:?}", accessed_address);
}

extern "x86-interrupt" fn x87_floating_point_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: x87 FLOATING POINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!("EXCEPTION: ALIGNMENT CHECK\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    panic!("EXCEPTION: MACHINE CHECK\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn simd_floating_point_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: SIMD FLOATING POINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: VIRTUALIZATION\n{:#?}", stack_frame);
}

// Hardware interrupt handlers

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // Handle timer tick
    crate::timer::tick();
    
    // Send EOI
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    // Process the scancode
    crate::keyboard::process_scancode(scancode);
    
    // Send EOI
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    let mut port = Port::new(0x60);
    let packet: u8 = unsafe { port.read() };
    
    // Process mouse packet
    crate::mouse::process_packet(packet);
    
    // Send EOI
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}

/// Initialize the interrupt system
pub fn init() {
    init_gdt();
    init_idt();
    unsafe { PICS.lock().initialize() };
    enable();
}
```

## Usage Examples

### Basic Interrupt Setup

```rust
use orbita_os::interrupts;

// Initialize the interrupt system
interrupts::init();

// Interrupts are now enabled and handlers are installed
```

### Custom Interrupt Handler

```rust
use x86_64::structures::idt::InterruptStackFrame;

// Define a custom interrupt handler
extern "x86-interrupt" fn custom_handler(stack_frame: InterruptStackFrame) {
    println!("Custom interrupt triggered!");
    
    // Send EOI if it's a hardware interrupt
    unsafe {
        interrupts::PICS.lock().notify_end_of_interrupt(interrupt_number);
    }
}

// Register the handler (during IDT initialization)
idt[custom_interrupt_vector].set_handler_fn(custom_handler);
```

### Interrupt-Safe Critical Sections

```rust
use orbita_os::interrupts::without_interrupts;

// Execute code with interrupts disabled
let result = without_interrupts(|| {
    // Critical section code here
    // Interrupts are automatically disabled and restored
    perform_critical_operation()
});
```

## Common Errors and Solutions

### 1. Double Fault

**Error**: System crashes with double fault
**Solution**: 
- Ensure double fault handler has its own stack
- Check for stack overflow in interrupt handlers
- Verify GDT and TSS are properly initialized

### 2. Page Fault in Interrupt Handler

**Error**: Page fault occurs during interrupt handling
**Solution**: 
- Ensure all interrupt handler memory is mapped
- Use separate interrupt stacks
- Avoid allocating memory in interrupt handlers

### 3. Interrupt Storm

**Error**: Continuous interrupts preventing normal execution
**Solution**: 
- Always send EOI after handling hardware interrupts
- Mask problematic interrupt sources
- Implement rate limiting for frequent interrupts

### 4. Lost Interrupts

**Error**: Interrupts not being delivered
**Solution**: 
- Verify PIC initialization
- Check interrupt flag in EFLAGS
- Ensure IDT entries are properly set

## Module Dependencies

1. **Hardware Dependencies**:
   - PIC 8259 controller
   - x86_64 CPU features
   - Local APIC (for advanced features)

2. **Internal Dependencies**:
   - `gdt`: Global Descriptor Table
   - `memory`: Stack allocation
   - `timer`: Timer tick handling
   - `keyboard`: Keyboard input processing

3. **Used By**:
   - `scheduler`: Preemptive multitasking
   - `syscall`: System call interface
   - `drivers`: Device interrupt handling
   - `panic`: Exception handling

## Performance Considerations

1. **Interrupt Latency**:
   - Keep interrupt handlers short
   - Defer heavy processing to bottom halves
   - Use interrupt coalescing where appropriate

2. **Stack Usage**:
   - Use minimal stack in interrupt handlers
   - Consider separate interrupt stacks
   - Monitor stack usage to prevent overflow

3. **Context Switching**:
   - Minimize register saves/restores
   - Use fast interrupt return when possible
   - Consider lazy FPU context switching

## Security Considerations

1. **Privilege Separation**:
   - Ensure proper ring transitions
   - Validate interrupt sources
   - Prevent interrupt injection attacks

2. **Stack Protection**:
   - Use guard pages around interrupt stacks
   - Implement stack canaries
   - Separate kernel and user stacks

3. **Interrupt Masking**:
   - Carefully manage interrupt enable/disable
   - Prevent denial of service via interrupts
   - Implement interrupt priorities

## Advanced Features

### 1. APIC Support

```rust
/// Initialize Local APIC
pub fn init_apic() {
    // Detect APIC presence
    let cpuid = raw_cpuid::CpuId::new();
    if let Some(features) = cpuid.get_feature_info() {
        if features.has_apic() {
            // Initialize Local APIC
            unsafe {
                let apic_base = rdmsr(IA32_APIC_BASE);
                // Enable APIC and set base address
                wrmsr(IA32_APIC_BASE, apic_base | APIC_ENABLE);
            }
        }
    }
}
```

### 2. MSI Support

```rust
/// Configure Message Signaled Interrupts
pub fn configure_msi(device: &PciDevice, vector: u8) {
    // Read MSI capability
    let msi_cap = device.find_capability(MSI_CAP_ID);
    
    // Configure MSI address and data
    let address = MSI_ADDRESS_BASE | (cpu_id << 12);
    let data = vector;
    
    device.write_msi_address(address);
    device.write_msi_data(data);
    device.enable_msi();
}
```

### 3. Interrupt Priorities

```rust
/// Set interrupt priority
pub fn set_interrupt_priority(vector: u8, priority: u8) {
    // For APIC-based priority
    unsafe {
        let tpr = priority << 4;
        apic_write(APIC_TPR, tpr as u32);
    }
}
```

## Future Improvements

1. **IOAPIC Support**:
   - Implement I/O APIC for better interrupt routing
   - Support multiple CPUs
   - Dynamic interrupt balancing

2. **Nested Interrupts**:
   - Support interrupt nesting
   - Implement priority-based preemption
   - Add interrupt threading

3. **Power Management**:
   - Implement interrupt-based CPU idle
   - Support wake-on-interrupt
   - Dynamic interrupt coalescing