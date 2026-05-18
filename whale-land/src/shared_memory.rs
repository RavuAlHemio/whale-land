use std::ffi::{CString, c_int, c_void};
use std::io;
use std::marker::PhantomData;
use std::ops::{Bound, RangeBounds};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::prelude::{BorrowedFd, RawFd};
use std::path::Path;
use std::ptr::null_mut;

use libc::{
    MAP_FAILED, MAP_SHARED, MFD_CLOEXEC, O_CREAT, O_EXCL, O_RDONLY, O_RDWR, O_TRUNC, PROT_READ,
    PROT_WRITE, close, fchmod, fchown, fstat, ftruncate, gid_t, memfd_create, mmap, mode_t, munmap,
    off_t, shm_open, shm_unlink, stat, uid_t,
};


macro_rules! impl_bool_setter {
    ($name:ident) => {
        pub fn $name(&mut self, $name: bool) -> &mut Self {
            self.$name = $name;
            self
        }
    }
}


#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OpenOptions {
    write: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
    mode: mode_t,
}
impl OpenOptions {
    pub const fn new() -> Self {
        Self {
            write: false,
            truncate: false,
            create: false,
            create_new: false,
            mode: 0o666,
        }
    }

    impl_bool_setter!(write);
    impl_bool_setter!(truncate);
    impl_bool_setter!(create);
    impl_bool_setter!(create_new);

    pub fn mode(&mut self, mode: u32) -> &mut Self {
        self.mode = mode;
        self
    }

    pub fn open(&self, name: &Path) -> Result<SharedMemoryObject, io::Error> {
        let name_c = CString::new(name.as_os_str().as_encoded_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let mut flags: c_int = if self.write {
            O_RDWR
        } else {
            O_RDONLY
        };
        if self.create_new {
            flags |= O_CREAT | O_EXCL;
        } else {
            if self.create {
                flags |= O_CREAT;
            }
            if self.truncate {
                flags |= O_TRUNC;
            }
        }

        let fd = unsafe {
            shm_open(name_c.as_ptr(), flags, self.mode)
        };
        if fd == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(SharedMemoryObject { file_descriptor: fd })
        }
    }
}
impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Metadata {
    pub length: off_t,
    pub owner: uid_t,
    pub group: gid_t,
    pub mode: mode_t,
}


#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SharedMemoryObject {
    file_descriptor: c_int,
}
impl SharedMemoryObject {
    pub fn new_anonymous() -> Result<Self, io::Error> {
        let memfd_name = c"whale_land::shared_memory::SharedMemoryObject";
        let file_descriptor = unsafe {
            memfd_create(
                memfd_name.as_ptr(),
                MFD_CLOEXEC,
            )
        };
        if file_descriptor == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self {
                file_descriptor,
            })
        }
    }

    pub fn close(mut self) -> Result<(), io::Error> {
        if self.file_descriptor == -1 {
            return Ok(());
        }
        let result = unsafe {
            close(self.file_descriptor)
        };
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            self.file_descriptor = -1;
            Ok(())
        }
    }

    pub fn metadata(&self) -> Result<Metadata, io::Error> {
        let mut st: stat = unsafe { std::mem::zeroed() };
        let result = unsafe {
            fstat(self.file_descriptor, &raw mut st)
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        let ret = Metadata {
            length: st.st_size,
            owner: st.st_uid,
            group: st.st_gid,
            mode: st.st_mode & 0o7777,
        };
        Ok(ret)
    }

    pub fn set_length(&mut self, new_length: off_t) -> Result<(), io::Error> {
        let result = unsafe {
            ftruncate(self.file_descriptor, new_length)
        };
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn set_ownership(&self, owner: uid_t, group: gid_t) -> Result<(), io::Error> {
        let result = unsafe {
            fchown(self.file_descriptor, owner, group)
        };
        if result == -1 {
            return Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn set_mode(&self, mode: mode_t) -> Result<(), io::Error> {
        let result = unsafe {
            fchmod(self.file_descriptor, mode)
        };
        if result == -1 {
            return Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn range_to_offset_and_length<R: RangeBounds<i64>>(&self, range: R) -> Result<(i64, usize), io::Error> {
        let offset = match range.start_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => (*i) + 1,
            Bound::Unbounded => 0,
        };
        let length_i64 = match range.end_bound() {
            Bound::Included(i) => (*i) + 1 - offset,
            Bound::Excluded(i) => (*i) - offset,
            Bound::Unbounded => {
                // find out the length
                let length = self.metadata()?.length;
                length - offset
            },
        };
        let length: usize = length_i64
            .try_into()
            .map_err(|_| io::Error::from(io::ErrorKind::FileTooLarge))?;
        Ok((offset, length))
    }

    pub fn map_read_only<'s, R: RangeBounds<i64>>(&'s mut self, range: R) -> Result<SharedMemoryMapping<'s>, io::Error> {
        let (offset, length) = self.range_to_offset_and_length(range)?;
        let pointer = unsafe {
            mmap(
                null_mut(),
                length,
                PROT_READ,
                MAP_SHARED,
                self.file_descriptor,
                offset,
            )
        };
        if pointer == MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(SharedMemoryMapping {
                pointer,
                length,
                shared_memory_phantom: PhantomData,
            })
        }
    }

    pub fn map_read_write<'s, R: RangeBounds<i64>>(&'s mut self, range: R) -> Result<SharedMemoryMapping<'s>, io::Error> {
        let (offset, length) = self.range_to_offset_and_length(range)?;
        let pointer = unsafe {
            mmap(
                null_mut(),
                length,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                self.file_descriptor,
                offset,
            )
        };
        if pointer == MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(SharedMemoryMapping {
                pointer,
                length,
                shared_memory_phantom: PhantomData,
            })
        }
    }
}
impl Drop for SharedMemoryObject {
    fn drop(&mut self) {
        if self.file_descriptor != -1 {
            unsafe {
                close(self.file_descriptor)
            };
            self.file_descriptor = -1;
        }
    }
}
impl AsFd for SharedMemoryObject {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.file_descriptor) }
    }
}
impl AsRawFd for SharedMemoryObject {
    fn as_raw_fd(&self) -> RawFd {
        self.file_descriptor
    }
}

pub struct SharedMemoryMapping<'s> {
    pointer: *mut c_void,
    length: usize,
    shared_memory_phantom: PhantomData<&'s mut SharedMemoryObject>,
}
impl<'s> SharedMemoryMapping<'s> {
    pub fn as_ptr(&self) -> *const c_void {
        self.pointer
    }

    pub fn as_mut_ptr(&self) -> *mut c_void {
        self.pointer
    }
}
impl<'s> Drop for SharedMemoryMapping<'s> {
    fn drop(&mut self) {
        unsafe {
            munmap(self.pointer, self.length)
        };
    }
}

pub fn unlink(name: &Path) -> Result<(), io::Error> {
    let name_c = CString::new(name.as_os_str().as_encoded_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    let result = unsafe {
        shm_unlink(name_c.as_ptr())
    };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
