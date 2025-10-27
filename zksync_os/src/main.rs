#![no_std]
#![feature(allocator_api)]
#![no_main]

pub mod helper_reg_utils;

#[cfg(not(feature = "no_exception_handling"))]
pub mod machine_trap;

#[cfg(feature = "print_debug_info")]
pub mod quasi_uart;

pub mod trap_frame;
pub mod utils;

use riscv_common::zksync_os_finish_success;

use self::trap_frame::MachineTrapFrame;

#[cfg(feature = "print_debug_info")]
#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
		use core::fmt::Write;
		let _ = write!(crate::quasi_uart::QuasiUART::new(), $($args)+);
	});
}

#[cfg(feature = "print_debug_info")]
#[macro_export]
macro_rules! println
{
	() => ({
		crate::print!("\r\n")
	});
	($fmt:expr) => ({
		crate::print!(concat!($fmt, "\r\n"))
	});
	($fmt:expr, $($args:tt)+) => ({
		crate::print!(concat!($fmt, "\r\n"), $($args)+)
	});
}

#[no_mangle]
extern "C" fn eh_personality() {}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    #[cfg(feature = "print_debug_info")]
    {
        print!("Aborting: ");
        if let Some(p) = _info.location() {
            println!("line {}, file {}", p.line(), p.file(),);

            if let Some(m) = _info.message().as_str() {
                println!("line {}, file {}: {}", p.line(), p.file(), m,);
            } else {
                println!(
                    "line {}, file {}, message:\n{}",
                    p.line(),
                    p.file(),
                    _info.message()
                );
                // println!("line {}, file {}", p.line(), p.file(),);
            }
        } else {
            println!("no information available.");
        }
    }

    riscv_common::rust_abort();
}

/// Uses CSR (control & status register) to communicate with outside oracle.
mod csr {
    use riscv_common::{csr_read_word, csr_write_word};

    #[derive(Clone, Copy, Debug)]
    pub struct CSRBasedNonDeterminismSource;

    impl proof_running_system::io_oracle::NonDeterminismCSRSourceImplementation
        for CSRBasedNonDeterminismSource
    {
        #[inline(always)]
        fn csr_read_impl() -> usize {
            const {
                assert!(core::mem::size_of::<usize>() == core::mem::size_of::<u32>());
            }
            csr_read_word() as usize
        }
        #[inline(always)]
        fn csr_write_impl(value: usize) {
            core::hint::black_box(csr_write_word(value))
        }
    }
}

pub use self::csr::CSRBasedNonDeterminismSource;

#[derive(Clone, Copy, Debug, Default)]
pub struct NullAllocator;

unsafe impl core::alloc::GlobalAlloc for NullAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        // panic!("use of global null allocator");
        core::hint::unreachable_unchecked()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // panic!("use of global null allocator");
        core::hint::unreachable_unchecked()
    }

    unsafe fn realloc(
        &self,
        _ptr: *mut u8,
        _layout: core::alloc::Layout,
        _new_size: usize,
    ) -> *mut u8 {
        // panic!("use of global null allocator");
        core::hint::unreachable_unchecked()
    }
}

// // This allocator actually allows link-time checks instead of unreachable panics
// struct FakeAllocator;

// unsafe impl core::alloc::GlobalAlloc for FakeAllocator {
//     unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
//         extern "Rust" {
//             fn fake_alloc_this_doesnt_exist(layout: core::alloc::Layout) -> *mut u8;
//         }
//         fake_alloc_this_doesnt_exist(layout)
//     }
//     unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
//         extern "Rust" {
//             fn fake_dealloc_this_doesnt_exist(ptr: *mut u8, layout: core::alloc::Layout);
//         }
//         fake_dealloc_this_doesnt_exist(ptr, layout)
//     }
// }

use proof_running_system::system::bootloader::OptionalGlobalAllocator;
#[global_allocator]
static GLOBAL_ALLOC: OptionalGlobalAllocator = OptionalGlobalAllocator;
// TODO: disable global alloc once dependencies are fixed
// static GLOBAL_ALLOCATOR_PLACEHOLDER: NullAllocator = NullAllocator;
// static GLOBAL_ALLOCATOR_PLACEHOLDER: FakeAllocator = FakeAllocator;

unsafe fn workload() -> ! {
    use proof_running_system::system::bootloader::init_allocator;
    init_allocator(riscv_common::boot_sequence::heap_start(), riscv_common::boot_sequence::heap_end());

    #[cfg(not(feature = "print_debug_info"))]
    type LoggerTy = proof_running_system::zk_ee::system::NullLogger;

    #[cfg(feature = "print_debug_info")]
    type LoggerTy = crate::quasi_uart::QuasiUART;

    use core::fmt::Write;
    let _ =
        LoggerTy::default().write_fmt(format_args!("Entry routine is done, moving into payload\n"));

    // When using blake circuits - make sure that they are initialized.
    // Otherwise, it will try accessing not-set memory.

    #[cfg(any(feature = "delegation", feature = "proving"))]
    crypto::init_lib();

    // and crunch
    let output = proof_running_system::system::bootloader::run_proving::<
        CSRBasedNonDeterminismSource,
        LoggerTy,
    >(riscv_common::boot_sequence::heap_start(), riscv_common::boot_sequence::heap_end());

    zksync_os_finish_success(&output);
}

#[inline(never)]
fn main() -> ! {
    riscv_common::boot_sequence::init();

    unsafe { workload() }
}

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

/// Exception (trap) handler in rust.
/// Called from the asm/asm.S
#[link_section = ".trap.rust"]
#[export_name = "_machine_start_trap_rust"]
pub extern "C" fn machine_start_trap_rust(trap_frame: *mut MachineTrapFrame) -> usize {
    #[cfg(feature = "no_exception_handling")]
    {
        unsafe { core::hint::unreachable_unchecked() }
    }

    #[cfg(not(feature = "no_exception_handling"))]
    {
        extern "C" {
            fn MachineExceptionHandler(trap_frame: &mut MachineTrapFrame) -> usize;
            // fn DefaultHandler();
        }

        unsafe {
            let cause = riscv::register::mcause::read();

            if cause.is_exception() {
                MachineExceptionHandler(&mut *trap_frame)
            } else {
                // DefaultHandler();
                riscv::register::mepc::read()
            }
        }
    }
}
