use std::{
    fs::File,
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    os::unix::fs::FileExt,
};

use crate::ResultType;

const BUFFER_LENGTH: usize = 128 * 1024;

/// Creates a buffer, no longer than `max_len`.
fn make_buffer() -> Vec<u8> {
    let mut result = Vec::<u8>::new();
    result.resize_with(BUFFER_LENGTH, Default::default);
    result
}

/// Structure to own some IO buffers and provide IO operations.
pub(crate) struct FileOps {
    buf_a: Vec<u8>,
    buf_b: Vec<u8>,
}

impl FileOps {
    pub fn new() -> Self {
        Self {
            buf_a: make_buffer(),
            buf_b: make_buffer(),
        }
    }

    pub fn check_equality_and_compute_checksum(
        &mut self,
        a: &mut File,
        a_offset: u64,
        b: &mut File,
        b_offset: u64,
        length: u64,
    ) -> ResultType<u64> {
        let mut hasher_a = DefaultHasher::new();
        let mut hasher_b = DefaultHasher::new();

        let mut read = 0u64;
        while read < length {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), length - read);
            let a_chunk = &mut self.buf_a[0..chunk_len.try_into().unwrap()];
            let b_chunk = &mut self.buf_b[0..chunk_len.try_into().unwrap()];
            a.read_exact_at(a_chunk, a_offset + read)?;
            b.read_exact_at(b_chunk, b_offset + read)?;

            assert_eq!(a_chunk, b_chunk);

            a_chunk.hash(&mut hasher_a);
            b_chunk.hash(&mut hasher_b);

            read += chunk_len;
        }

        let hash_a = hasher_a.finish();
        let hash_b = hasher_b.finish();
        assert_eq!(hash_a, hash_b);

        Ok(hash_a)
    }

    pub fn copy_segment(
        &mut self,
        f: &mut File,
        source: &Range<u64>,
        dest_offset: u64,
    ) -> ResultType<()> {
        let length = source.end - source.start;
        let mut read = 0u64;
        while read < length {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), length - read);
            let chunk = &mut self.buf_a[0..chunk_len.try_into().unwrap()];
            f.read_exact_at(chunk, source.start + read)?;
            f.write_all_at(chunk, dest_offset + read)?;
            read += chunk_len;
        }
        Ok(())
    }

    pub fn swap_segment(
        &mut self,
        f: &mut File,
        source: &Range<u64>,
        dest_offset: u64,
    ) -> ResultType<()> {
        let length = source.end - source.start;
        let mut read = 0u64;
        while read < length {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), length - read);
            let chunk_a = &mut self.buf_a[0..chunk_len.try_into().unwrap()];
            let chunk_b = &mut self.buf_b[0..chunk_len.try_into().unwrap()];

            f.read_exact_at(chunk_a, source.start + read)?;
            f.read_exact_at(chunk_b, dest_offset + read)?;

            f.write_all_at(chunk_a, dest_offset + read)?;
            f.write_all_at(chunk_b, source.start + read)?;

            read += chunk_len;
        }
        Ok(())
    }

    pub fn fill_zeros(&mut self, f: &mut File, range: &Range<u64>) -> ResultType<()> {
        self.buf_a.fill_with(Default::default);
        let mut out_offset = range.start;
        while out_offset < range.end {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), range.end - out_offset);
            f.write_all_at(&self.buf_a[0..chunk_len.try_into().unwrap()], out_offset)?;
            out_offset += chunk_len;
        }

        Ok(())
    }

    pub fn validate_checksum(
        &mut self,
        f: &File,
        offset: u64,
        length: u64,
        expected_csum: u64,
    ) -> ResultType<()> {
        let mut hasher = DefaultHasher::new();

        let mut read = 0u64;
        while read < length {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), length - read);
            let chunk = &mut self.buf_a[0..chunk_len.try_into().unwrap()];

            f.read_exact_at(chunk, offset + read)?;

            chunk.hash(&mut hasher);
            read += chunk_len;
        }

        let hash = hasher.finish();
        assert_eq!(hash, expected_csum, "Checksums should match");

        Ok(())
    }
}
