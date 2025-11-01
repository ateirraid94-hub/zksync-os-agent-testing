
use core::mem::MaybeUninit;
use core::alloc::Allocator;
extern crate alloc;

mod talc;

#[derive(Clone, Copy, Debug, Default)]
pub struct ProxyAllocator;

unsafe impl Allocator for ProxyAllocator {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        #[allow(static_mut_refs)]
        unsafe {
            USED_ALLOCATOR.assume_init_ref().allocate(layout)
        }
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
        #[allow(static_mut_refs)]
        unsafe {
            USED_ALLOCATOR.assume_init_ref().deallocate(ptr, layout)
        }
    }

    unsafe fn grow(
        &self,
        _ptr: core::ptr::NonNull<u8>,
        _old_layout: core::alloc::Layout,
        _new_layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        panic!("grow is not allowed");
        // Commented out to avoid warning:
        // #[allow(static_mut_refs)]
        // unsafe {
        //     USED_ALLOCATOR
        //         .assume_init_ref()
        //         .grow(ptr, old_layout, new_layout)
        // }
    }

    unsafe fn shrink(
        &self,
        _ptr: core::ptr::NonNull<u8>,
        _old_layout: core::alloc::Layout,
        _new_layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        panic!("shrink is not allowed");
        // Commented out to avoid warning:
        // unsafe {
        //     USED_ALLOCATOR
        //         .assume_init_ref()
        //         .shrink(ptr, old_layout, new_layout)
        // }
    }
}


static mut USED_ALLOCATOR: MaybeUninit<talc::TalcWrapper> = MaybeUninit::uninit();

#[inline(never)]
/// # Safety
/// `heap_start` must be less than or equal to heap_end
pub unsafe fn init_allocator(heap_start: *mut usize, heap_end: *mut usize) {
    #[allow(static_mut_refs)]
    unsafe {
        talc::create_talc_allocator_wrapper(
            USED_ALLOCATOR.as_mut_ptr(),
            heap_start,
            heap_end,
        );
    }
}

// we can not use generic allocator below due to constraints cycles (even though it's not true),
// so we have to typedef

pub type CustomAllocator = ProxyAllocator;

use alloc::alloc::{GlobalAlloc, Layout};

#[derive(Clone)]
pub struct OptionalGlobalAllocator;

unsafe impl GlobalAlloc for OptionalGlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CustomAllocator::default()
            .allocate(layout)
            .expect("Global allocactor: alloc")
            .as_mut_ptr()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CustomAllocator::default().deallocate(
            core::ptr::NonNull::new(ptr).expect("Global allocator: dealloc"),
            layout,
        );
    }
}

#[global_allocator]
static GLOBAL_ALLOC: OptionalGlobalAllocator = OptionalGlobalAllocator;