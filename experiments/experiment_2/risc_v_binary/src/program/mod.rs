mod storage_layer;
use storage_layer::StorageLayer;
use crate::custom_allocator::CustomAllocator;


pub unsafe fn program() -> u32 {
    let mut storage = StorageLayer::default();

    let a = storage.get(0);
    let b = storage.get(1);

    storage.set(2, a + b);

    storage.commit()
}
