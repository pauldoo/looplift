use std::{
    error::Error,
    fs::{self},
    io::{BufReader, BufWriter},
};

use clap::{command, Parser, Subcommand};
use serde::{Deserialize, Serialize};

/// Raw wrapper for FIEMAP ioctl.
///
/// Definitions taken from `/usr/include/linux`.
mod fiemap;
mod lift;
mod report;
mod scan;
mod utils;

pub(crate) type ResultType<T> = std::result::Result<T, Box<dyn Error>>;

/// Lift loop files from within a filesystem to the block device hosting that filesystem.
///
/// Lifting is a two-step process.  First, use the `scan` command to obtain mapping details
/// for the file to be lifted.  Second, use the `lift` command to perform the promotion.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scans a file in preperation for lifting.
    ///
    /// Mapping data is sent to stdout which the user should capture.
    /// This is a non-destructive read-only operation.  It should be
    /// invoked while the host filesystem is mounted (ideally read-only).
    Scan {
        /// The file intended to be lifted.
        ///
        /// Must be the same logical length as the device.
        file: String,

        /// The device intended to be lifted to.
        ///
        /// This is required only so that the scanner can verify that
        /// the physical extents reported for the file can be read
        /// correctly from the underlying device, and are consistent
        /// with the file content.
        device: String,
    },
    /// Lifts a previously scanned file to the device.
    ///
    /// Previously captured mapping data is expected on stdin.
    ///
    /// This is a highly destructive operation, must not be cancelled
    /// once started, cannot be undone, and will result in data loss
    /// if interrupted.  It is a silly thing to do.
    Lift {
        /// The device to lift onto.
        device: String,
    },
}

fn main() -> ResultType<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { file, device } => scan::do_scan(
            &mut fs::OpenOptions::new().read(true).open(file)?,
            &mut fs::OpenOptions::new().read(true).open(device)?,
            &mut BufWriter::new(std::io::stdout()),
        )?,
        Commands::Lift { device } => lift::do_lift(
            fs::OpenOptions::new().read(true).write(true).open(device)?,
            &mut BufReader::new(std::io::stdin()),
        )?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use core::str;
    use std::io::{Cursor, Read};

    use log::info;
    use serde::{Deserialize, Serialize};

    use crate::ResultType;

    pub(crate) fn init_logger() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();
    }

    #[derive(Deserialize, Serialize, Debug, PartialEq)]
    enum CSum {
        Zeros(),
        NonZero(u64),
    }

    #[derive(Deserialize, Serialize, Debug, PartialEq)]
    struct Blah {
        thing: u64,
        other_thing: CSum,
    }

    #[derive(Deserialize, Serialize, Debug, PartialEq)]
    struct Header {
        message: String,
    }

    #[test]
    fn serde_play() -> ResultType<()> {
        init_logger();

        let h = Header {
            message: "Hello world!".to_string(),
        };

        let a = Blah {
            thing: 10,
            other_thing: CSum::NonZero(40),
        };

        let b = Blah {
            thing: 20,
            other_thing: CSum::Zeros(),
        };

        let mut buf: Vec<u8> = Vec::new();
        let mut serializer = serde_json::Serializer::new(&mut buf);
        h.serialize(&mut serializer)?;
        a.serialize(&mut serializer)?;
        b.serialize(&mut serializer)?;

        info!("encoded: {}", str::from_utf8(buf.as_slice())?);

        let mut r = serde_json::Deserializer::from_reader(buf.as_slice());

        let h2: Header = Header::deserialize(&mut r)?;
        let mut r_it = r.into_iter::<Blah>();
        let a2: Blah = r_it.next().unwrap()?;
        let b2: Blah = r_it.next().unwrap()?;

        assert_eq!(h, h2);
        assert_eq!(a, a2);
        assert_eq!(b, b2);
        assert!(r_it.next().is_none());

        Ok(())
    }
}
