use std::hash::{DefaultHasher, Hash, Hasher};

pub fn default_vec<T: Default>(len: usize) -> Vec<T> {
    let mut result: Vec<T> = Vec::new();
    result.resize_with(len, Default::default);
    result
}
