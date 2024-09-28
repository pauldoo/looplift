use std::{collections::VecDeque, fs::File, hash::{DefaultHasher, Hash, Hasher}, io, ops::Range, os::unix::fs::FileExt};

use log::info;
use serde::Deserialize;

use crate::{
    report::{ReportExtent, ReportSummary}, utils::default_vec, ResultType
};

mod itree;

pub(crate) fn do_lift(device: std::fs::File, input: &mut impl io::Read) -> ResultType<()> {
    let device_length = device.metadata()?.len();
    let mut deserializer = serde_json::Deserializer::from_reader(input);

    let sr = ReportSummary::deserialize(&mut deserializer)?;
    assert_eq!(sr.device_length, device_length);

    let mut zeroing_queue: VecDeque<Range<u64>> = VecDeque::new();

    let mut expected_next_offset = 0u64;
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

            }
        }
    }

    info!("Extents loaded and csums match");

    todo!()
}


fn validate_checksum(
    f: &File,
    offset: u64,
    length: u64,
    expected_csum: u64
) -> ResultType<()> {
    let buf_len = u64::min(64 * 1024, length);
    let mut buf = default_vec::<u8>(buf_len.try_into().unwrap());

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
    assert_eq!(hash, expected_csum);

    Ok(())
}