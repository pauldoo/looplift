use bitflags::bitflags;
use log::info;
use std::{
    ffi::c_int,
    fs::File,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    os::{fd::AsRawFd, raw::c_ulong, unix::fs::FileExt},
};

extern "C" {
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
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
        /// Data is encrypted by fs. Sets EXTENT_NO_BYPASS.
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
pub struct FiemapExtent {
    /// logical offset in bytes for the start of
    /// the extent from the beginning of the file
    pub fe_logical: u64,
    /// physical offset in bytes for the start
    /// of the extent from the beginning of the disk
    pub fe_physical: u64,
    /// length in bytes for this extent
    pub fe_length: u64,
    fe_reserved64: [u64; 2],
    /// FIEMAP_EXTENT_* flags for this extent
    pub fe_flags: u32,
    fe_reserved: [u32; 3],
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct FiemapRequest {
    /// logical offset (inclusive) at which to start mapping (in)
    pub fm_start: u64,
    /// logical length of mapping which userspace wants (in)
    pub fm_length: u64,
    /// FIEMAP_FLAG_* flags for request (in/out)
    pub fm_flags: u32, /*  */
    /// number of extents that were mapped (out)
    pub fm_mapped_extents: u32,
    /// size of fm_extents array (in)
    pub fm_extent_count: u32,
    fm_reserved: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct FiemapRequestFull {
    pub request: FiemapRequest,
    /// array of mapped extents (out)
    /// 32 is the most that `Default` gives us ootb.
    pub fm_extents: [FiemapExtent; 32],
}

/// The value of FS_IOC_FIEMAP constant.
///
/// Could be simply lifted from the C header,
/// but calculating it and testing that value
/// give some assurance that the structs are
/// the right size.
pub fn fs_ioc_fiemap() -> c_ulong {
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

    use crate::{fiemap::fs_ioc_fiemap, tests::init_logger};

    #[test]
    fn fs_ioc_fiemap_value() {
        init_logger();
        assert_eq_hex!(0xC020660B, fs_ioc_fiemap());
    }
}
