use std::{collections::VecDeque, io, ops::Range};

use serde::Deserialize;

use crate::{report::{ReportExtent, ReportSummary}, ResultType};

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
                zeroing_queue.push_back(e.destination_offset..(e.destination_offset+e.length));
            },
            crate::report::ExtentSource::Offset { offset, checksum } => {
                //validate_checksum(device, offset, e.length, checksum)?;

                todo!("enque operation")
            },
        }
    }

    todo!()
}

