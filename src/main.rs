use std::{
    error::Error,
    fs::{self},
};

mod fiemap;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn Error>>;

// A random file that exists on my system on ext4.
const TEST_PATH: &str = "/boot/ostree/default-14cb0a46f0d5ea6de660642da798e63f45bf1466c7141826fcf30f1f46f54652/initramfs-6.9.12-100.fc39.x86_64.img";

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    println!("Hello, world!");

    let file = fs::OpenOptions::new().read(true).open(TEST_PATH)?;
    fiemap::do_the_thing(&file)?;

    Ok(())
}
