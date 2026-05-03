use std::num::NonZero;
use std::os::fd::RawFd;

use crate::{NewObject, NewObjectId, ObjectId};
use crate::error::Error;
use crate::fixed::Fixed;


#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Packet {
    object_id: ObjectId,
    // top_size_bytes_bottom_opcode: u32
    opcode: u16, // merged with size in protocol
    payload: Vec<u8>,
    fds: Vec<RawFd>,
}
impl Packet {
    pub fn new(
        object_id: ObjectId,
        opcode: u16,
    ) -> Self {
        Self {
            object_id,
            opcode,
            payload: Vec::new(),
            fds: Vec::new(),
        }
    }

    pub fn new_from_existing(
        object_id: ObjectId,
        opcode: u16,
        payload: Vec<u8>,
        fds: Vec<RawFd>,
    ) -> Self {
        Self {
            object_id,
            opcode,
            payload,
            fds,
        }
    }

    pub fn object_id(&self) -> ObjectId { self.object_id }
    pub fn opcode(&self) -> u16 { self.opcode }

    pub fn set_object_id(&mut self, new_value: ObjectId) { self.object_id = new_value; }
    pub fn set_opcode(&mut self, new_value: u16) { self.opcode = new_value; }

    pub fn push_uint(&mut self, value: u32) {
        let bs = value.to_ne_bytes();
        self.payload.extend(&bs);
    }

    pub fn push_int(&mut self, value: i32) {
        let bs = value.to_ne_bytes();
        self.payload.extend(&bs);
    }

    pub fn push_fixed(&mut self, value: Fixed) {
        let bs = value.inner_value().to_ne_bytes();
        self.payload.extend(&bs);
    }

    pub fn push_str(&mut self, value: &str) {
        assert!(!value.contains("\0"));
        let len_with_nul = value.len() + 1;
        let lwn_u32: u32 = len_with_nul.try_into().unwrap();
        self.payload.extend(&lwn_u32.to_ne_bytes());
        self.payload.extend(value.as_bytes());
        self.payload.push(0x00);

        // align to 4 bytes
        let realign_count = (4 - (len_with_nul % 4)) % 4;
        self.payload.extend(std::iter::repeat_n(0x00, realign_count));
    }

    pub fn push_array(&mut self, array: &[u8]) {
        let len = array.len();
        let len_u32: u32 = len.try_into().unwrap();
        self.payload.extend(&len_u32.to_ne_bytes());
        self.payload.extend(array);

        let realign_count = (4 - (len % 4)) % 4;
        self.payload.extend(std::iter::repeat_n(0x00, realign_count));
    }

    pub fn push_object(&mut self, obj_id: Option<ObjectId>) {
        match obj_id {
            Some(oi) => self.push_uint(oi.0.into()),
            None => self.push_uint(0),
        }
    }

    pub fn push_new_id_known_interface(&mut self, new_id: NewObjectId) {
        self.push_object(Some(new_id.0))
    }

    pub fn push_new_id_unknown_interface(&mut self, new_obj: &NewObject) {
        self.push_str(&new_obj.interface);
        self.push_uint(new_obj.interface_version);
        self.push_object(Some(new_obj.object_id));
    }

    pub fn push_fd(&mut self, fd: RawFd) {
        self.fds.push(fd);
    }

    pub fn clear_payload(&mut self) {
        self.payload.clear();
        self.fds.clear();
    }

    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        let total_bytes = 8 + self.payload.len();
        let max_size: usize = u16::MAX.into();
        if total_bytes > max_size {
            return Err(Error::PacketTooLong {
                actual: total_bytes,
                maximum: max_size,
            });
        }
        let size_u16: u16 = total_bytes.try_into().unwrap();
        let top_size_bytes_bottom_opcode =
            (u32::from(size_u16) << 16)
            | (u32::from(self.opcode) <<  0)
        ;

        let mut buf = vec![0u8; total_bytes];
        buf[0..4].copy_from_slice(&self.object_id.0.get().to_ne_bytes());
        buf[4..8].copy_from_slice(&top_size_bytes_bottom_opcode.to_ne_bytes());
        buf[8..].copy_from_slice(&self.payload);
        Ok(buf)
    }

    pub fn fds(&self) -> &[RawFd] { &self.fds }

    pub fn read(&self) -> PacketReader<'_> {
        PacketReader {
            packet: self,
            payload_pos: 0,
            fd_pos: 0,
        }
    }
}

pub struct PacketReader<'a> {
    packet: &'a Packet,
    payload_pos: usize,
    fd_pos: usize,
}
impl<'a> PacketReader<'a> {
    fn peek_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        if self.payload_pos + buf.len() > self.packet.payload.len() {
            Err(Error::FieldOutOfBounds {
                actual: self.payload_pos + 4,
                maximum: self.packet.payload.len(),
            })
        } else {
            buf.copy_from_slice(
                &self.packet.payload[self.payload_pos..self.payload_pos+buf.len()]
            );
            Ok(())
        }
    }

    pub fn read_uint(&mut self) -> Result<u32, Error> {
        let mut buf = [0u8; 4];
        self.peek_bytes(&mut buf)?;
        self.payload_pos += 4;
        Ok(u32::from_ne_bytes(buf))
    }

    pub fn read_int(&mut self) -> Result<i32, Error> {
        let mut buf = [0u8; 4];
        self.peek_bytes(&mut buf)?;
        self.payload_pos += 4;
        Ok(i32::from_ne_bytes(buf))
    }

    pub fn read_fixed(&mut self) -> Result<Fixed, Error> {
        let mut buf = [0u8; 4];
        self.peek_bytes(&mut buf)?;
        self.payload_pos += 4;
        Ok(Fixed::from_inner_value(i32::from_ne_bytes(buf)))
    }

    pub fn read_str(&mut self) -> Result<String, Error> {
        let mut len_buf = [0u8; 4];
        self.peek_bytes(&mut len_buf)?;
        let len_u32 = u32::from_ne_bytes(len_buf);
        let len: usize = len_u32.try_into().unwrap();
        let padding_len = (4 - (len % 4)) % 4;
        let len_padded = len + padding_len;

        if self.payload_pos + 4 + len_padded > self.packet.payload.len() {
            return Err(Error::FieldOutOfBounds {
                actual: self.payload_pos + 4 + len_padded,
                maximum: self.packet.payload.len(),
            });
        }

        self.payload_pos += 4;
        let string_slice = &self.packet.payload[self.payload_pos..self.payload_pos+len];
        self.payload_pos += len_padded;

        let nul_pos = string_slice.iter().position(|b| *b == 0x00);
        if nul_pos != Some(string_slice.len() - 1) {
            return Err(Error::StringMisplacedNul {
                actual: nul_pos,
                expected: string_slice.len() - 1,
            });
        }
        let no_nul_string_slice = &string_slice[..string_slice.len()-1];

        let stringy = std::str::from_utf8(no_nul_string_slice)
            .map_err(|_| Error::StringInvalidUtf8 { data: no_nul_string_slice.to_vec() })?;
        Ok(stringy.to_owned())
    }

    pub fn read_array(&mut self) -> Result<Vec<u8>, Error> {
        let mut len_buf = [0u8; 4];
        self.peek_bytes(&mut len_buf)?;
        let len_u32 = u32::from_ne_bytes(len_buf);
        let len: usize = len_u32.try_into().unwrap();
        let padding_len = (4 - (len % 4)) % 4;
        let len_padded = len + padding_len;

        if self.payload_pos + 4 + len_padded > self.packet.payload.len() {
            return Err(Error::FieldOutOfBounds {
                actual: self.payload_pos + 4 + len_padded,
                maximum: self.packet.payload.len(),
            });
        }

        self.payload_pos += 4;
        let byte_slice = &self.packet.payload[self.payload_pos..self.payload_pos+len];
        self.payload_pos += len_padded;

        Ok(byte_slice.to_vec())
    }

    pub fn read_object(&mut self) -> Result<Option<ObjectId>, Error> {
        let oid = self.read_uint()?;
        Ok(NonZero::new(oid).map(ObjectId))
    }

    pub fn read_new_id_known_interface(&mut self) -> Result<NewObjectId, Error> {
        let oid_opt = self.read_object()?;
        let Some(oid) = oid_opt else {
            return Err(Error::ZeroObjectId);
        };
        Ok(NewObjectId(oid))
    }

    pub fn read_new_id_unknown_interface(&mut self) -> Result<NewObject, Error> {
        let interface = self.read_str()?;
        let version = self.read_uint()?;
        let oid_opt = self.read_object()?;
        let Some(oid) = oid_opt else {
            return Err(Error::ZeroObjectId);
        };

        Ok(NewObject {
            object_id: oid,
            interface,
            interface_version: version,
        })
    }

    pub fn read_fd(&mut self) -> Result<RawFd, Error> {
        if self.fd_pos >= self.packet.fds.len() {
            Err(Error::FdOutOfBounds { total: self.packet.fds.len() })
        } else {
            let fd = self.packet.fds[self.fd_pos];
            self.fd_pos += 1;
            Ok(fd)
        }
    }

    pub fn finish(&self) -> Result<(), Error> {
        let all_read =
            self.payload_pos >= self.packet.payload.len()
            && self.fd_pos >= self.packet.fds.len();
        if all_read {
            Ok(())
        } else {
            Err(Error::IncompleteRead {
                read_bytes: self.payload_pos,
                total_bytes: self.packet.payload.len(),
                read_fds: self.fd_pos,
                total_fds: self.packet.fds.len(),
            })
        }
    }
}
