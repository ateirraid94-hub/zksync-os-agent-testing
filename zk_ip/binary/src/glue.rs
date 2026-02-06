use std::{
    alloc::{Allocator, Layout},
    fmt::Write as _,
};

#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_halt() {
    #[cfg(target_arch = "riscv32")]
    {
        riscv_common::zksync_os_finish_error();
    }
    #[cfg(not(target_arch = "riscv32"))]
    {
        panic!("Must never be called outside of zkVM");
    }
}

#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_rand(_recv_buf: *mut u32, _words: usize) {
    // panic!("Randomness is not supported in Airbender");
}

///
/// # Safety
///
/// This is a platform function, we expect provided values to be correct,
/// since they are provided by the compiler.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe fn sys_panic(msg_ptr: *const u8, len: usize) -> ! {
    let msg = unsafe { core::str::from_raw_parts(msg_ptr, len) };
    crate::logger::LoggerTy::default()
        .write_str("PANIC: ")
        .unwrap();
    crate::logger::LoggerTy::default().write_str(msg).unwrap();

    sys_halt();
    unsafe { core::hint::unreachable_unchecked() }
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sys_log(_msg_ptr: *const u8, _len: usize) {
    // todo!()
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sys_read(_fd: u32, _recv_buf: *mut u8, _nrequested: usize) -> usize {
    0
    // todo!()
}

/// # Safety
///
/// This is a platform function, we expect provided values to be correct,
/// since they are provided by the compiler.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe fn sys_write(fd: u32, write_buf: *const u8, nbytes: usize) {
    let msg = unsafe { core::str::from_raw_parts(write_buf, nbytes) };
    match fd {
        1 => {
            crate::logger::LoggerTy::default().write_str(msg).unwrap();
        }
        2 => {
            crate::logger::LoggerTy::default().write_str(msg).unwrap();
        }
        _ => {
            // ignore other fds
        }
    }

    // todo!()
}

///
/// # Safety
///
/// This is a platform function, we expect provided values to be correct,
/// since they are provided by the compiler.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe fn sys_getenv(
    _recv_buf: *mut u32,
    _words: usize,
    varname: *const u8,
    varname_len: usize,
) -> usize {
    let varname = unsafe { core::str::from_raw_parts(varname, varname_len) };
    crate::logger::LoggerTy::default()
        .write_fmt(format_args!("env: {}", varname))
        .unwrap();

    // panic!("sys_getenv")
    // Do nothing -> empty value
    0
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sys_argc() -> usize {
    panic!("sys_argc")
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sys_argv(_out_words: *mut u32, _out_nwords: usize, _arg_index: usize) -> usize {
    panic!("sys_argv")
}

// Allocate memory from global HEAP.
const WORD_SIZE: usize = core::mem::size_of::<u32>();

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sys_alloc_words(nwords: usize) -> *mut u32 {
    std::alloc::Global
        .allocate(Layout::from_size_align(nwords * WORD_SIZE, WORD_SIZE).expect("Layout failed"))
        .expect("Allocation failed")
        .as_ptr() as *mut u32
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn sys_alloc_aligned(nwords: usize, align: usize) -> *mut u8 {
    std::alloc::Global
        .allocate(Layout::from_size_align(nwords * WORD_SIZE, align).expect("Layout failed"))
        .expect("Allocation failed")
        .as_ptr() as *mut u8
}

#[no_mangle]
unsafe extern "Rust" fn __getrandom_v03_custom(
    _dest: *mut u8,
    _len: usize,
) -> Result<(), getrandom::Error> {
    panic!("Getrandom called")
}
