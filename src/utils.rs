use std::{
    fs::File,
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    os::unix::fs::FileExt,
};

use indicatif::{HumanBytes, HumanCount, ProgressBar};
use log::info;

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
    dry_run: bool,
    buf_a: Vec<u8>,
    buf_b: Vec<u8>,
    read_ops: u64,
    read_bytes: u64,
    write_ops: u64,
    write_bytes: u64,
}

impl FileOps {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            buf_a: make_buffer(),
            buf_b: make_buffer(),
            read_ops: 0,
            read_bytes: 0,
            write_ops: 0,
            write_bytes: 0,
        }
    }

    pub fn check_equality_and_compute_checksum(
        &mut self,
        a: &File,
        a_offset: u64,
        b: &File,
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

            self.read_ops += 2;
            self.read_bytes += 2 * chunk_len;
        }

        let hash_a = hasher_a.finish();
        let hash_b = hasher_b.finish();
        assert_eq!(hash_a, hash_b);

        Ok(hash_a)
    }

    pub fn copy_segment(
        &mut self,
        f: &File,
        source: &Range<u64>,
        dest_offset: u64,
    ) -> ResultType<()> {
        let length = source.end - source.start;
        let mut read = 0u64;
        while read < length {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), length - read);
            let chunk = &mut self.buf_a[0..chunk_len.try_into().unwrap()];

            f.read_exact_at(chunk, source.start + read)?;
            self.read_ops += 1;
            self.read_bytes += chunk_len;

            if !self.dry_run {
                f.write_all_at(chunk, dest_offset + read)?;
                self.write_ops += 1;
                self.write_bytes += chunk_len;
            }

            read += chunk_len;
        }
        Ok(())
    }

    pub fn swap_segment(
        &mut self,
        f: &File,
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
            self.read_ops += 2;
            self.read_bytes += 2 * chunk_len;

            if !self.dry_run {
                f.write_all_at(chunk_a, dest_offset + read)?;
                f.write_all_at(chunk_b, source.start + read)?;
                self.write_ops += 2;
                self.write_bytes += 2 * chunk_len;
            }

            read += chunk_len;
        }
        Ok(())
    }

    pub fn fill_zeros(&mut self, f: &File, range: &Range<u64>) -> ResultType<()> {
        if self.dry_run {
            return Ok(());
        }

        self.buf_a.fill_with(Default::default);
        let mut out_offset = range.start;
        while out_offset < range.end {
            let chunk_len = u64::min(BUFFER_LENGTH.try_into().unwrap(), range.end - out_offset);
            f.write_all_at(&self.buf_a[0..chunk_len.try_into().unwrap()], out_offset)?;
            out_offset += chunk_len;

            self.write_ops += 1;
            self.write_bytes += chunk_len;
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

            self.read_ops += 1;
            self.read_bytes += chunk_len;
        }

        let hash = hasher.finish();
        assert_eq!(hash, expected_csum, "Checksums should match");

        Ok(())
    }

    pub(crate) fn log_stats(&self) {
        if self.read_ops > 0 {
            info!(
                "Read {} in {} operations ({} per operation)",
                HumanBytes(self.read_bytes),
                HumanCount(self.read_ops),
                HumanBytes(self.read_bytes / self.read_ops)
            );
        }
        if self.write_ops > 0 {
            info!(
                "Wrote {} in {} operations ({} per operation)",
                HumanBytes(self.write_bytes),
                HumanCount(self.write_ops),
                HumanBytes(self.write_bytes / self.write_ops)
            );
        }
    }
}

pub(crate) struct SimpleProgress {
    pb: ProgressBar,
    max: u64,
    last: Option<u64>,
}

impl SimpleProgress {
    pub fn new(max: u64) -> Self {
        Self {
            pb: ProgressBar::new(100),
            max,
            last: None,
        }
    }

    pub fn update(&mut self, value: u64) {
        if value > self.max {
            return self.update(self.max);
        }

        let value = (value * 100) / self.max;

        match self.last {
            Some(v) if v == value => {
                // no change
            }
            _ => {
                self.pb.set_position(value);
                self.last = Some(value);
            }
        }
    }

    pub fn finish(self) {
        self.pb.finish();
    }
}

/**
 * Verify that the device is at least as big as the provided size.
 *
 * This is performed by attempting to read the very last byte.
 */
pub(crate) fn validate_device_size(device: &std::fs::File, minimum_size: u64) -> ResultType<()> {
    assert!(minimum_size >= 1);
    let mut buf: [u8; 1] = [0u8];
    device
        .read_exact_at(&mut buf, minimum_size - 1)
        .map_err(|_| "Failed to verify that device is at least as large as the file to lift.")?;
    Ok(())
}
