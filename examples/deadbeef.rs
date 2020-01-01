use std::io::Error;
use vblk::{mount, BlockDevice};

struct DeadbeefDevice;

impl BlockDevice for DeadbeefDevice {
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        println!("read request offset {} len {}", offset, bytes.len());

        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = match (index as u64 + offset) % 4 {
                0 => 0xDE,
                1 => 0xAD,
                2 => 0xBE,
                _ => 0xEF,
            };
        }

        Ok(())
    }

    fn block_size(&self) -> u32 {
        1024
    }

    fn blocks(&self) -> u64 {
        4096 // 4MB device
    }
}

fn main() {
    unsafe {
        mount(&mut DeadbeefDevice, "/dev/nbd0", |device| {
            ctrlc::set_handler(move || {
                device.unmount().unwrap();
            })
            .unwrap();

            Ok(())
        })
        .unwrap();
    }

    println!("exiting gracefully...");
}
