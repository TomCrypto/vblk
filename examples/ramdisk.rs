use std::io::Result;
use vblk::{mount, BlockDevice};

struct RamDisk {
    memory: Vec<u8>,
}

impl BlockDevice for RamDisk {
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<()> {
        println!("read request offset {} len {}", offset, bytes.len());

        bytes.copy_from_slice(&self.memory[offset as usize..offset as usize + bytes.len()]);

        Ok(())
    }

    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        println!("write request offset {} len {}", offset, bytes.len());

        self.memory[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);

        Ok(())
    }

    fn unmount(&mut self) {
        println!("ramdisk unmounted!");
    }

    fn flush(&mut self) -> Result<()> {
        println!("flush request");

        Ok(())
    }

    fn block_size(&self) -> u32 {
        1024
    }

    fn blocks(&self) -> u64 {
        (self.memory.len() / 1024) as u64
    }
}

fn main() {
    const RAMDISK_SIZE: usize = 33_554_432;

    let mut disk = RamDisk {
        memory: vec![0; RAMDISK_SIZE],
    };

    unsafe {
        mount(&mut disk, "/dev/nbd0", |device| {
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
