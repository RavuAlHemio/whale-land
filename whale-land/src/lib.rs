pub mod connection;
pub mod error;
pub mod fixed;
pub mod packet;
pub mod protocol;
pub mod shared_memory;
mod socket_fd_ext;


use std::num::NonZero;

pub use crate::connection::{Connection, WeakConnection};
pub use crate::error::Error;
pub use crate::fixed::Fixed;
pub use crate::packet::Packet;


#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct ObjectId(pub NonZero<u32>);
impl ObjectId {
    pub const DISPLAY: ObjectId = ObjectId::new(1).unwrap();

    pub const fn new(object_id: u32) -> Option<Self> {
        match NonZero::new(object_id) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct NewObjectId(pub ObjectId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NewObject {
    pub object_id: ObjectId,
    pub interface: String,
    pub interface_version: u32,
}
