#![no_std]
#![allow(incomplete_features)]
#![feature(btreemap_alloc)]
#![feature(allocator_api)]
#![feature(generic_const_exprs)]
#![feature(slice_ptr_get)]
#![feature(pointer_is_aligned_to)]
#![feature(unsafe_cell_access)]

#![no_main]
#![no_builtins]

extern "C" {
    // Boundaries of the heap
    static mut _sheap: usize;
    static mut _eheap: usize;

    // Boundaries of the stack
    static mut _sstack: usize;
    static mut _estack: usize;
}

core::arch::global_asm!(include_str!("../../../utils/scripts/asm/asm_reduced.S"));

#[no_mangle]
extern "C" fn eh_personality() {}

#[link_section = ".init.rust"]
#[export_name = "_start_rust"]
unsafe extern "C" fn start_rust() -> ! {
    main()
}

#[export_name = "_setup_interrupts"]
pub unsafe fn custom_setup_interrupts() {
    extern "C" {
        fn _machine_start_trap();
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct MachineTrapFrame {
    pub registers: [u32; 32],
}

/// Exception (trap) handler in rust.
/// Called from the asm/asm.S
#[link_section = ".trap.rust"]
#[export_name = "_machine_start_trap_rust"]
pub extern "C" fn machine_start_trap_rust(_trap_frame: *mut MachineTrapFrame) -> usize {
    {
        unsafe { core::hint::unreachable_unchecked() }
    }
}

mod program;
mod custom_allocator;

use riscv_common::zksync_os_finish_success;

unsafe fn workload() -> ! {
    use core::ptr::addr_of_mut;
    let heap_start = addr_of_mut!(_sheap);
    let heap_end = addr_of_mut!(_eheap);

    custom_allocator::init_allocator(heap_start, heap_end);

    let result = program::program();
    zksync_os_finish_success(&[result, 0, 0, 0, 0, 0, 0, 0]);
}

#[inline(never)]
fn main() -> ! {
    unsafe { workload() }
}
