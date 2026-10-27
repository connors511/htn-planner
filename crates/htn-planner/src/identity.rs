//! Compatibility-sensitive streaming identity codec; this is not a persistence format.
use std::fmt;

use serde::{Serialize, Serializer, ser};

use crate::{COMPATIBILITY_ID, CompiledDomain, Definition, Limits, Schema};

/// Canonical identity of an admitted schema, definition, limits and planner semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DefinitionIdentity([u8; 32]);

impl DefinitionIdentity {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A representation outside the identity codec's deterministic vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IdentityError {
    UnsupportedRepresentation(&'static str),
    LengthOutOfRange,
    InvalidLength,
    Serialization(String),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRepresentation(kind) => {
                write!(f, "unsupported identity representation: {kind}")
            }
            Self::LengthOutOfRange => f.write_str("identity length exceeds u64"),
            Self::InvalidLength => {
                f.write_str("identity collection length disagrees with its contents")
            }
            Self::Serialization(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for IdentityError {}
impl ser::Error for IdentityError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Serialization(message.to_string())
    }
}

impl CompiledDomain {
    /// Returns the portable identity computed once during admission, excluding source locators.
    ///
    /// The cached value can accompany pending plans without hashing during search.
    pub const fn identity(&self) -> DefinitionIdentity {
        self.identity
    }
}

pub(crate) fn compute_identity(
    schema: &Schema,
    definition: &Definition,
    limits: &Limits,
) -> Result<DefinitionIdentity, IdentityError> {
    let mut serializer = Canonical(blake3::Hasher::new());
    serializer
        .0
        .update(b"HTN-PLANNER-DEFINITION-IDENTITY\0CODEC-1\0");
    (COMPATIBILITY_ID, schema, definition, limits).serialize(&mut serializer)?;
    Ok(DefinitionIdentity(*serializer.0.finalize().as_bytes()))
}

struct Canonical(blake3::Hasher);
impl Canonical {
    fn tag(&mut self, tag: u8) {
        self.0.update(&[tag]);
    }
    fn length(&mut self, length: usize) -> Result<(), IdentityError> {
        self.0.update(
            &u64::try_from(length)
                .map_err(|_| IdentityError::LengthOutOfRange)?
                .to_le_bytes(),
        );
        Ok(())
    }
    fn compound(&mut self, tag: u8, length: usize) -> Result<Compound<'_>, IdentityError> {
        self.tag(tag);
        self.length(length)?;
        Ok(Compound {
            serializer: self,
            remaining: length,
        })
    }
    fn name(&mut self, name: &str) -> Result<(), IdentityError> {
        self.tag(0x21);
        self.length(name.len())?;
        self.0.update(name.as_bytes());
        Ok(())
    }
    fn variant(
        &mut self,
        tag: u8,
        name: &str,
        index: u32,
        variant: &str,
    ) -> Result<(), IdentityError> {
        self.tag(tag);
        self.name(name)?;
        self.0.update(&index.to_le_bytes());
        self.name(variant)
    }
}

macro_rules! integer {
    ($method:ident, $ty:ty, $tag:expr) => {
        fn $method(self, value: $ty) -> Result<(), IdentityError> {
            self.tag($tag);
            self.0.update(&value.to_le_bytes());
            Ok(())
        }
    };
}

impl<'a> Serializer for &'a mut Canonical {
    type Ok = ();
    type Error = IdentityError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = ser::Impossible<(), IdentityError>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, value: bool) -> Result<(), IdentityError> {
        self.tag(0x01);
        self.tag(u8::from(value));
        Ok(())
    }
    integer!(serialize_i8, i8, 0x02);
    integer!(serialize_i16, i16, 0x03);
    integer!(serialize_i32, i32, 0x04);
    integer!(serialize_i64, i64, 0x05);
    integer!(serialize_i128, i128, 0x06);
    integer!(serialize_u8, u8, 0x10);
    integer!(serialize_u16, u16, 0x11);
    integer!(serialize_u32, u32, 0x12);
    integer!(serialize_u64, u64, 0x13);
    integer!(serialize_u128, u128, 0x14);

    fn serialize_f32(self, _: f32) -> Result<(), IdentityError> {
        Err(IdentityError::UnsupportedRepresentation("f32"))
    }
    fn serialize_f64(self, _: f64) -> Result<(), IdentityError> {
        Err(IdentityError::UnsupportedRepresentation("f64"))
    }
    fn serialize_char(self, value: char) -> Result<(), IdentityError> {
        self.tag(0x20);
        self.0.update(&u32::from(value).to_le_bytes());
        Ok(())
    }
    fn serialize_str(self, value: &str) -> Result<(), IdentityError> {
        self.name(value)
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<(), IdentityError> {
        self.tag(0x22);
        self.length(value.len())?;
        self.0.update(value);
        Ok(())
    }
    fn serialize_none(self) -> Result<(), IdentityError> {
        self.tag(0x30);
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), IdentityError> {
        self.tag(0x31);
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), IdentityError> {
        self.tag(0x32);
        Ok(())
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<(), IdentityError> {
        self.tag(0x33);
        self.name(name)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<(), IdentityError> {
        self.variant(0x34, name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<(), IdentityError> {
        self.tag(0x35);
        self.name(name)?;
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), IdentityError> {
        self.variant(0x36, name, index, variant)?;
        value.serialize(self)
    }
    fn serialize_seq(self, length: Option<usize>) -> Result<Compound<'a>, IdentityError> {
        self.compound(
            0x40,
            length.ok_or(IdentityError::UnsupportedRepresentation(
                "unknown sequence length",
            ))?,
        )
    }
    fn serialize_tuple(self, length: usize) -> Result<Compound<'a>, IdentityError> {
        self.compound(0x41, length)
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Compound<'a>, IdentityError> {
        self.tag(0x42);
        self.name(name)?;
        self.compound(0x41, length)
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Compound<'a>, IdentityError> {
        self.variant(0x43, name, index, variant)?;
        self.compound(0x41, length)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, IdentityError> {
        Err(IdentityError::UnsupportedRepresentation("map"))
    }
    fn serialize_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Compound<'a>, IdentityError> {
        self.tag(0x44);
        self.name(name)?;
        self.compound(0x41, length)
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Compound<'a>, IdentityError> {
        self.variant(0x45, name, index, variant)?;
        self.compound(0x41, length)
    }
    fn collect_str<T: fmt::Display + ?Sized>(self, _: &T) -> Result<(), IdentityError> {
        Err(IdentityError::UnsupportedRepresentation("display string"))
    }
    fn is_human_readable(&self) -> bool {
        false
    }
}

struct Compound<'a> {
    serializer: &'a mut Canonical,
    remaining: usize,
}
impl Compound<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), IdentityError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(IdentityError::InvalidLength)?;
        self.serializer.tag(0xfd);
        value.serialize(&mut *self.serializer)
    }
    fn field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), IdentityError> {
        self.serializer.tag(0xfe);
        self.serializer.name(key)?;
        self.element(value)
    }
    fn finish(self) -> Result<(), IdentityError> {
        if self.remaining != 0 {
            return Err(IdentityError::InvalidLength);
        }
        self.serializer.tag(0xff);
        Ok(())
    }
}
macro_rules! elements {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Compound<'_> {
            type Ok = ();
            type Error = IdentityError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), IdentityError> {
                self.element(value)
            }
            fn end(self) -> Result<(), IdentityError> {
                self.finish()
            }
        }
    };
}
elements!(SerializeSeq, serialize_element);
elements!(SerializeTuple, serialize_element);
elements!(SerializeTupleStruct, serialize_field);
elements!(SerializeTupleVariant, serialize_field);
macro_rules! fields {
    ($trait:ident) => {
        impl ser::$trait for Compound<'_> {
            type Ok = ();
            type Error = IdentityError;
            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result<(), IdentityError> {
                self.field(key, value)
            }
            fn end(self) -> Result<(), IdentityError> {
                self.finish()
            }
        }
    };
}
fields!(SerializeStruct);
fields!(SerializeStructVariant);
