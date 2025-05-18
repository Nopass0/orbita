# Memory Management Implementation Guide

## Overview
The memory management module is responsible for managing physical and virtual memory in Orbita OS. It includes a physical frame allocator, heap allocator, and virtual memory management.

## Module Structure

### 1. Physical Frame Allocator (memory.rs)

```rust
//! Physical memory frame allocator
//! Manages allocation of 4KB physical memory frames

use core::mem;
use spin::Mutex;

/// Size of a memory frame (4KB)
pub const FRAME_SIZE: usize = 4096;

/// Physical address type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    /// Create a new physical address
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }
    
    /// Get the raw address value
    pub const fn as_u64(self) -> u64 {
        self.0
    }
    
    /// Align the address down to the nearest frame boundary
    pub const fn align_down(self) -> Self {
        Self(self.0 & !(FRAME_SIZE as u64 - 1))
    }
    
    /// Align the address up to the nearest frame boundary
    pub const fn align_up(self) -> Self {
        let aligned = (self.0 + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1);
        Self(aligned)
    }
    
    /// Convert to a frame number
    pub const fn as_frame_number(self) -> u64 {
        self.0 / FRAME_SIZE as u64
    }
}

/// A physical memory frame
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub start_address: PhysAddr,
}

impl Frame {
    /// Check if the frame contains a given address
    pub fn contains(&self, addr: PhysAddr) -> bool {
        addr >= self.start_address && addr < self.start_address + FRAME_SIZE
    }
    
    /// Get the frame containing a given address
    pub fn containing_address(addr: PhysAddr) -> Self {
        Self {
            start_address: addr.align_down(),
        }
    }
}

/// Bitmap-based frame allocator
pub struct FrameAllocator {
    bitmap: Vec<u8>,
    next_free: usize,
    total_frames: usize,
    used_frames: usize,
}

impl FrameAllocator {
    /// Create a new frame allocator
    /// 
    /// # Arguments
    /// * `memory_size` - Total available physical memory in bytes
    pub fn new(memory_size: usize) -> Self {
        let total_frames = memory_size / FRAME_SIZE;
        let bitmap_size = (total_frames + 7) / 8; // Round up to nearest byte
        
        Self {
            bitmap: vec![0; bitmap_size],
            next_free: 0,
            total_frames,
            used_frames: 0,
        }
    }
    
    /// Mark a frame as used
    fn mark_frame_used(&mut self, frame_num: usize) {
        let byte_index = frame_num / 8;
        let bit_index = frame_num % 8;
        self.bitmap[byte_index] |= 1 << bit_index;
    }
    
    /// Mark a frame as free
    fn mark_frame_free(&mut self, frame_num: usize) {
        let byte_index = frame_num / 8;
        let bit_index = frame_num % 8;
        self.bitmap[byte_index] &= !(1 << bit_index);
    }
    
    /// Check if a frame is used
    fn is_frame_used(&self, frame_num: usize) -> bool {
        let byte_index = frame_num / 8;
        let bit_index = frame_num % 8;
        (self.bitmap[byte_index] & (1 << bit_index)) != 0
    }
    
    /// Allocate a physical frame
    pub fn allocate_frame(&mut self) -> Option<Frame> {
        // Start searching from the last allocated position
        for frame_num in self.next_free..self.total_frames {
            if !self.is_frame_used(frame_num) {
                self.mark_frame_used(frame_num);
                self.used_frames += 1;
                self.next_free = frame_num + 1;
                
                let start_address = PhysAddr::new(frame_num as u64 * FRAME_SIZE as u64);
                return Some(Frame { start_address });
            }
        }
        
        // If we didn't find anything, search from the beginning
        for frame_num in 0..self.next_free {
            if !self.is_frame_used(frame_num) {
                self.mark_frame_used(frame_num);
                self.used_frames += 1;
                self.next_free = frame_num + 1;
                
                let start_address = PhysAddr::new(frame_num as u64 * FRAME_SIZE as u64);
                return Some(Frame { start_address });
            }
        }
        
        None // Out of memory
    }
    
    /// Free a physical frame
    pub fn free_frame(&mut self, frame: Frame) {
        let frame_num = frame.start_address.as_frame_number() as usize;
        if self.is_frame_used(frame_num) {
            self.mark_frame_free(frame_num);
            self.used_frames -= 1;
            
            // Update next_free to potentially speed up next allocation
            if frame_num < self.next_free {
                self.next_free = frame_num;
            }
        }
    }
    
    /// Get memory statistics
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            total_frames: self.total_frames,
            used_frames: self.used_frames,
            free_frames: self.total_frames - self.used_frames,
        }
    }
}

#[derive(Debug)]
pub struct MemoryStats {
    pub total_frames: usize,
    pub used_frames: usize,
    pub free_frames: usize,
}

/// Global frame allocator instance
pub static FRAME_ALLOCATOR: Mutex<Option<FrameAllocator>> = Mutex::new(None);

/// Initialize the frame allocator
pub fn init_frame_allocator(memory_size: usize) {
    let mut allocator = FRAME_ALLOCATOR.lock();
    *allocator = Some(FrameAllocator::new(memory_size));
}

/// Allocate a frame from the global allocator
pub fn allocate_frame() -> Option<Frame> {
    FRAME_ALLOCATOR.lock().as_mut()?.allocate_frame()
}

/// Free a frame to the global allocator
pub fn free_frame(frame: Frame) {
    if let Some(allocator) = FRAME_ALLOCATOR.lock().as_mut() {
        allocator.free_frame(frame);
    }
}
```

### 2. Heap Allocator (allocator.rs)

```rust
//! Kernel heap allocator
//! Implements a linked list allocator for dynamic memory allocation

use core::alloc::{GlobalAlloc, Layout};
use core::mem;
use core::ptr;
use spin::Mutex;

/// Align the given address upwards to the given alignment
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// A free memory region in the heap
struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }
    
    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }
    
    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

/// Linked list allocator
pub struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    /// Create a new empty allocator
    pub const fn new() -> Self {
        Self {
            head: ListNode::new(0),
        }
    }
    
    /// Initialize the allocator with a heap region
    /// 
    /// # Safety
    /// The caller must ensure that the given heap region is valid and not used elsewhere
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.add_free_region(heap_start, heap_size);
    }
    
    /// Add a free region to the allocator
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        assert_eq!(align_up(addr, mem::align_of::<ListNode>()), addr);
        assert!(size >= mem::size_of::<ListNode>());
        
        let mut node = ListNode::new(size);
        node.next = self.head.next.take();
        let node_ptr = addr as *mut ListNode;
        node_ptr.write(node);
        self.head.next = Some(&mut *node_ptr);
    }
    
    /// Find a free region with the given size and alignment
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        let mut current = &mut self.head;
        
        while let Some(ref mut region) = current.next {
            let alloc_start = align_up(region.start_addr(), align);
            let alloc_end = alloc_start.checked_add(size)?;
            
            if alloc_end <= region.end_addr() {
                let excess_size = region.end_addr() - alloc_end;
                if excess_size > 0 && excess_size < mem::size_of::<ListNode>() {
                    // The remaining region is too small for a ListNode
                    continue;
                }
                
                // Found a suitable region
                return Some((current.next.take().unwrap(), alloc_start));
            } else {
                current = current.next.as_mut().unwrap();
            }
        }
        
        None
    }
    
    /// Allocate memory with the given layout
    pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let (size, align) = LinkedListAllocator::size_align(layout);
        
        if let Some((region, alloc_start)) = self.find_region(size, align) {
            let alloc_end = alloc_start + size;
            let excess_size = region.end_addr() - alloc_end;
            
            if excess_size >= mem::size_of::<ListNode>() {
                // Create a new node for the remaining memory
                unsafe {
                    self.add_free_region(alloc_end, excess_size);
                }
            }
            
            alloc_start as *mut u8
        } else {
            ptr::null_mut()
        }
    }
    
    /// Deallocate memory at the given pointer with the given layout
    pub fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        let (size, _) = LinkedListAllocator::size_align(layout);
        unsafe {
            self.add_free_region(ptr as usize, size);
        }
    }
    
    /// Adjust the given layout to include space for a ListNode if necessary
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("adjusting alignment failed")
            .pad_to_align();
        
        let size = layout.size().max(mem::size_of::<ListNode>());
        (size, layout.align())
    }
}

/// Global heap allocator
pub struct HeapAllocator {
    allocator: Mutex<LinkedListAllocator>,
}

impl HeapAllocator {
    /// Create a new heap allocator
    pub const fn new() -> Self {
        Self {
            allocator: Mutex::new(LinkedListAllocator::new()),
        }
    }
    
    /// Initialize the heap allocator
    /// 
    /// # Safety
    /// The caller must ensure that the given heap region is valid
    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        self.allocator.lock().init(heap_start, heap_size);
    }
}

unsafe impl GlobalAlloc for HeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocator.lock().allocate(layout)
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.allocator.lock().deallocate(ptr, layout)
    }
}

/// Global allocator instance
#[global_allocator]
static ALLOCATOR: HeapAllocator = HeapAllocator::new();

/// Initialize the kernel heap
/// 
/// # Arguments
/// * `heap_start` - Starting address of the heap
/// * `heap_size` - Size of the heap in bytes
/// 
/// # Safety
/// The caller must ensure that the heap memory region is valid and unused
pub unsafe fn init_heap(heap_start: usize, heap_size: usize) {
    ALLOCATOR.init(heap_start, heap_size);
}

/// Heap allocation statistics
pub struct HeapStats {
    pub allocated: usize,
    pub free: usize,
    pub total: usize,
}

/// Get current heap statistics
pub fn heap_stats() -> HeapStats {
    // Implementation would need to track allocations
    // This is a simplified version
    HeapStats {
        allocated: 0,
        free: 0,
        total: 0,
    }
}
```

## Usage Examples

### Frame Allocator Usage

```rust
use orbita_os::memory::{init_frame_allocator, allocate_frame, free_frame};

// Initialize the frame allocator with 1GB of memory
init_frame_allocator(1024 * 1024 * 1024);

// Allocate a frame
if let Some(frame) = allocate_frame() {
    println!("Allocated frame at address: {:?}", frame.start_address);
    
    // Use the frame...
    
    // Free the frame when done
    free_frame(frame);
}
```

### Heap Allocator Usage

```rust
use alloc::vec::Vec;
use alloc::boxed::Box;

// After heap initialization, you can use Rust's standard allocation types
let mut vec = Vec::new();
vec.push(42);

let boxed_value = Box::new(100);

// Memory is automatically freed when these go out of scope
```

## Common Errors and Solutions

### 1. Out of Memory

**Error**: Frame allocator returns `None` or heap allocator returns null pointer
**Solution**: 
- Check available memory with `stats()` method
- Free unused frames/heap memory
- Increase total memory if possible

### 2. Alignment Issues

**Error**: Misaligned address panic
**Solution**: 
- Always use provided alignment functions
- Ensure heap start address is properly aligned
- Use proper Layout alignment in allocations

### 3. Double Free

**Error**: Attempting to free already freed memory
**Solution**: 
- Track allocated memory properly
- Use RAII patterns (Box, Vec) when possible
- Implement reference counting if needed

### 4. Memory Leaks

**Error**: Memory usage continuously increases
**Solution**: 
- Always free allocated frames when done
- Use smart pointers for automatic deallocation
- Implement memory profiling

## Module Dependencies

1. **Core Dependencies**:
   - `spin`: Mutex for thread-safe access
   - `core::alloc`: GlobalAlloc trait
   - `core::mem`: Memory utilities

2. **Internal Dependencies**:
   - `interrupts`: Disable interrupts during critical sections
   - `paging`: Virtual memory management
   - `multiboot`: Get memory map information

3. **Used By**:
   - `process`: Process memory allocation
   - `drivers`: DMA buffer allocation
   - `filesystem`: Cache allocation
   - `network`: Packet buffer allocation

## Performance Considerations

1. **Frame Allocator**:
   - Bitmap-based for O(n) allocation in worst case
   - Maintains next_free hint for faster allocation
   - Consider buddy allocator for better performance

2. **Heap Allocator**:
   - Linked list has O(n) allocation time
   - Consider segregated lists for different size classes
   - Implement thread-local caching for better SMP performance

## Security Considerations

1. **Memory Isolation**:
   - Ensure frame allocator respects kernel/user boundaries
   - Implement guard pages between allocations
   - Zero memory before allocation to prevent data leaks

2. **Overflow Protection**:
   - Check for integer overflow in size calculations
   - Implement canaries for heap corruption detection
   - Use safe wrappers for all allocations

## Future Improvements

1. **Advanced Allocators**:
   - Implement buddy allocator for frames
   - Add slab allocator for kernel objects
   - Implement NUMA-aware allocation

2. **Memory Statistics**:
   - Track per-process memory usage
   - Implement memory pressure notifications
   - Add allocation profiling support

3. **Large Page Support**:
   - Add 2MB/1GB page support
   - Implement transparent huge pages
   - Add page merging for identical pages