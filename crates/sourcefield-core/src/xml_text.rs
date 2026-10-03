//! Allocation-free XML 1.0 text admission for every serialized field of public models.

use serde::Serialize;
use serde::ser::{self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant};

use crate::ValidationError;

/// Validate strings through their serde representation without allocating a second object tree.
/// This includes newly added fields and map keys, unlike a hand-maintained list of text fields.
pub(crate) fn validate<T: Serialize>(value: &T) -> Result<(), ValidationError> {
    value.serialize(TextValidator)
}

struct TextValidator;

impl ser::Error for ValidationError {
    fn custom<T: std::fmt::Display>(_message: T) -> Self {
        Self::InvalidValue("text serialization failed".into())
    }
}

impl ser::Serializer for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_str(self, value: &str) -> Result<(), ValidationError> {
        // XML 1.0 Fifth Edition Char allows tab/LF/CR but excludes other C0 controls and
        // U+FFFE/U+FFFF. Rust strings already exclude surrogate code points.
        // UTF-8 continuation bytes cannot encode C0 controls. Scan ASCII bytes directly,
        // then search the only forbidden scalar values above that range without decoding
        // every valid Unicode scalar in every repeatedly validated model field.
        if value
            .bytes()
            .any(|byte| byte < 0x20 && !matches!(byte, 9 | 10 | 13))
            || value.contains('\u{fffe}')
            || value.contains('\u{ffff}')
        {
            return Err(ValidationError::InvalidValue(
                "XML 1.0 text character".into(),
            ));
        }

        Ok(())
    }

    fn serialize_char(self, value: char) -> Result<(), ValidationError> {
        self.serialize_str(value.encode_utf8(&mut [0; 4]))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), ValidationError> {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), ValidationError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), ValidationError> {
        value.serialize(self)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, ValidationError> {
        Ok(self)
    }
    fn serialize_unit(self) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_none(self) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_bool(self, _: bool) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_i8(self, _: i8) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_i16(self, _: i16) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_i32(self, _: i32) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_i64(self, _: i64) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_u8(self, _: u8) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_u16(self, _: u16) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_u32(self, _: u32) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_u64(self, _: u64) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_f32(self, _: f32) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_f64(self, _: f64) -> Result<(), ValidationError> {
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl SerializeSeq for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_element<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl ser::SerializeTuple for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_element<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl ser::SerializeTupleStruct for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl ser::SerializeTupleVariant for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl SerializeMap for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl SerializeStruct for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl SerializeStructVariant for TextValidator {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), ValidationError> {
        value.serialize(TextValidator)
    }
    fn end(self) -> Result<(), ValidationError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn utf8_scan_matches_xml_10_char_production_for_every_unicode_scalar() {
        // Arrange
        let scalars = (0..=0x10ffff).filter_map(char::from_u32);

        // Act
        let mismatches = scalars
            .filter(|character| {
                let specified = matches!(*character,
                '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}'
                | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}');
                super::validate(character).is_ok() != specified
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(
            mismatches.is_empty(),
            "XML 1.0 character mismatch: {mismatches:?}"
        );
    }
}
