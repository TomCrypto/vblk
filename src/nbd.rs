use nix::{errno, libc::ioctl, request_code_none};
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::os::unix::io::{AsRawFd, RawFd};
use zerocopy::{AsBytes, FromBytes};

macro_rules! nbd_ioctl {
    ($name:ident, $code:literal $(, $($arg:ident : $argty:ty),*)?) => {
        pub unsafe fn $name(file: &File$(, $($arg : $argty)*,)?) -> Result<()> {
            let _ = errno::Errno::result(
                ioctl(file.as_raw_fd(), request_code_none!(0xab, $code)$(, $($arg)*,)?)
            ).map_err(nix_error)?;

            Ok(())
        }
    };
}

nbd_ioctl!(set_sock, 0, sock: RawFd);
nbd_ioctl!(set_blksize, 1, blksize: u32);
nbd_ioctl!(do_it, 3);
nbd_ioctl!(clear_sock, 4);
nbd_ioctl!(clear_que, 5);
nbd_ioctl!(set_size_blocks, 7, size_blocks: u64);
nbd_ioctl!(disconnect, 8);
nbd_ioctl!(set_timeout, 9, timeout: u64);
nbd_ioctl!(set_flags, 10, flags: u64);

fn nix_error(error: nix::Error) -> Error {
    match error {
        nix::Error::Sys(errno) => Error::from_raw_os_error(errno as i32),
        nix::Error::UnsupportedOperation => panic!("unsupported by nix"),
        nix::Error::InvalidPath => Error::from(ErrorKind::InvalidInput),
        nix::Error::InvalidUtf8 => Error::from(ErrorKind::InvalidData),
    }
}

#[repr(C)]
#[derive(AsBytes, FromBytes, Default, Debug)]
pub struct Request {
    magic: u32,
    kind: u32,
    handle: u64,
    from: u64,
    len: u32,
    padding: u32,
}

pub const REQUEST_LEN: usize = std::mem::size_of::<Request>() - 4;

impl Request {
    pub fn is_magic_valid(&self) -> bool {
        self.magic == REQUEST_MAGIC.to_be()
    }

    pub fn new_reply_for_request(&self) -> Reply {
        Reply {
            magic: REPLY_MAGIC.to_be(),
            handle: self.handle,
            error: 0,
        }
    }

    pub fn command(&self) -> Command {
        match u32::from_be(self.kind) {
            CMD_READ => Command::Read,
            CMD_WRITE => Command::Write,
            CMD_DISC => Command::Disconnect,
            CMD_FLUSH => Command::Flush,
            CMD_TRIM => Command::Trim,
            _ => Command::Unknown,
        }
    }

    pub fn offset(&self) -> u64 {
        u64::from_be(self.from)
    }

    pub fn len(&self) -> u32 {
        u32::from_be(self.len)
    }
}

#[repr(C)]
#[derive(AsBytes, FromBytes, Debug)]
pub struct Reply {
    magic: u32,
    error: u32,
    handle: u64,
}

impl Reply {
    pub fn set_errno(&mut self, errno: i32) {
        self.error = (errno as u32).to_be();
    }
}

const REQUEST_MAGIC: u32 = 0x2560_9513;
const REPLY_MAGIC: u32 = 0x6744_6698;

const CMD_READ: u32 = 0;
const CMD_WRITE: u32 = 1;
const CMD_DISC: u32 = 2;
const CMD_FLUSH: u32 = 3;
const CMD_TRIM: u32 = 4;

pub const SEND_FLUSH: u64 = 1 << 2;
pub const SEND_TRIM: u64 = 1 << 5;

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Read,
    Write,
    Disconnect,
    Flush,
    Trim,
    Unknown,
}
