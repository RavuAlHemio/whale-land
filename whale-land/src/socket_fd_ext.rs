use std::ffi::c_void;
use std::fmt;
use std::future::Future;
use std::io;
use std::mem::size_of;
use std::os::fd::{AsRawFd, RawFd};
use std::ptr::null_mut;

use libc::{
    CMSG_DATA, CMSG_FIRSTHDR, CMSG_LEN, CMSG_NXTHDR, CMSG_SPACE, iovec, msghdr, recvmsg, SCM_RIGHTS,
    sendmsg, SOL_SOCKET,
};
use tokio::io::Interest;
use tokio::net::UnixStream;
use tracing::debug;


/// Socket extensions to send or receive file descriptors in parallel to data.
pub trait SocketFdExt {
    /// Sends the given data through the socket.
    ///
    /// The bytes to send are taken from `data`. `send` may transmit fewer bytes than are contained
    /// in `data`; if the return value is `Ok(n)`, the bytes `data[..n]` were sent.
    ///
    /// Automatically retries if the operating system returns [`WouldBlock`].
    ///
    /// [`WouldBlock`]: std::io::ErrorKind::WouldBlock
    ///
    /// Returns how many bytes were actually sent.
    fn send(&self, data: &[u8]) -> impl Future<Output = Result<usize, io::Error>> + Send;

    /// Sends the given data and the given file descriptors through the socket.
    ///
    /// The bytes to send are taken from `data`. `send_with_fds` may transmit fewer bytes than are
    /// contained in `data`; if the return value is `Ok(n)`, the bytes `data[..n]` were sent.
    ///
    /// The file descriptors to send are taken from `fds`. In contrast to the bytes in `data`, the
    /// caller may assume that all file descriptors in `fds` have been transmitted, unless at least
    /// one of the following conditions is true:
    ///
    /// 1. `data` is empty (`data.len()` equals 0)
    /// 2. `send_with_fds` returns `Err(_)`
    ///
    /// Automatically retries if the operating system returns [`WouldBlock`].
    ///
    /// [`WouldBlock`]: std::io::ErrorKind::WouldBlock
    ///
    /// Returns how many bytes were actually sent.
    fn send_with_fds(&self, data: &[u8], fds: &[RawFd]) -> impl Future<Output = Result<usize, io::Error>>;

    /// Receives data through the socket.
    ///
    /// The received bytes are stored in `buf`. `recv` may receive fewer bytes than fit into `buf`;
    /// if the return value is `Ok(n)`, the bytes `buf[..n]` were received.
    ///
    /// Automatically retries if the operating system returns [`WouldBlock`].
    ///
    /// [`WouldBlock`]: std::io::ErrorKind::WouldBlock
    ///
    /// Returns how many bytes were actually received.
    fn recv(&self, buf: &mut [u8]) -> impl Future<Output = Result<usize, io::Error>> + Send;

    /// Receives data and file descriptors through the socket.
    ///
    /// The received bytes are stored in `buf`. `recv` may receive fewer bytes than fit into `buf`;
    /// if the return value is `Ok((n, _))`, the bytes `buf[..n]` were received.
    ///
    /// The received file descriptors are appended to `fds`.
    ///
    /// Automatically retries if the operating system returns [`WouldBlock`].
    ///
    /// [`WouldBlock`]: std::io::ErrorKind::WouldBlock
    ///
    /// Returns how many bytes and how many file descriptors were actually received.
    fn recv_with_fds(&self, buf: &mut [u8], fds: &mut Vec<RawFd>) -> impl Future<Output = Result<(usize, usize), io::Error>>;
}


struct SendableIovec(pub iovec);
unsafe impl Send for SendableIovec {}
unsafe impl Sync for SendableIovec {}

struct SendableMsghdr(pub msghdr);
unsafe impl Send for SendableMsghdr {}
unsafe impl Sync for SendableMsghdr {}


struct HexSlice<'a>(pub &'a [u8]);
impl<'a> fmt::Display for HexSlice<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}


impl SocketFdExt for UnixStream {
    async fn send(&self, data: &[u8]) -> Result<usize, io::Error> {
        let sent_count = loop {
            self.writable().await?;
            match self.try_write(data) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
        };

        debug!("sent {}", HexSlice(&data[..sent_count]));

        Ok(sent_count)
    }

    async fn send_with_fds(&self, data: &[u8], fds: &[RawFd]) -> Result<usize, io::Error> {
        // assemble the general message structure including the buffer for "additional stuff"
        let add_stuff_payload_len = fds.len() * size_of::<RawFd>();
        let add_stuff_len: usize = unsafe {
            CMSG_SPACE(
                add_stuff_payload_len.try_into().unwrap()
            ).try_into().unwrap()
        };
        let mut add_stuff_buf = vec![0u8; add_stuff_len];
        let mut iov = SendableIovec(iovec {
            iov_base: data.as_ptr() as *const c_void as *mut c_void,
            iov_len: data.len(),
        });
        let add_struct = SendableMsghdr(msghdr {
            msg_name: null_mut(),
            msg_namelen: 0,
            msg_iov: &mut iov.0,
            msg_iovlen: 1,
            msg_control: add_stuff_buf.as_mut_ptr() as *mut c_void,
            msg_controllen: add_stuff_len,
            msg_flags: 0,
        });

        unsafe {
            // get the header of the first additional-stuff value
            let add_first_header = CMSG_FIRSTHDR(&add_struct.0);

            // populate it
            (*add_first_header).cmsg_level = SOL_SOCKET;
            (*add_first_header).cmsg_type = SCM_RIGHTS;
            (*add_first_header).cmsg_len = CMSG_LEN(
                add_stuff_payload_len.try_into().unwrap()
            ).try_into().unwrap();

            // get the location of its data and write the FDs
            let data_ptr = CMSG_DATA(add_first_header);
            let data_ptr_slice = std::slice::from_raw_parts_mut(
                data_ptr,
                add_stuff_payload_len,
            );
            write_slice_as_bytes(
                fds,
                data_ptr_slice,
            );
        }

        // grab the file descriptor
        let fd: RawFd = self.as_raw_fd();

        let total_sent = loop {
            // wait until we are ready to send
            self.writable().await?;

            let send_res: Result<usize, io::Error> = self.try_io(
                Interest::WRITABLE,
                || {
                    let sent = unsafe {
                        sendmsg(fd, &add_struct.0, 0)
                    };
                    if sent == -1 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(sent.try_into().unwrap())
                    }
                },
            );
            match send_res {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // try again
                    continue;
                },
                Err(e) => return Err(e.into()),
            }
        };

        debug!("sent {} with FDs: {:?}", HexSlice(&data[..total_sent]), fds);

        Ok(total_sent)
    }

    async fn recv(&self, buf: &mut [u8]) -> Result<usize, io::Error> {
        let received = loop {
            self.readable().await?;
            match self.try_read(buf) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
        };

        if received > 0 {
            debug!("received {}", HexSlice(&buf[..received]));
        }

        Ok(received)
    }

    async fn recv_with_fds(&self, buf: &mut [u8], fds: &mut Vec<RawFd>) -> Result<(usize, usize), io::Error> {
        let mut iov = iovec {
            iov_base: buf.as_mut_ptr() as *mut c_void,
            iov_len: buf.len(),
        };
        // let's hope 4M is big enough for the additional stuff
        let mut add_stuff_buf = vec![0u8; 4*1024*1024];
        let mut msg = msghdr {
            msg_name: null_mut(),
            msg_namelen: 0,
            msg_iov: &mut iov,
            msg_iovlen: 1,
            msg_control: add_stuff_buf.as_mut_ptr() as *mut c_void,
            msg_controllen: add_stuff_buf.len(),
            msg_flags: 0,
        };

        let fd = self.as_raw_fd();

        // and here we go again
        let total_received = loop {
            self.readable().await?;

            let receive_res: Result<usize, io::Error> = self.try_io(
                Interest::READABLE,
                || {
                    let received = unsafe {
                        recvmsg(fd, &mut msg, 0)
                    };
                    if received == -1 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(received.try_into().unwrap())
                    }
                },
            );
            match receive_res {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // try again
                    continue;
                },
                Err(e) => return Err(e.into()),
            }
        };

        // fish out the file descriptors
        let mut fds_read_count = 0;
        unsafe {
            let mut add_header = CMSG_FIRSTHDR(&msg);
            while !add_header.is_null() {
                if (*add_header).cmsg_level == SOL_SOCKET && (*add_header).cmsg_type == SCM_RIGHTS {
                    // yup, that's the one!
                    let data_buffer = CMSG_DATA(add_header);
                    let data_len_bytes = (*add_header).cmsg_len - usize::try_from(CMSG_LEN(0)).unwrap();
                    let data_len_fds = data_len_bytes / size_of::<RawFd>();
                    let mut fd_buf = vec![0 as RawFd; data_len_fds];

                    // copy out as bytes
                    let fd_buf_slice = std::slice::from_raw_parts_mut(
                        fd_buf.as_mut_ptr() as *mut u8,
                        fd_buf.len() * size_of::<RawFd>(),
                    );
                    let data_slice = std::slice::from_raw_parts(
                        data_buffer,
                        fd_buf_slice.len(),
                    );
                    fd_buf_slice.copy_from_slice(data_slice);

                    // run through
                    fds.extend(&fd_buf);
                    fds_read_count += fd_buf.len();
                }
                add_header = CMSG_NXTHDR(&msg, add_header);
            }
        }

        let my_fds = &fds[fds.len()-fds_read_count..];
        debug!("received {} with FDs: {:?}", HexSlice(&buf[..total_received]), my_fds);

        // and that is it
        Ok((total_received, my_fds.len()))
    }
}


unsafe fn write_slice_as_bytes<T>(value: &[T], buf: &mut [u8]) {
    if value.len() == 0 {
        return;
    }

    let size1 = size_of_val(&value[0]);
    let size = value.len() * size1;
    assert_eq!(size, buf.len());
    let ptr_b = value.as_ptr() as *const u8;
    let slice_b = unsafe {
        std::slice::from_raw_parts(
            ptr_b,
            size,
        )
    };
    buf.copy_from_slice(slice_b);
}
