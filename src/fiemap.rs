// Definitions taken from `/usr/include/linux`.
use bitflags::bitflags;
use log::info;
use std::{
    ffi::c_int,
    fs::File,
    io,
    os::{fd::AsRawFd, raw::c_ulong},
};

pub(crate) fn do_the_thing(file: &File) -> crate::Result<()> {
    let mut fr = Box::new(FiemapRequestFull::default());
    fr.request.fm_start = 0;
    fr.request.fm_length = file.metadata()?.len();
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

    info!("Result!");
    info!("extent count: {}", fr.request.fm_mapped_extents);

    for e in &fr.fm_extents[..fr.request.fm_mapped_extents.try_into().unwrap()] {
        info!("Extent: {:?}", *e);
        info!("Flags: {:?}", FiemapExtentFlag::from_bits(e.fe_flags));
    }

    Ok(())
}

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct FiemapFlag: u32 {
        /// sync file data before map
        const SYNC = 0x00000001 ;
        /// map extended attribute tree
        const XATTR = 0x00000002 ;
        /// request caching of the extents
        const CACHE = 0x00000004;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
        pub(crate) struct FiemapExtentFlag: u32 {
        /// Last extent in file.
        const LAST            =  0x00000001;
        /// Data location unknown.
        const UNKNOWN         =  0x00000002 ;
        /// Location still pending. Sets EXTENT_UNKNOWN.
        const DELALLOC        =  0x00000004;

        /// Data can not be read while fs is unmounted
        const ENCODED        =   0x00000008;
        /// Data is encrypted by fs. EXTENT_NO_BYPASS.
        const ENCRYPTED   = 0x00000080;
        /// Extent offsets may not be block aligned.
        const NOT_ALIGNED     = 0x00000100;
        /// Data mixed with metadata. Sets EXTENT_NOT_ALIGNED.
        const DATA_INLINE      = 0x00000200;
        /// Multiple files in block. Sets EXTENT_NOT_ALIGNED.
        const DATA_TAIL        = 0x00000400;
        /// Space allocated, but no data (i.e. zero).
        const UNWRITTEN        = 0x00000800;
        /// File does not natively support extents. Result merged for efficiency.
        const MERGED           = 0x00001000;
        /// Space shared with other files.
        const SHARED          =  0x00002000;
    }
}

#[repr(C)]
#[derive(Default, Debug)]
struct FiemapExtent {
    /// logical offset in bytes for the start of
    /// the extent from the beginning of the file
    fe_logical: u64,
    /// physical offset in bytes for the start
    /// of the extent from the beginning of the disk
    fe_physical: u64,
    /// length in bytes for this extent
    fe_length: u64,
    fe_reserved64: [u64; 2],
    /// FIEMAP_EXTENT_* flags for this extent
    fe_flags: u32,
    fe_reserved: [u32; 3],
}

#[repr(C)]
#[derive(Debug, Default)]
struct FiemapRequest {
    /// logical offset (inclusive) at which to start mapping (in)
    fm_start: u64,
    /// logical length of mapping which userspace wants (in)
    fm_length: u64,
    /// FIEMAP_FLAG_* flags for request (in/out)
    fm_flags: u32, /*  */
    /// number of extents that were mapped (out)
    fm_mapped_extents: u32,
    /// size of fm_extents array (in)
    fm_extent_count: u32,
    fm_reserved: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
struct FiemapRequestFull {
    request: FiemapRequest,
    /// array of mapped extents (out)
    /// 32 is the most that `Default` gives us ootb.
    fm_extents: [FiemapExtent; 32],
}

/// The value of FS_IOC_FIEMAP constant.
///
/// Could be simply lifted from the C header,
/// but calculating it and testing that value
/// give some assurance that the structs are
/// the right size.
fn fs_ioc_fiemap() -> c_ulong {
    // access mode
    (0b11u64 << 30) |
        // size of request
        ((u64::try_from(std::mem::size_of::<FiemapRequest>()).unwrap() & 0x3FFF) << 16) |
        // type (f = file?)
        (u64::from(b'f') << 8) |
        // FIEMAP code.
        (11u64)
}

#[cfg(test)]
mod tests {
    use assert_hex::assert_eq_hex;

    use crate::fiemap::fs_ioc_fiemap;

    #[test]
    fn fs_ioc_fiemap_value() {
        assert_eq_hex!(0xC020660B, fs_ioc_fiemap());
    }
}
