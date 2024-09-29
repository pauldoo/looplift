use std::{collections::VecDeque, fs::File, hash::{DefaultHasher, Hash, Hasher}, io::{self, Write}, ops::Range, os::unix::fs::FileExt};

use itree::{IntervalTree, IntervalTreeEntry};
use log::info;
use serde::Deserialize;

use crate::{
    report::{ReportExtent, ReportSummary}, utils::make_buffer, ResultType
};

mod itree;

pub(crate) fn do_lift(mut device: std::fs::File, input: &mut impl io::Read) -> ResultType<()> {
    let device_length = device.metadata()?.len();
    let mut deserializer = serde_json::Deserializer::from_reader(input);

    let sr = ReportSummary::deserialize(&mut deserializer)?;
    assert_eq!(sr.device_length, device_length);

    let mut zeroing_queue = VecDeque::<Range<u64>>::new();
    let mut final_csum_queue = VecDeque::<CsumOp>::new();
    let mut copy_queue = IntervalTree::<CopyOp>::new(0..device_length);

    let mut expected_next_offset = 0u64;

    info!("Parsing report and validating initial checksums.");
    while expected_next_offset < device_length {
        let e = ReportExtent::deserialize(&mut deserializer)?;
        assert_eq!(e.destination_offset, expected_next_offset);
        expected_next_offset += e.length;

        match e.source {
            crate::report::ExtentSource::Zeros => {
                zeroing_queue.push_back(e.destination_offset..(e.destination_offset + e.length));
            }
            crate::report::ExtentSource::Offset { offset, checksum } => {
                validate_checksum(&device, offset, e.length, checksum)?;

                final_csum_queue.push_back(CsumOp{
                    offset: e.destination_offset,
                    length: e.length,
                    csum: checksum
                });

                copy_queue.insert(CopyOp{
                    source: offset..(offset + e.length),
                    destination_offset: e.destination_offset
                });
            }
        }

    }

    info!("Extents loaded and csums match");

    info!("Coping extend data");
    while !copy_queue.is_empty() {
        let op = copy_queue.pop_first().unwrap();

        if op.source.start == op.destination_offset {
            // is a no-op op
            continue;
        }

        let dest_range = op.destination_offset..(op.source.end - op.source.start + op.destination_offset);
        let overlapping_sources = copy_queue.find(&dest_range);
        if !op.source.overlaps_range(&dest_range) && overlapping_sources.is_empty() {
            // Nothing overlaps, do the copy

            todo!()
        }

        todo!()
    }

    info!("Writing zero extents");
    while !zeroing_queue.is_empty() {
        let range = zeroing_queue.pop_front().unwrap();

        fill_zeros(&mut device, range)?;
    }

    info!("Validating final csums");
    while !final_csum_queue.is_empty() {
        let csum = final_csum_queue.pop_front().unwrap();

        validate_checksum(&device, csum.offset, csum.length, csum.csum)?;
    }

    info!("All done.");

    Ok(())
}

fn fill_zeros(f: &mut File, range: Range<u64>) -> ResultType<()> {
    let buf = make_buffer(range.end - range.start);
    let mut out_offset = range.start;
    while out_offset < range.end {
        let chunk_len = u64::min(buf.len().try_into().unwrap(), range.end - out_offset); 
        f.write_all(&buf[0..chunk_len.try_into().unwrap()])?;
        out_offset += chunk_len;
    }

    Ok(())
}

struct CsumOp {
    offset: u64,
    length: u64,
    csum: u64
}

#[derive(Debug, PartialEq, Eq)]
struct CopyOp {
    source: Range<u64>,
    destination_offset: u64
}

impl IntervalTreeEntry for CopyOp {
    fn interval(&self) -> Range<u64> {
        self.source.clone()
    }
}

fn validate_checksum(
    f: &File,
    offset: u64,
    length: u64,
    expected_csum: u64
) -> ResultType<()> {
    let mut buf = make_buffer(length);
    let buf_len: u64 = buf.len().try_into().unwrap();

    let mut hasher = DefaultHasher::new();

    let mut read = 0u64;
    while read < length {
        let chunk_len = u64::min(buf_len, length - read);
        let chunk = &mut buf[0..chunk_len.try_into().unwrap()];

        f.read_exact_at(chunk, offset + read)?;

        chunk.hash(&mut hasher);
        read += chunk_len;
    }

    let hash = hasher.finish();
    assert_eq!(hash, expected_csum, "Checksums should match");

    Ok(())
}

trait RangeOps<T> {
    fn contains_range(&self, other: &Range<T>) -> bool;
    fn overlaps_range(&self, other: &Range<T>) -> bool;
}

impl<T: PartialOrd> RangeOps<T> for Range<T> {
    fn contains_range(&self, other: &Range<T>) -> bool {
        (self.start <= other.start) && (other.end <= self.end)
    }
    fn overlaps_range(&self, other: &Range<T>) -> bool {
        !((self.end <= other.start) || (other.end <= self.start))
    }
}
