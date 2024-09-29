use std::hash::{DefaultHasher, Hash, Hasher};

fn default_vec<T: Default>(len: u64) -> Vec<T> {
    let mut result: Vec<T> = Vec::new();
    result.resize_with(len.try_into().unwrap(), Default::default);
    result
}

/// Creates a buffer, no longer than `max_len`.
pub fn make_buffer(max_len: u64) -> Vec<u8> {
    default_vec(u64::min(64 * 1024, max_len))
}