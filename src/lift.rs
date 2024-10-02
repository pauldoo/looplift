use std::{collections::VecDeque, fs::File, hash::{DefaultHasher, Hash, Hasher}, io::{self, Write}, ops::Range, os::unix::fs::FileExt, process::Output};

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

                assert!(copy_queue.insert(CopyOp{
                    source: offset..(offset + e.length),
                    destination_offset: e.destination_offset
                }));
            }
        }

    }

    info!("Extents loaded and csums match");

    info!("Copying extend data");
    'copy_loop:
    while !copy_queue.is_empty() {
        let op: CopyOp = copy_queue.first().unwrap().clone();

        if op.source.start == op.destination_offset {
            // is a no-op op, mark as done.
            assert!(copy_queue.remove(&op));
            continue;
        }

        let dest_range = op.destination_offset..(op.source.end - op.source.start + op.destination_offset);
        let overlapping_sources: Vec<CopyOp> = copy_queue.find(&dest_range).into_iter().cloned().collect();

        if overlapping_sources.is_empty() {
            // Nothing overlaps, including self which is still in the tree, do the copy
            assert!(copy_queue.remove(&op));
            copy_segment(&mut device, &op.source, op.destination_offset)?;
            continue;
        }

        // Look for overlapping operations, that we can split.
        for other_op in &overlapping_sources {
            assert!(dest_range.overlaps_range(&other_op.source));
            if dest_range == other_op.source {
                continue;
            }

            // Found an overlapping operation that isn't identical in range.
            // Split to make progress.
            // Note op and other_op _may_ be the same operation.
            if &op == other_op {
                info!("Found a self-overlapping operation: {:?}", op);
            }
            let (op1, op2) = 'split: {
                if dest_range.start < other_op.source.start {
                    // cut off piece at start
                    let prefix_len = other_op.source.start - dest_range.start;
                    assert!(copy_queue.remove(&op));
                    break 'split chop_op(&op, prefix_len);
                }
                if other_op.source.start < dest_range.start {
                    // cut off piece at start
                    let prefix_len = dest_range.start - other_op.source.start;
                    let other_op = other_op.clone();
                    assert!(copy_queue.remove(&other_op));
                    break 'split chop_op(&other_op, prefix_len);
                }
                assert_eq!(dest_range.start, other_op.source.start);

                if dest_range.end > other_op.source.end {
                    // cut off piece at end
                    let prefix_len = other_op.source.end - other_op.source.start;
                    assert!(copy_queue.remove(&op));
                    break 'split chop_op(&op, prefix_len);
                }
                if other_op.source.end > dest_range.end {
                    // cut off piece at end
                    let prefix_len = dest_range.end - dest_range.start;
                    let other_op = other_op.clone();
                    assert!(copy_queue.remove(&other_op));
                    break 'split chop_op(&other_op, prefix_len);
                }
                panic!("BUG");
            };

            assert!(copy_queue.insert(op1));
            assert!(copy_queue.insert(op2));
            continue 'copy_loop;
        }

        // Some things overlap, but they all do so with identical extents.
        assert!(copy_queue.remove(&op));
        swap_segment(&mut device, &op.source, op.destination_offset)?;
        for other_op in &overlapping_sources {
            assert!(&op != other_op);
            assert!(dest_range == other_op.source);
            assert!(copy_queue.remove(other_op));
            let mut new_op = other_op.clone();
            new_op.source = op.source.clone();
            assert!(copy_queue.insert(new_op));
        }

    }

    info!("Writing zero extents");
    while !zeroing_queue.is_empty() {
        let range = zeroing_queue.pop_front().unwrap();

        fill_zeros(&mut device, &range)?;
    }

    info!("Validating final csums");
    while !final_csum_queue.is_empty() {
        let csum = final_csum_queue.pop_front().unwrap();

        validate_checksum(&device, csum.offset, csum.length, csum.csum)?;
    }

    info!("All done.");

    Ok(())
}

fn chop_op(op: &CopyOp, prefix_len: u64) -> (CopyOp, CopyOp) {
    assert!(prefix_len > 0);
    assert!(op.source.start + prefix_len < op.source.end);
    let op1: CopyOp = CopyOp {
        source: op.source.start..(op.source.start + prefix_len),
        destination_offset: op.destination_offset
    };
    let op2: CopyOp = CopyOp {
        source: op1.source.end..op.source.end,
        destination_offset: op.destination_offset+prefix_len
    };
    (op1, op2)
}

fn copy_segment(f: &mut File, source: &Range<u64>, dest_offset: u64) -> ResultType<()> {
    let length = source.end - source.start;
    let mut buf = make_buffer(length);
    let mut read = 0u64;
    while read < length {
        let chunk_len = u64::min(buf.len().try_into().unwrap(), length - read);
        let chunk = &mut buf[0..chunk_len.try_into().unwrap()];
        f.read_exact_at(chunk, source.start + read)?;
        f.write_all_at(chunk, dest_offset + read)?;
        read += chunk_len;
    }
    Ok(())
}

fn swap_segment(f: &mut File, source: &Range<u64>, dest_offset: u64) -> ResultType<()> {
    let length = source.end - source.start;
    let mut buf_a = make_buffer(length);
    let mut buf_b = make_buffer(length);
    assert_eq!(buf_a.len(), buf_b.len());
    let mut read = 0u64;
    while read < length {
        let chunk_len = u64::min(buf_a.len().try_into().unwrap(), length - read);
        let chunk_a = &mut buf_a[0..chunk_len.try_into().unwrap()];
        let chunk_b = &mut buf_b[0..chunk_len.try_into().unwrap()];

        f.read_exact_at(chunk_a, source.start + read)?;
        f.read_exact_at(chunk_b, dest_offset + read)?;

        f.write_all_at(chunk_a, dest_offset + read)?;
        f.write_all_at(chunk_b, source.start + read)?;
        
        read += chunk_len;
    }
    Ok(())
}

fn fill_zeros(f: &mut File, range: &Range<u64>) -> ResultType<()> {
    let buf = make_buffer(range.end - range.start);
    let mut out_offset = range.start;
    while out_offset < range.end {
        let chunk_len = u64::min(buf.len().try_into().unwrap(), range.end - out_offset); 
        f.write_all_at(&buf[0..chunk_len.try_into().unwrap()], out_offset)?;
        out_offset += chunk_len;
    }

    Ok(())
}

struct CsumOp {
    offset: u64,
    length: u64,
    csum: u64
}

#[derive(Debug, PartialEq, Eq, Clone)]
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

#[cfg(test)]
mod tests {
    use std::{collections::btree_set::Intersection, ops::Range};

    use crate::tests::init_logger;

    use super::{IntervalTree, IntervalTreeEntry, RangeOps};

    #[test]
    fn overlaps() {
        assert!((0..10).overlaps_range(&(10..20)) == false);
        assert!((0..11).overlaps_range(&(10..20)) == true);
        assert!((0..30).overlaps_range(&(10..20)) == true);
        assert!((10..20).overlaps_range(&(10..20)) == true);
        assert!((12..18).overlaps_range(&(10..20)) == true);
        assert!((19..30).overlaps_range(&(10..20)) == true);
        assert!((20..30).overlaps_range(&(10..20)) == false);
    }

    #[test]
    fn overlaps_spam() {
        let min = 0u64;
        let max = 10u64;

        for a_b in min..max {
            for a_e in (a_b + 1)..=max {

                for b_b in min..max {
                    for b_e in (b_b + 1)..=max {

                        let expected = (min..max).into_iter()
                            .any(|q| (a_b..a_e).contains(&q) && (b_b..b_e).contains(&q));

                        let actual = (a_b..a_e).overlaps_range(&(b_b..b_e));
                        assert_eq!(expected, actual);
                    }
                }
            }
        }

    }
}