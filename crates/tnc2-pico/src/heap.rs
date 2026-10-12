//! The heap. tnc2-core allocates the two EPROM halves and the RAM (96 KB,
//! once per board) and its frame queues.

use core::mem::MaybeUninit;
use embedded_alloc::LlffHeap;

#[global_allocator]
static HEAP: LlffHeap = LlffHeap::empty();

const HEAP_SIZE: usize = 160 * 1024;

/// Set up the heap. Call once, before anything allocates.
pub fn init() {
    static mut MEMORY: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    // SAFETY: called once at start up, before any allocation.
    unsafe { HEAP.init(&raw mut MEMORY as usize, HEAP_SIZE) }
}
