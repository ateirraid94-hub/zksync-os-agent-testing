#![allow(incomplete_features)]
#![feature(allocator_api)]
#![feature(generic_const_exprs)]
#![feature(pointer_is_aligned_to)]
#![feature(unsafe_cell_access)]
#![feature(slice_ptr_get)]
#![feature(str_from_raw_parts)]
#![no_main]

pub mod allocator;
pub mod glue;
pub mod logger;
pub mod quasi_uart;

use riscv_common::csr_read_word;
use riscv_common::zksync_os_finish_success;

#[global_allocator]
static GLOBAL_ALLOC: allocator::OptionalGlobalAllocator = allocator::OptionalGlobalAllocator;

use crypto::bigint_delegation::u256::{self, U256};

#[inline(always)]
fn csr_trigger_delegation(
    states_ptr: *mut u32,
    input_ptr: *const u32,
    round_mask: u32,
    control_mask: u32,
) {
    unsafe {
        core::arch::asm!(
            "csrrw x0, 0x7c7, x0",
            in("x10") states_ptr.addr(),
            in("x11") input_ptr.addr(),
            in("x12") round_mask,
            in("x13") control_mask,
            options(nostack, preserves_flags)
        )
    }
}

// We have to be sure that the memory that we pass to the delegation is properly aligned.
#[repr(align(65536))]
struct Aligner;

pub const CONFIGURED_IV: [u32; 8] = [
    0x6A09E667 ^ 0x01010000 ^ 32,
    0xBB67AE85,
    0x3C6EF372,
    0xA54FF53A,
    0x510E527F,
    0x9B05688C,
    0x1F83D9AB,
    0x5BE0CD19,
];

// Blake magic.
pub const EXTENDED_IV: [u32; 16] = [
    0x6A09E667 ^ 0x01010000 ^ 32,
    0xBB67AE85,
    0x3C6EF372,
    0xA54FF53A,
    0x510E527F,
    0x9B05688C,
    0x1F83D9AB,
    0x5BE0CD19,
    0x6A09E667,
    0xBB67AE85,
    0x3C6EF372,
    0xA54FF53A,
    0x510E527F,
    0x9B05688C,
    0x1F83D9AB,
    0x5BE0CD19,
];

#[repr(C)]
struct BlakeState {
    pub _aligner: Aligner,
    pub state: [u32; 8],
    pub ext_state: [u32; 16],
    pub input_buffer: [u32; 16],
    pub round_bitmask: u32,
    pub t: u32, // we limit ourselves to <4Gb inputs
}

#[repr(C)]
#[derive(Default)]
struct BalanceDiff {
    asset_id: H256,
    sign: u32,
    prev_balance: U256,
    amount: U256,
    index: u32,
    // path: Vec<H256>
}

type H256 = [u32; 8];

fn blake_hash_parts(left: H256, right: H256) -> H256 {
    let mut state = BlakeState {
        _aligner: Aligner,
        // The order here is extremely important - as it has to match
        // the expected 'ABI' of the delegation circuit.
        // When we later call the csr_trigger_delegation, it will look at all the fields
        // below.
        state: CONFIGURED_IV,
        ext_state: EXTENDED_IV,
        input_buffer: [0u32; 16],
        round_bitmask: 0,
        t: 0,
    };

    // We are hashing 64 bytes
    state.t = 64;

    // our data - no alignment requirements
    let mut input_buffer = [0u32; 16];
    input_buffer[..8].copy_from_slice(&left);
    input_buffer[8..].copy_from_slice(&right);

    const NORMAL_MODE_FIRST_ROUNDS_CONTROL_REGISTER: u32 = 0b000;
    const NORMAL_MODE_LAST_ROUND_CONTROL_REGISTER: u32 = 0b001;

    // This is some Blake initialization magic.
    state.ext_state[12] = state.t ^ EXTENDED_IV[12];
    state.ext_state[14] = 0xffffffff ^ EXTENDED_IV[14];

    // Now we have to call the 'precompile' - blake requires us to actually call it 10 times.
    let mut round_bitmask = 1;
    for _round_idx in 0..9 {
        // We are passing the pointer to the state, but the code inside is actually reading
        // other fields from the BlakeState too (including input_buffer and round bitmask).
        // That's why we're in the 'unsafe' block.
        csr_trigger_delegation(
            ((&mut state) as *mut BlakeState).cast::<u32>(),
            input_buffer.as_ptr(),
            round_bitmask,
            NORMAL_MODE_FIRST_ROUNDS_CONTROL_REGISTER,
        );
        // Every time, we're pushing the bitmask, that is used internally to figure out which round it is.
        round_bitmask <<= 1;
    }
    // final one with final xor
    csr_trigger_delegation(
        ((&mut state) as *mut BlakeState).cast::<u32>(),
        input_buffer.as_ptr(),
        round_bitmask,
        NORMAL_MODE_LAST_ROUND_CONTROL_REGISTER,
    );

    state.state
}


fn read_h256() -> H256 {
    core::array::from_fn(|i| csr_read_word())
}

fn read_u256() -> (U256, [u32; 8]) {
    let mut bytes = [0u8; 32];
    let mut words = [0u32; 8];
    for j in 0..8 {
        words[j] = csr_read_word();
        bytes[j * 4..(j + 1) * 4].copy_from_slice(&words[j].to_be_bytes());
    }
    (u256::from_bytes_unchecked(&bytes), words)
}

unsafe fn workload() -> ! {
    crate::allocator::init_allocator(
        riscv_common::boot_sequence::heap_start(),
        riscv_common::boot_sequence::heap_end(),
    );

    let prev_root = read_h256();
    let tree_height = csr_read_word();
    let n = csr_read_word();

    let mut some_root = H256::default();

    let mut diffs = Vec::with_capacity(n as usize);

    for _i in 0..n {
        let sign = csr_read_word();
        let (amount, amount_raw) = read_u256();
        let asset_id = read_h256();
        let (prev_balance, _) = read_u256();

        if sign == 0 {
            assert!(amount.lt(&prev_balance));
        }

        let index = csr_read_word();
        let mut parity = index;
        let mut hash = blake_hash_parts(asset_id, amount_raw);
        for _j in 0..tree_height {
            let sibling = read_h256();
            if parity % 2 == 0 {
                hash = blake_hash_parts(hash, sibling);
            } else {
                hash = blake_hash_parts(sibling, hash);
            }
            parity >>= 1;
        }

        some_root = hash;

        // TODO compare hash with prev_root
        // assert!(hash.eq(&prev_root));

        diffs.push(BalanceDiff {
            sign,
            amount,
            asset_id,
            prev_balance,
            index
        });
    }

    zksync_os_finish_success(&some_root);
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
