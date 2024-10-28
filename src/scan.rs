use std::{
    io::{self},
    os::fd::AsRawFd,
};

use log::debug;
use serde::Serialize;

use crate::{
    fiemap::{fs_ioc_fiemap, ioctl, FiemapExtentFlag, FiemapFlag, FiemapRequestFull},
    report::{ExtentSource, ReportExtent, ReportSummary},
    utils::{validate_device_size, FileOps, SimpleProgress},
    ResultType,
};

pub(crate) fn do_scan(
    file: &mut std::fs::File,
    device: &mut std::fs::File,
    out: &mut impl io::Write,
) -> ResultType<()> {
    let file_length = file.metadata()?.len();
    validate_device_size(device, file_length)?;

    let mut fops = FileOps::new(
        true, /* flag doesn't matter, as we don't attempt writes during scan. */
    );

    let mut serializer = serde_json::Serializer::new(out);
    ReportSummary {
        device_length: file_length,
    }
    .serialize(&mut serializer)?;

    let mut pb = SimpleProgress::new(file_length);

    let mut file_offset = 0u64;
    while file_offset < file_length {
        assert!(file_offset < file_length);
        pb.update(file_offset);

        let mut fr = Box::new(FiemapRequestFull::default());
        fr.request.fm_start = file_offset;
        fr.request.fm_length = file_length - file_offset;
        fr.request.fm_flags = FiemapFlag::SYNC.bits();
        fr.request.fm_mapped_extents = 0;
        fr.request.fm_extent_count = fr.fm_extents.len().try_into().unwrap();

        let result = unsafe {
            ioctl(
                file.as_raw_fd(),
                fs_ioc_fiemap(),
                (&mut *fr) as *mut FiemapRequestFull,
            )
        };

        if result != 0 {
            return Err(Box::new(io::Error::last_os_error()));
        }

        for e in &fr.fm_extents[..fr.request.fm_mapped_extents.try_into().unwrap()] {
            debug!("Extent: {:?}", *e);
            let flags = FiemapExtentFlag::from_bits(e.fe_flags).expect("Unknown extend bits set.");

            assert!(e.fe_logical >= file_offset);
            assert!(e.fe_logical < file_length);

            if e.fe_logical > file_offset {
                let re = ReportExtent {
                    destination_offset: file_offset,
                    length: e.fe_logical - file_offset,
                    source: ExtentSource::Zeros,
                };
                re.serialize(&mut serializer)?;
            }

            debug!("Flags: {:?}", flags);
            assert!(!flags.contains(FiemapExtentFlag::ENCODED));

            let readable_length = u64::min(file_length - e.fe_logical, e.fe_length);

            if flags.contains(FiemapExtentFlag::UNWRITTEN) {
                let re = ReportExtent {
                    destination_offset: e.fe_logical,
                    length: readable_length,
                    source: ExtentSource::Zeros,
                };
                re.serialize(&mut serializer)?;
            } else {
                let csum = fops.check_equality_and_compute_checksum(
                    file,
                    e.fe_logical,
                    device,
                    e.fe_physical,
                    readable_length,
                )?;

                let re = ReportExtent {
                    destination_offset: e.fe_logical,
                    length: readable_length,
                    source: ExtentSource::Offset {
                        offset: e.fe_physical,
                        checksum: csum,
                    },
                };
                re.serialize(&mut serializer)?;
            }

            file_offset = e.fe_logical + readable_length;

            if flags.contains(FiemapExtentFlag::LAST) {
                let re = ReportExtent {
                    destination_offset: file_offset,
                    length: file_length - file_offset,
                    source: ExtentSource::Zeros,
                };
                re.serialize(&mut serializer)?;
                file_offset = file_length;
            }
        }
    }
    pb.finish();

    fops.log_stats();

    Ok(())
}
