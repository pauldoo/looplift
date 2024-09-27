use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ReportSummary {
    pub device_length: u64
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ReportExtent {
    pub destination_offset: u64,
    pub length: u64,
    pub source: ExtentSource,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ExtentSource {
    Zeros,
    Offset{ offset: u64, checksum: u64 }
}
