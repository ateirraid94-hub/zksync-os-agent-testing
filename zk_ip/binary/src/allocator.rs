use core::alloc::AllocError;
use core::alloc::Allocator;
use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::cmp::Ordering;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::ptr::null_mut;
use core::ptr::NonNull;
use talc::*;

unsafe impl GlobalAlloc for OptionalGlobalAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        BootloaderAllocator::default()
            .allocate(layout)
            .expect("Global allocactor: alloc")
            .as_mut_ptr()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        BootloaderAllocator::default().deallocate(
            core::ptr::NonNull::new(ptr).expect("Global allocator: dealloc"),
            layout,
        );
    }
}

pub type BootloaderAllocator = ProxyAllocator;

// TODO: disable global alloc once dependencies are fixed
pub struct OptionalGlobalAllocator;

#[derive(Clone, Copy, Debug, Default)]
pub struct ProxyAllocator;

// impl ZSTAllocator for ProxyAllocator {}

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

static mut USED_ALLOCATOR: MaybeUninit<TalcWrapper> = MaybeUninit::uninit();

#[inline(never)]
/// # Safety
/// `heap_start` must be less than or equal to heap_end
pub unsafe fn init_allocator(heap_start: *mut usize, heap_end: *mut usize) {
    #[allow(static_mut_refs)]
    unsafe {
        create_talc_allocator_wrapper(USED_ALLOCATOR.as_mut_ptr(), heap_start, heap_end);
    }
}

pub fn is_aligned_to(ptr: *mut u8, align: usize) -> bool {
    (ptr as usize).trailing_zeros() >= align.trailing_zeros()
}

const RELEASE_LOCK_ON_REALLOC_LIMIT: usize = 0x10000;

pub struct TalcWrapper(pub(crate) UnsafeCell<Talc<ClaimOnOom>>);

impl TalcWrapper {
    pub fn new(inner: Talc<ClaimOnOom>) -> Self {
        Self(UnsafeCell::new(inner))
    }

    #[allow(clippy::mut_from_ref)]
    unsafe fn quasi_lock(&self) -> &mut Talc<ClaimOnOom> {
        // This allocator is only intended to be run on single-threaded system,
        // so we only need to prevent compiler aliasing optimizations
        self.0.as_mut_unchecked()
    }
}

unsafe impl GlobalAlloc for TalcWrapper {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.quasi_lock()
            .malloc(layout)
            .map_or(null_mut(), |nn| nn.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.quasi_lock().free(NonNull::new_unchecked(ptr), layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        let nn_ptr = NonNull::new_unchecked(ptr);

        match new_size.cmp(&old_layout.size()) {
            Ordering::Greater => {
                // first try to grow in-place before manually re-allocating

                if let Ok(nn) = self
                    .quasi_lock()
                    .grow_in_place(nn_ptr, old_layout, new_size)
                {
                    return nn.as_ptr();
                }

                // grow in-place failed, reallocate manually

                let new_layout = Layout::from_size_align_unchecked(new_size, old_layout.align());

                let mut lock = self.quasi_lock();
                let allocation = match lock.malloc(new_layout) {
                    Ok(ptr) => ptr,
                    Err(_) => return null_mut(),
                };

                if old_layout.size() > RELEASE_LOCK_ON_REALLOC_LIMIT {
                    // TODO: check the following line makes sense
                    #[allow(dropping_references)]
                    drop(lock);
                    allocation
                        .as_ptr()
                        .copy_from_nonoverlapping(ptr, old_layout.size());
                    lock = self.quasi_lock();
                } else {
                    allocation
                        .as_ptr()
                        .copy_from_nonoverlapping(ptr, old_layout.size());
                }

                lock.free(nn_ptr, old_layout);
                allocation.as_ptr()
            }

            Ordering::Less => {
                self.quasi_lock()
                    .shrink(NonNull::new_unchecked(ptr), old_layout, new_size);
                ptr
            }

            Ordering::Equal => ptr,
        }
    }
}

unsafe impl core::alloc::Allocator for TalcWrapper {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, core::alloc::AllocError> {
        if layout.size() == 0 {
            return Ok(NonNull::slice_from_raw_parts(NonNull::dangling(), 0));
        }

        let ptr = unsafe { self.quasi_lock().malloc(layout).map_err(|_| AllocError) }?;
        debug_assert!(
            ptr.is_aligned_to(layout.align()),
            "allocated ptr {ptr:?} with non-matching layout {layout:?}"
        );

        Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            debug_assert!(
                ptr.is_aligned_to(layout.align()),
                "trying to deallocate ptr {ptr:?} with non-matching layout {layout:?}"
            );
            self.quasi_lock().free(ptr, layout);
        }
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, core::alloc::AllocError> {
        debug_assert!(new_layout.size() >= old_layout.size());

        if old_layout.size() == 0 {
            return self.allocate(new_layout);
        } else if is_aligned_to(ptr.as_ptr(), new_layout.align()) {
            // alignment is fine, try to allocate in-place
            if let Ok(nn) = self
                .quasi_lock()
                .grow_in_place(ptr, old_layout, new_layout.size())
            {
                return Ok(NonNull::slice_from_raw_parts(nn, new_layout.size()));
            }
        }

        // can't grow in place, reallocate manually

        let mut lock = self.quasi_lock();
        let allocation = lock.malloc(new_layout).map_err(|_| AllocError)?;

        if old_layout.size() > RELEASE_LOCK_ON_REALLOC_LIMIT {
            // TODO: check the following line makes sense
            #[allow(dropping_references)]
            drop(lock);
            allocation
                .as_ptr()
                .copy_from_nonoverlapping(ptr.as_ptr(), old_layout.size());
            lock = self.quasi_lock();
        } else {
            allocation
                .as_ptr()
                .copy_from_nonoverlapping(ptr.as_ptr(), old_layout.size());
        }

        lock.free(ptr, old_layout);

        Ok(NonNull::slice_from_raw_parts(allocation, new_layout.size()))
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, core::alloc::AllocError> {
        let res = self.grow(ptr, old_layout, new_layout);

        if let Ok(allocation) = res {
            allocation
                .as_ptr()
                .cast::<u8>()
                .add(old_layout.size())
                .write_bytes(0, new_layout.size() - old_layout.size());
        }

        res
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, core::alloc::AllocError> {
        debug_assert!(new_layout.size() <= old_layout.size());

        if new_layout.size() == 0 {
            if old_layout.size() > 0 {
                self.quasi_lock().free(ptr, old_layout);
            }

            return Ok(NonNull::slice_from_raw_parts(NonNull::dangling(), 0));
        }

        if !is_aligned_to(ptr.as_ptr(), new_layout.align()) {
            let mut lock = self.quasi_lock();
            let allocation = lock.malloc(new_layout).map_err(|_| AllocError)?;

            if new_layout.size() > RELEASE_LOCK_ON_REALLOC_LIMIT {
                // TODO: check the following line makes sense
                #[allow(dropping_references)]
                drop(lock);
                allocation
                    .as_ptr()
                    .copy_from_nonoverlapping(ptr.as_ptr(), new_layout.size());
                lock = self.quasi_lock();
            } else {
                allocation
                    .as_ptr()
                    .copy_from_nonoverlapping(ptr.as_ptr(), new_layout.size());
            }

            lock.free(ptr, old_layout);
            return Ok(NonNull::slice_from_raw_parts(allocation, new_layout.size()));
        }

        self.quasi_lock().shrink(ptr, old_layout, new_layout.size());

        Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
    }
}
///
/// TODO: add doc
///
/// # Safety
///
pub const unsafe fn create_uninit_talc_allocator() -> Talc<ClaimOnOom> {
    let mode = ClaimOnOom::new(Span::from_base_size(core::ptr::null_mut(), 0));

    Talc::new(mode)
}

///
/// # Safety
/// `upper_bound` > `lower_bound`
/// `dst` is aligned
///
pub unsafe fn create_talc_allocator(
    dst: *mut Talc<ClaimOnOom>,
    lower_bound: *mut usize,
    upper_bound: *mut usize,
) {
    let base = lower_bound.cast();
    let size = upper_bound.cast::<u8>().offset_from_unsigned(base);
    let span = Span::from_base_size(base, size);
    let mode = ClaimOnOom::new(Span::empty());
    *dst = Talc::new(mode);
    dst.as_mut()
        .unwrap_unchecked()
        .claim(span)
        .expect("must claim initial span");
}

///
/// # Safety
/// `upper_bound` > `lower_bound`
/// `dst` is aligned
///
pub unsafe fn create_talc_allocator_wrapper(
    dst: *mut TalcWrapper,
    lower_bound: *mut usize,
    upper_bound: *mut usize,
) {
    let unsafe_cell_addr = addr_of_mut!((*dst).0);
    // UnsafeCell is repr(transparent), so we can just cast a pointer
    create_talc_allocator(unsafe_cell_addr.cast(), lower_bound, upper_bound);
}
