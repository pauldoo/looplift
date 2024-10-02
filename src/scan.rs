use std::{
    fs::File,
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Read},
    os::{fd::AsRawFd, unix::fs::FileExt},
};

use log::{debug, info};
use serde::Serialize;

use crate::{
    fiemap::{fs_ioc_fiemap, ioctl, FiemapExtentFlag, FiemapFlag, FiemapRequestFull},
    report::{ExtentSource, ReportExtent, ReportSummary},
    utils::make_buffer,
    ResultType,
};

pub(crate) fn do_scan(
    file: &mut std::fs::File,
    device: &mut std::fs::File,
    out: &mut impl io::Write,
) -> ResultType<()> {
    let file_length = file.metadata()?.len();
    if file_length != device.metadata()?.len() {
        return Err("File length should be the same as the device.".into());
    }

    let mut serializer = serde_json::Serializer::new(out);
    ReportSummary {
        device_length: file_length,
    }
    .serialize(&mut serializer)?;

    let mut file_offset = 0u64;
    while file_offset < file_length {
        assert!(file_offset < file_length);
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
                let csum = check_equality_and_compute_checksum(
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

    Ok(())
}

fn check_equality_and_compute_checksum(
    a: &mut File,
    a_offset: u64,
    b: &mut File,
    b_offset: u64,
    length: u64,
) -> ResultType<u64> {
    let mut a_buf = make_buffer(length);
    let mut b_buf = make_buffer(length);
    assert_eq!(a_buf.len(), b_buf.len());
    let buf_len:u64 = a_buf.len().try_into().unwrap();

    let mut hasher_a = DefaultHasher::new();
    let mut hasher_b = DefaultHasher::new();

    let mut read = 0u64;
    while read < length {
        let chunk_len = u64::min(buf_len, length - read);
        let a_chunk = &mut a_buf[0..chunk_len.try_into().unwrap()];
        let b_chunk = &mut b_buf[0..chunk_len.try_into().unwrap()];
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
