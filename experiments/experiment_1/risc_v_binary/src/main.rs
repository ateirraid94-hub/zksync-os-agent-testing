#![no_std]
#![allow(incomplete_features)]
#![feature(allocator_api)]
#![feature(generic_const_exprs)]
#![no_main]
#![no_builtins]

use riscv_common::{csr_read_word, zksync_os_finish_success};

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

fn read_from_input() -> u32 {
    csr_read_word()
}

fn return_data(x0: u32, x1: u32) -> ! {
    zksync_os_finish_success(&[x0, x1, 0, 0, 0, 0, 0, 0]);
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

// ↑ Nothing to see here
// ↓ Actual interesting stuff

const MODULUS: u32 = 7919;

unsafe fn workload() -> ! {
    // Read the n number from the input.
    let n = read_from_input();
    let mut a = 0;
    let mut b = 1;
    for _i in 0..n {
        let c = (a + b) % MODULUS;
        a = b;
        b = c;
    }
    return_data(b, n);
}

#[inline(never)]
fn main() -> ! {
    unsafe { workload() }
}
