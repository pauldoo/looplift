use std::{
    collections::VecDeque,
    io::{self, Read},
    ops::Range,
};

use itree::{IntervalTree, IntervalTreeEntry};
use log::info;
use serde::Deserialize;

use crate::{
    report::{ReportExtent, ReportSummary},
    utils::{FileOps, SimpleProgress},
    ResultType,
};

mod itree;

struct OperationQueues {
    zeroing: VecDeque<Range<u64>>,
    csums: VecDeque<CsumOp>,
    copies: IntervalTree<CopyOp>,
    device_length: u64,
}

pub(crate) fn do_lift(
    device: std::fs::File,
    input: &mut impl io::Read,
    dry_run: bool,
) -> ResultType<()> {
    let mut fops = FileOps::new(dry_run);

    let opq: OperationQueues = load_mapping(&device, input, &mut fops)?;
    perform_shuffles(&device, opq.copies, &mut fops, opq.device_length)?;
    fill_zeros(&device, opq.zeroing, &mut fops, opq.device_length)?;
    if !dry_run {
        validate_csums(&device, opq.csums, &mut fops, opq.device_length)?;
    } else {
        info!("Dry-run, so not confirming final checksums.");
    }

    info!("All done.");

    fops.log_stats();

    Ok(())
}

fn load_mapping(
    device: &std::fs::File,
    input: &mut impl Read,
    fops: &mut FileOps,
) -> ResultType<OperationQueues> {
    let device_length = device.metadata()?.len();
    info!("Parsing report and validating initial checksums.");

    let mut deserializer = serde_json::Deserializer::from_reader(input);
    let sr = ReportSummary::deserialize(&mut deserializer)?;
    assert_eq!(sr.device_length, device_length);

    let mut result = OperationQueues {
        zeroing: Default::default(),
        csums: Default::default(),
        copies: IntervalTree::new(0..device_length),
        device_length,
    };

    let mut pb = SimpleProgress::new(device_length);

    let mut expected_next_offset = 0u64;
    while expected_next_offset < device_length {
        pb.update(expected_next_offset);

        let e = ReportExtent::deserialize(&mut deserializer)?;
        assert_eq!(e.destination_offset, expected_next_offset);
        expected_next_offset += e.length;

        match e.source {
            crate::report::ExtentSource::Zeros => {
                result
                    .zeroing
                    .push_back(e.destination_offset..(e.destination_offset + e.length));
            }
            crate::report::ExtentSource::Offset { offset, checksum } => {
                fops.validate_checksum(device, offset, e.length, checksum)?;

                result.csums.push_back(CsumOp {
                    offset: e.destination_offset,
                    length: e.length,
                    csum: checksum,
                });

                assert!(result.copies.insert(CopyOp {
                    source: offset..(offset + e.length),
                    destination_offset: e.destination_offset
                }));
            }
        }
    }
    pb.finish();
    info!("Extents loaded and csums match");
    Ok(result)
}

fn perform_shuffles(
    device: &std::fs::File,
    mut copy_queue: IntervalTree<CopyOp>,
    fops: &mut FileOps,
    device_length: u64,
) -> ResultType<()> {
    info!("Copying extent data");
    let mut pb = SimpleProgress::new(device_length);
    'copy_loop: while !copy_queue.is_empty() {
        let op: CopyOp = copy_queue.first().unwrap().clone();
        pb.update(op.source.start);

        if op.source.start == op.destination_offset {
            // is a no-op op, mark as done.
            assert!(copy_queue.remove(&op));
            continue;
        }

        let dest_range =
            op.destination_offset..(op.source.end - op.source.start + op.destination_offset);
        let overlapping_sources: Vec<CopyOp> =
            copy_queue.find(&dest_range).into_iter().cloned().collect();

        if overlapping_sources.is_empty() {
            // Nothing overlaps, including self which is still in the tree, do the copy
            assert!(copy_queue.remove(&op));
            fops.copy_segment(device, &op.source, op.destination_offset)?;
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
            // Note op and other_op _may_ be the same operation, that is okay.
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
        fops.swap_segment(device, &op.source, op.destination_offset)?;
        for other_op in &overlapping_sources {
            assert!(&op != other_op);
            assert!(dest_range == other_op.source);
            assert!(copy_queue.remove(other_op));
            let mut new_op = other_op.clone();
            new_op.source = op.source.clone();
            assert!(copy_queue.insert(new_op));
        }
    }
    pb.finish();

    Ok(())
}

fn fill_zeros(
    device: &std::fs::File,
    mut zeroing_queue: VecDeque<Range<u64>>,
    fops: &mut FileOps,
    device_length: u64,
) -> ResultType<()> {
    info!("Writing zero extents");
    let mut pb = SimpleProgress::new(device_length);
    while !zeroing_queue.is_empty() {
        let range = zeroing_queue.pop_front().unwrap();
        pb.update(range.start);

        fops.fill_zeros(device, &range)?;
    }
    pb.finish();
    Ok(())
}

fn validate_csums(
    device: &std::fs::File,
    mut csums: VecDeque<CsumOp>,
    fops: &mut FileOps,
    device_length: u64,
) -> ResultType<()> {
    info!("Validating final csums");
    let mut pb = SimpleProgress::new(device_length);
    while !csums.is_empty() {
        let csum = csums.pop_front().unwrap();
        pb.update(csum.offset);

        fops.validate_checksum(device, csum.offset, csum.length, csum.csum)?;
    }
    pb.finish();
    Ok(())
}

fn chop_op(op: &CopyOp, prefix_len: u64) -> (CopyOp, CopyOp) {
    assert!(prefix_len > 0);
    assert!(op.source.start + prefix_len < op.source.end);
    let op1: CopyOp = CopyOp {
        source: op.source.start..(op.source.start + prefix_len),
        destination_offset: op.destination_offset,
    };
    let op2: CopyOp = CopyOp {
        source: op1.source.end..op.source.end,
        destination_offset: op.destination_offset + prefix_len,
    };
    (op1, op2)
}

struct CsumOp {
    offset: u64,
    length: u64,
    csum: u64,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct CopyOp {
    source: Range<u64>,
    destination_offset: u64,
}

impl IntervalTreeEntry for CopyOp {
    fn interval(&self) -> Range<u64> {
        self.source.clone()
    }
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

    use crate::tests::init_logger;

    use super::RangeOps;

    #[test]
    fn overlaps() {
        init_logger();
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
        init_logger();
        let min = 0u64;
        let max = 10u64;

        for a_b in min..max {
            for a_e in (a_b + 1)..=max {
                for b_b in min..max {
                    for b_e in (b_b + 1)..=max {
                        let expected = (min..max)
                            .into_iter()
                            .any(|q| (a_b..a_e).contains(&q) && (b_b..b_e).contains(&q));

                        let actual = (a_b..a_e).overlaps_range(&(b_b..b_e));
                        assert_eq!(expected, actual);
                    }
                }
            }
        }
    }
}
