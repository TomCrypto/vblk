//! Mount virtual, application-defined block devices on Linux.

#![forbid(missing_docs)]

use crossbeam_utils::thread::scope;
use nix::errno::Errno::{EIO, EPERM};
use std::fs::{File, OpenOptions};
use std::io::{Error, Read, Result, Write};
use std::os::unix::{io::AsRawFd, net::UnixStream};
use std::path::Path;
use std::time::Duration;
use zerocopy::AsBytes;

mod nbd;

/// A virtual block device.
pub trait BlockDevice {
    /// Reads a byte range from the block device.
    ///
    /// # Note
    ///
    /// If you return an I/O error not associated with an OS `errno`, vblk
    /// will automatically return an `EIO` error to the caller by default.
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<()> {
        let _ = (offset, bytes);

        Err(Error::from_raw_os_error(EPERM as i32))
    }

    /// Writes a byte range to the block device.
    ///
    /// # Note
    ///
    /// If you return an I/O error not associated with an OS `errno`, vblk
    /// will automatically return an `EIO` error to the caller by default.
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        let _ = (offset, bytes);

        Err(Error::from_raw_os_error(EPERM as i32))
    }

    /// Flushes any cached data to the block device.
    ///
    /// # Note
    ///
    /// If you return an I/O error not associated with an OS `errno`, vblk
    /// will automatically return an `EIO` error to the caller by default.
    ///
    /// # Warning
    ///
    /// Support for this command depends on your Linux kernel version.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    /// Trims a byte range of the block device.
    ///
    /// # Note
    ///
    /// If you return an I/O error not associated with an OS `errno`, vblk
    /// will automatically return an `EIO` error to the caller by default.
    ///
    /// # Warning
    ///
    /// Support for this command depends on your Linux kernel version.
    fn trim(&mut self, offset: u64, len: u32) -> Result<()> {
        let _ = (offset, len);

        Ok(())
    }

    /// Called when the block device is unmounted.
    fn unmount(&mut self) {}

    /// Returns the device block size in bytes.
    ///
    /// According to the NBD kernel source code, the block size must currently
    /// be a power of two between 512 bytes and the system page size in bytes.
    fn block_size(&self) -> u32;

    /// Returns the device size in blocks.
    fn blocks(&self) -> u64;
}

/// A mounted block device.
#[derive(Debug)]
pub struct Device {
    file: File,
}

impl Device {
    /// Sets the internal kernel socket timeout for this device.
    ///
    /// # Safety
    ///
    /// Communicates with the NBD kernel module through ioctls.
    pub unsafe fn set_timeout(&self, timeout: Duration) -> Result<()> {
        nbd::set_timeout(&self.file, timeout.as_secs())
    }

    /// Unmounts this device immediately.
    ///
    /// # Safety
    ///
    /// Communicates with the NBD kernel module through ioctls.
    pub unsafe fn unmount(&self) -> Result<()> {
        nbd::disconnect(&self.file)
    }
}

/// Mounts a block device on an NBD device such as `/dev/nbd0`.
///
/// The callback will be invoked at the start of the mounting process and will
/// yield a structure which can be used to asynchronously unmount this device.
///
/// # Safety
///
/// Communicates with the NBD kernel module through ioctls.
pub unsafe fn mount<P: AsRef<Path>>(
    device: &mut dyn BlockDevice,
    path: P,
    callback: impl FnOnce(Device) -> Result<()>,
) -> Result<()> {
    let file = &OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.as_ref())?;

    let (block_size, blocks) = (device.block_size(), device.blocks());

    assert!(block_size.is_power_of_two());
    assert!(block_size >= 512);

    nbd::set_blksize(&file, block_size)?;
    nbd::set_size_blocks(&file, blocks)?;
    nbd::clear_sock(file)?;

    let (mut userspace_socket, kernel_socket) = UnixStream::pair()?;

    let result = scope(|scope| -> Result<()> {
        callback(Device {
            file: file.try_clone()?,
        })?;

        let thread = scope.spawn(move |_| -> Result<()> {
            nbd::set_sock(file, kernel_socket.as_raw_fd())?;

            // These flags (or even the ability to set flags) are not available
            // in every Linux version; this call is best-effort, ignore errors.

            let _ = nbd::set_flags(file, nbd::SEND_FLUSH | nbd::SEND_TRIM);

            nbd::do_it(file)?;

            // We can't really do anything meaningful if these cleanup calls
            // fail, so just assume that they succeed and hope for the best.

            let _ = nbd::clear_sock(file);
            let _ = nbd::clear_que(file);

            Ok(())
        });

        let mut request = nbd::Request::default();
        let mut buffer = Vec::with_capacity(4096);

        loop {
            let len = userspace_socket.read(&mut request.as_bytes_mut()[0..nbd::REQUEST_LEN])?;

            if len == 0 {
                break;
            }

            assert_eq!(len, nbd::REQUEST_LEN, "NBD driver error: too few bytes");
            assert!(request.is_magic_valid(), "NBD driver error: invalid magic");

            let mut reply = request.new_reply_for_request();

            match request.command() {
                nbd::Command::Read => {
                    buffer.resize(request.len() as usize, 0);

                    if let Err(err) = device.read(request.offset(), &mut buffer) {
                        reply.set_errno(err.raw_os_error().unwrap_or(EIO as i32));
                    }

                    userspace_socket.write_all(reply.as_bytes())?;
                    userspace_socket.write_all(buffer.as_slice())?;
                }
                nbd::Command::Write => {
                    buffer.resize(request.len() as usize, 0);
                    userspace_socket.read_exact(&mut buffer)?;

                    if let Err(err) = device.write(request.offset(), &buffer) {
                        reply.set_errno(err.raw_os_error().unwrap_or(EIO as i32));
                    }

                    userspace_socket.write_all(reply.as_bytes())?;
                }
                nbd::Command::Flush => {
                    if let Err(err) = device.flush() {
                        reply.set_errno(err.raw_os_error().unwrap_or(EIO as i32));
                    }

                    userspace_socket.write_all(reply.as_bytes())?;
                }
                nbd::Command::Trim => {
                    if let Err(err) = device.trim(request.offset(), request.len()) {
                        reply.set_errno(err.raw_os_error().unwrap_or(EIO as i32));
                    }

                    userspace_socket.write_all(reply.as_bytes())?;
                }
                nbd::Command::Disconnect => {
                    device.unmount();
                    break; // cancel
                }
                nbd::Command::Unknown => unreachable!("NBD driver error: unknown request type"),
            }
        }

        drop(userspace_socket);
        thread.join().unwrap()
    });

    if result.is_err() || result.as_ref().unwrap().is_err() {
        let _ = nbd::disconnect(file); // forced disconnect
    }

    result.unwrap()
}
