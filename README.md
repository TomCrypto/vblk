# vblk

[![Documentation](https://docs.rs/vblk/badge.svg)](https://docs.rs/vblk)
[![Crates.io](https://img.shields.io/crates/v/vblk.svg)](https://crates.io/crates/vblk)

The `vblk` crate provides an interface to create virtual block devices which call into your Rust code for read and write operations. This is similar to FUSE except that instead of exposing an entire filesystem, you only expose a single fixed-size file as a block device. This can be handy if you are interfacing with a remote block device but don't have any convenient means of mounting it for whatever reason, or for prototyping block device drivers.

This is achieved by (ab)using the Linux kernel's NBD driver rather than implementing a new generic block device driver; a basic NBD client is started in the Rust application serving NBD requests which are shuttled to and from the kernel driver through a Unix domain socket. Most of the time this is done through forking; in this crate a separate thread is started instead for hosting the kernel-side NBD server.

This crate **only works on Linux 2.6+** and requires that the NBD kernel module be installed and loaded. It also **requires root** by default. Newer NBD features such as the `FLUSH` and `TRIM` commands may only be available in later Linux kernel versions; for older versions the Rust handlers for these commands will simply never be called.

## Usage

Assuming you have the proper permissions and the NBD module is loaded, this example will mount a virtual block device on `/dev/nbd0` which will read `0xDEADBEEF` across the entire block device. Since no write handler is specified, write requests to the block device will be denied. You can find and run this example in `examples/deadbeef.rs`, and there is a ramdisk example showing how writes work in `examples/ramdisk.rs`.

```rust
use std::io::Error;
use vblk::{mount, BlockDevice};

struct DeadbeefDevice;

impl BlockDevice for DeadbeefDevice {
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
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
    unsafe { mount(&mut DeadbeefDevice, "/dev/nbd0", |_device| Ok(())).unwrap() };
}
```

Once it's running, you should be able to access and read the block device:

```
# xxd /dev/nbd0 | head -n 2
00000000: dead beef dead beef dead beef dead beef  ................
00000010: dead beef dead beef dead beef dead beef  ................
# blockdev --getsize64 /dev/nbd0
4194304
```

But attempting to write to the block device will fail as we haven't provided a `write` handler, as observed below. Note that the `oflag=dsync` parameter is important here, otherwise the writes will be cached and asynchronously written back by the kernel and you will find the write errors in `dmesg` instead.

```
# dd oflag=dsync if=/dev/zero of=/dev/nbd0 bs=1024 count=4096
dd: error writing '/dev/nbd0': Input/output error
1+0 records in
0+0 records out
0 bytes copied, 0.000263995 s, 0.0 kB/s
```

## Unmounting

The virtual block device will only get its `unmount` handler called if the NBD connection is broken gracefully, so just sending Ctrl+C to your application will by default not do that. The callback you pass to the `mount` function yields an owned `Device` struct, which among other things has an `unmount` method on it; the struct is intended to be stored somewhere and used whenever your application needs to shut down. You must not block in the `callback`; the block device will not be mounted until it returns.

The examples show how to use this with the `ctrlc` crate, by intercepting Ctrl+C and unmounting the block device, which will cause the `mount` method to return gracefully.

## Permissions

By default on most distributions only root (or users in the `disk` group) can interface with the NBD kernel module and NBD block devices. I haven't tried this yet but it should be possible to configure the NBD device driver through e.g. `udev` rules depending on your environment.

## License

This software is provided under the MIT license.
