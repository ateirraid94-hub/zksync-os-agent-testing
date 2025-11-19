use core::mem::MaybeUninit;
use oracle_provider::U32Memory;

pub fn read_memory_as_u8(memory: &dyn U32Memory, offset: u32, len: u32) -> Result<Vec<u8>, ()> {
    let (_, of) = offset.overflowing_add(len);
    if of == true {
        return Err(());
    }

    let mut offset = offset;
    let mut len = len;

    let mut result = Vec::with_capacity(len as usize);

    if offset % 4 != 0 {
        let max_take_bytes = 4 - offset;
        let take_bytes = std::cmp::min(max_take_bytes, len);
        let aligned = (offset >> 2) << 2;
        let value = memory.read_word(aligned);
        let value = value.to_le_bytes();
        result.extend_from_slice(&value[offset as usize % 4..][..take_bytes as usize]);
        offset += max_take_bytes;
        len -= take_bytes;
    }
    // then aligned w
    while len >= 4 {
        let value = memory.read_word(offset);
        let value = value.to_le_bytes();
        result.extend_from_slice(&value[..]);
        offset += 4;
        len -= 4;
    }
    // then tail
    if len != 0 {
        let value = memory.read_word(offset);
        let value = value.to_le_bytes();
        result.extend_from_slice(&value[..len as usize]);
        len = 0;
    }

    assert_eq!(len, 0);

    Ok(result)
}

pub fn read_memory_as_u64(
    memory: &dyn U32Memory,
    mut offset: u32,
    len_u64_words: u32,
) -> Result<Vec<u64>, ()> {
    let mut len_u32_words = len_u64_words * 2;

    let (_, of) = offset.overflowing_add(len_u32_words);
    if of == true {
        return Err(());
    }

    let mut result = Vec::with_capacity(len_u32_words as usize * 2);

    if offset % 4 != 0 {
        return Err(());
    }

    while len_u32_words >= 2 {
        let value1 = memory.read_word(offset);
        let value2 = memory.read_word(offset + 4);

        let value = (value2 as u64) << 32 | value1 as u64;

        result.push(value);
        offset += 8;
        len_u32_words -= 2;
    }

    assert_eq!(len_u32_words, 0);

    Ok(result)
}

/// # Safety
/// The data in the memory at offset should actually be T.
pub unsafe fn read_struct<T>(memory: &dyn U32Memory, offset: u32) -> Result<T, ()> {
    if core::mem::size_of::<T>() % 4 != 0 {
        todo!()
    }

    if offset as usize % core::mem::align_of::<T>() != 0 {
        return Err(());
    }

    let mut r = MaybeUninit::<T>::uninit();
    let ptr = r.as_mut_ptr();

    for i in (0..core::mem::size_of::<T>()).step_by(4) {
        let v = memory.read_word(offset + i as u32);

        // Safety: iterating over size of T, add will not overflow.
        unsafe { ptr.cast::<u32>().add(i / 4).write(v) };
    }

    // Safety: written all bytes.
    unsafe { Ok(r.assume_init()) }
}
