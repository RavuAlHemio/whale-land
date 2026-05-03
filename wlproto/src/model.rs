#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Protocol {
    pub name: String,
    pub interfaces: Vec<Interface>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Interface {
    pub name: String,
    pub version: u32,
    pub short_description: Option<String>,
    pub description: Option<String>,
    pub requests: Vec<Procedure>,
    pub events: Vec<Procedure>,
    pub enums: Vec<Enum>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Procedure {
    pub name: String,
    pub short_description: Option<String>,
    pub description: Option<String>,
    pub args: Vec<Arg>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Enum {
    pub name: String,
    pub short_description: Option<String>,
    pub description: Option<String>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EnumVariant {
    pub name: String,
    pub value: u32,
    pub short_description: Option<String>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Arg {
    pub name: String,
    pub arg_type: ArgType,
    pub interface: Option<String>,
    pub short_description: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ArgType {
    Uint,
    Int,
    Fixed,
    String,
    ObjectId,
    NewId,
    Array,
    FileDescriptor,
}
impl ArgType {
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "uint" => Some(Self::Uint),
            "int" => Some(Self::Int),
            "fixed" => Some(Self::Fixed),
            "string" => Some(Self::String),
            "object" => Some(Self::ObjectId),
            "new_id" => Some(Self::NewId),
            "array" => Some(Self::Array),
            "fd" => Some(Self::FileDescriptor),
            _ => None,
        }
    }
}
