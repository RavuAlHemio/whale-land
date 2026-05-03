use std::io;
use std::fmt;

use crate::ObjectId;


#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    MissingEnvVar { name: String },
    PacketTooLong { actual: usize, maximum: usize },
    PacketTooShort { actual: usize, minimum: usize },
    FieldOutOfBounds { actual: usize, maximum: usize },
    FdOutOfBounds { total: usize },
    StringMisplacedNul { actual: Option<usize>, expected: usize },
    StringInvalidUtf8 { data: Vec<u8> },
    IncompleteRead { read_bytes: usize, total_bytes: usize, read_fds: usize, total_fds: usize },
    ZeroObjectId,
    NoEventHandler { object_id: ObjectId },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e)
                => write!(f, "I/O error: {}", e),
            Self::MissingEnvVar { name }
                => write!(f, "missing environment variable {:?}", name),
            Self::PacketTooLong { actual, maximum }
                => write!(f, "packet ({} bytes) too long (maximum {} bytes)", actual, maximum),
            Self::PacketTooShort { actual, minimum }
                => write!(f, "packet ({} bytes) too short (minimum {} bytes)", actual, minimum),
            Self::FieldOutOfBounds { actual, maximum }
                => write!(f, "the requested field ({} bytes) would be out of bounds (maximum {} bytes)", actual, maximum),
            Self::FdOutOfBounds { total }
                => write!(f, "the requested file descriptor would be out of bounds (we have {})", total),
            Self::StringMisplacedNul { actual, expected }
                => write!(f, "the string's NUL termination is misplaced (actual {:?}, expected {})", actual, expected),
            Self::StringInvalidUtf8 { data }
                => write!(f, "string is invalid UTF-8: {:?}", data),
            Self::IncompleteRead { read_bytes, total_bytes, read_fds, total_fds }
                => write!(f, "incomplete read ({}/{} bytes, {}/{} file descriptors)", read_bytes, total_bytes, read_fds, total_fds),
            Self::ZeroObjectId
                => write!(f, "object ID was zero where a concrete object ID was expected"),
            Self::NoEventHandler { object_id }
                => write!(f, "no event handler for object ID {}", object_id.0),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::MissingEnvVar { .. } => None,
            Self::PacketTooLong { .. } => None,
            Self::PacketTooShort { .. } => None,
            Self::FieldOutOfBounds { .. } => None,
            Self::FdOutOfBounds { .. } => None,
            Self::StringMisplacedNul { .. } => None,
            Self::StringInvalidUtf8 { .. } => None,
            Self::IncompleteRead { .. } => None,
            Self::ZeroObjectId => None,
            Self::NoEventHandler { .. } => None,
        }
    }
}
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self { Self::Io(value) }
}
