use serde::{
    Deserialize, Deserializer,
    __private::{
        de::missing_field, fmt, Err, Formatter, None, Ok, Option, PhantomData, Result, Some,
    },
    de::{self, Error, IgnoredAny, MapAccess, SeqAccess},
};

use crate::{
    LanguageInformation, ServerErrorMessage, TranslatableTextList, TranslatedText,
    TranslatedTextList, UsageInformation,
};

#[automatically_derived]
impl<'de> Deserialize<'de> for UsageInformation {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        enum Field {
            F0,
            F1,
            Ignore,
        }
        struct FieldVisitor;

        impl<'de> de::Visitor<'de> for FieldVisitor {
            type Value = Field;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "field identifier")
            }
            fn visit_u64<E: Error>(self, val: u64) -> Result<Self::Value, E> {
                match val {
                    0u64 => Ok(Field::F0),
                    1u64 => Ok(Field::F1),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_str<E: Error>(self, val: &str) -> Result<Self::Value, E> {
                match val {
                    "character_limit" => Ok(Field::F0),
                    "character_count" => Ok(Field::F1),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_bytes<E: Error>(self, val: &[u8]) -> Result<Self::Value, E> {
                match val {
                    b"character_limit" => Ok(Field::F0),
                    b"character_count" => Ok(Field::F1),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Deserializer::deserialize_identifier(de, FieldVisitor)
            }
        }
        struct Visitor<'de> {
            marker: PhantomData<UsageInformation>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> de::Visitor<'de> for Visitor<'de> {
            type Value = UsageInformation;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "struct UsageInformation")
            }
            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let f0 = match SeqAccess::next_element::<u64>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            0,
                            &"struct UsageInformation with 2 elements",
                        ));
                    }
                };
                let f1 = match SeqAccess::next_element::<u64>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            1,
                            &"struct UsageInformation with 2 elements",
                        ));
                    }
                };
                Ok(UsageInformation {
                    character_limit: f0,
                    character_count: f1,
                })
            }
            #[inline]
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut f0: Option<u64> = None;
                let mut f1: Option<u64> = None;
                while let Some(key) = MapAccess::next_key::<Field>(&mut m)? {
                    match key {
                        Field::F0 => {
                            if Option::is_some(&f0) {
                                return Err(<A::Error as Error>::duplicate_field(
                                    "character_limit",
                                ));
                            }
                            f0 = Some(MapAccess::next_value::<u64>(&mut m)?);
                        }
                        Field::F1 => {
                            if Option::is_some(&f1) {
                                return Err(<A::Error as Error>::duplicate_field(
                                    "character_count",
                                ));
                            }
                            f1 = Some(MapAccess::next_value::<u64>(&mut m)?);
                        }
                        _ => {
                            let _ = MapAccess::next_value::<IgnoredAny>(&mut m)?;
                        }
                    }
                }
                let f0 = match f0 {
                    Some(f) => f,
                    None => missing_field("character_limit")?,
                };
                let f1 = match f1 {
                    Some(f) => f,
                    None => missing_field("character_count")?,
                };
                Ok(UsageInformation {
                    character_limit: f0,
                    character_count: f1,
                })
            }
        }
        const FIELDS: &[&str] = &["character_limit", "character_count"];

        Deserializer::deserialize_struct(
            de,
            "UsageInformation",
            FIELDS,
            Visitor {
                marker: PhantomData::<UsageInformation>,
                lifetime: PhantomData,
            },
        )
    }
}
impl<'de> serde::Deserialize<'de> for LanguageInformation {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        enum Field {
            F0,
            F1,
            Ignore,
        }
        struct FieldVisitor;

        impl<'de> de::Visitor<'de> for FieldVisitor {
            type Value = Field;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "field identifier")
            }
            fn visit_u64<E: Error>(self, val: u64) -> Result<Self::Value, E> {
                match val {
                    0 => Ok(Field::F0),
                    1 => Ok(Field::F1),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_str<E: Error>(self, val: &str) -> Result<Self::Value, E> {
                match val {
                    "language" => Ok(Field::F0),
                    "name" => Ok(Field::F1),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_bytes<E: Error>(self, val: &[u8]) -> Result<Self::Value, E> {
                match val {
                    b"language" => Ok(Field::F0),
                    b"name" => Ok(Field::F1),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Deserializer::deserialize_identifier(de, FieldVisitor)
            }
        }
        struct Visitor<'de> {
            marker: PhantomData<LanguageInformation>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> de::Visitor<'de> for Visitor<'de> {
            type Value = LanguageInformation;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "struct LanguageInformation")
            }
            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let f0 = match SeqAccess::next_element::<String>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            0usize,
                            &"struct LanguageInformation with 2 elements",
                        ));
                    }
                };
                let f1 = match SeqAccess::next_element::<String>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            1usize,
                            &"struct LanguageInformation with 2 elements",
                        ));
                    }
                };
                Ok(LanguageInformation {
                    language: f0,
                    name: f1,
                })
            }
            #[inline]
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut f0: Option<String> = None;
                let mut f1: Option<String> = None;
                while let Some(__key) = MapAccess::next_key::<Field>(&mut m)? {
                    match __key {
                        Field::F0 => {
                            if Option::is_some(&f0) {
                                return Err(<A::Error as Error>::duplicate_field("language"));
                            }
                            f0 = Some(MapAccess::next_value::<String>(&mut m)?);
                        }
                        Field::F1 => {
                            if Option::is_some(&f1) {
                                return Err(<A::Error as Error>::duplicate_field("name"));
                            }
                            f1 = Some(MapAccess::next_value::<String>(&mut m)?);
                        }
                        _ => {
                            let _ = MapAccess::next_value::<IgnoredAny>(&mut m)?;
                        }
                    }
                }
                let f0 = match f0 {
                    Some(f) => f,
                    None => missing_field("language")?,
                };
                let f1 = match f1 {
                    Some(f) => f,
                    None => missing_field("name")?,
                };
                Ok(LanguageInformation {
                    language: f0,
                    name: f1,
                })
            }
        }
        const FIELDS: &[&str] = &["language", "name"];
        Deserializer::deserialize_struct(
            de,
            "LanguageInformation",
            FIELDS,
            Visitor {
                marker: PhantomData::<LanguageInformation>,
                lifetime: PhantomData,
            },
        )
    }
}

impl<'de> serde::Deserialize<'de> for TranslatableTextList {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        enum Field {
            Field0,
            Field1,
            Field2,
            Ignore,
        }
        struct FieldVisitor;

        impl<'de> de::Visitor<'de> for FieldVisitor {
            type Value = Field;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "field identifier")
            }
            fn visit_u64<E: Error>(self, val: u64) -> Result<Self::Value, E> {
                match val {
                    0 => Ok(Field::Field0),
                    1 => Ok(Field::Field1),
                    2 => Ok(Field::Field2),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_str<E: Error>(self, val: &str) -> Result<Self::Value, E> {
                match val {
                    "source_language" => Ok(Field::Field0),
                    "target_language" => Ok(Field::Field1),
                    "texts" => Ok(Field::Field2),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_bytes<E: Error>(self, val: &[u8]) -> Result<Self::Value, E> {
                match val {
                    b"source_language" => Ok(Field::Field0),
                    b"target_language" => Ok(Field::Field1),
                    b"texts" => Ok(Field::Field2),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Deserializer::deserialize_identifier(de, FieldVisitor)
            }
        }
        struct Visitor<'de> {
            marker: PhantomData<TranslatableTextList>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> de::Visitor<'de> for Visitor<'de> {
            type Value = TranslatableTextList;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "struct TranslatableTextList")
            }
            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let f0 = match SeqAccess::next_element::<Option<String>>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            0usize,
                            &"struct TranslatableTextList with 3 elements",
                        ));
                    }
                };
                let f1 = match SeqAccess::next_element::<String>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            1usize,
                            &"struct TranslatableTextList with 3 elements",
                        ));
                    }
                };
                let f2 = match SeqAccess::next_element::<Vec<String>>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            2usize,
                            &"struct TranslatableTextList with 3 elements",
                        ));
                    }
                };
                Ok(TranslatableTextList {
                    source_language: f0,
                    target_language: f1,
                    texts: f2,
                })
            }
            #[inline]
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut f0: Option<Option<String>> = None;
                let mut f1: Option<String> = None;
                let mut f2: Option<Vec<String>> = None;

                while let Some(key) = MapAccess::next_key::<Field>(&mut m)? {
                    match key {
                        Field::Field0 => {
                            if Option::is_some(&f0) {
                                return Err(<A::Error as Error>::duplicate_field(
                                    "source_language",
                                ));
                            }
                            f0 = Some(MapAccess::next_value::<Option<String>>(&mut m)?);
                        }
                        Field::Field1 => {
                            if Option::is_some(&f1) {
                                return Err(<A::Error as Error>::duplicate_field(
                                    "target_language",
                                ));
                            }
                            f1 = Some(MapAccess::next_value::<String>(&mut m)?);
                        }
                        Field::Field2 => {
                            if Option::is_some(&f2) {
                                return Err(<A::Error as Error>::duplicate_field("texts"));
                            }
                            f2 = Some(MapAccess::next_value::<Vec<String>>(&mut m)?);
                        }
                        _ => {
                            let _ = MapAccess::next_value::<IgnoredAny>(&mut m)?;
                        }
                    }
                }
                let f0 = match f0 {
                    Some(f) => f,
                    None => missing_field("source_language")?,
                };
                let f1 = match f1 {
                    Some(f) => f,
                    None => missing_field("target_language")?,
                };
                let f2 = match f2 {
                    Some(f) => f,
                    None => missing_field("texts")?,
                };
                Ok(TranslatableTextList {
                    source_language: f0,
                    target_language: f1,
                    texts: f2,
                })
            }
        }
        const FIELDS: &[&str] = &["source_language", "target_language", "texts"];
        Deserializer::deserialize_struct(
            de,
            "TranslatableTextList",
            FIELDS,
            Visitor {
                marker: PhantomData::<TranslatableTextList>,
                lifetime: PhantomData,
            },
        )
    }
}

impl<'de> serde::Deserialize<'de> for TranslatedText {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        enum Field {
            Field0,
            Field1,
            Ignore,
        }
        struct FieldVisitor;

        impl<'de> serde::de::Visitor<'de> for FieldVisitor {
            type Value = Field;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "field identifier")
            }
            fn visit_u64<E: Error>(self, val: u64) -> Result<Self::Value, E> {
                match val {
                    0 => Ok(Field::Field0),
                    1 => Ok(Field::Field1),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_str<E: Error>(self, __value: &str) -> Result<Self::Value, E> {
                match __value {
                    "detected_source_language" => Ok(Field::Field0),
                    "text" => Ok(Field::Field1),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_bytes<E: Error>(self, val: &[u8]) -> Result<Self::Value, E> {
                match val {
                    b"detected_source_language" => Ok(Field::Field0),
                    b"text" => Ok(Field::Field1),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Deserializer::deserialize_identifier(de, FieldVisitor)
            }
        }
        struct Visitor<'de> {
            marker: PhantomData<TranslatedText>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> serde::de::Visitor<'de> for Visitor<'de> {
            type Value = TranslatedText;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "struct TranslatedText")
            }
            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let f0 = match SeqAccess::next_element::<String>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            0,
                            &"struct TranslatedText with 2 elements",
                        ));
                    }
                };
                let f1 = match SeqAccess::next_element::<String>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            1,
                            &"struct TranslatedText with 2 elements",
                        ));
                    }
                };
                Ok(TranslatedText {
                    detected_source_language: f0,
                    text: f1,
                })
            }
            #[inline]
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut f0: Option<String> = None;
                let mut f1: Option<String> = None;
                while let Some(key) = MapAccess::next_key::<Field>(&mut m)? {
                    match key {
                        Field::Field0 => {
                            if Option::is_some(&f0) {
                                return Err(<A::Error as Error>::duplicate_field(
                                    "detected_source_language",
                                ));
                            }
                            f0 = Some(MapAccess::next_value::<String>(&mut m)?);
                        }
                        Field::Field1 => {
                            if Option::is_some(&f1) {
                                return Err(<A::Error as Error>::duplicate_field("text"));
                            }
                            f1 = Some(MapAccess::next_value::<String>(&mut m)?);
                        }
                        _ => {
                            let _ = MapAccess::next_value::<IgnoredAny>(&mut m)?;
                        }
                    }
                }
                let f0 = match f0 {
                    Some(f) => f,
                    None => missing_field("detected_source_language")?,
                };
                let f1 = match f1 {
                    Some(f) => f,
                    None => missing_field("text")?,
                };
                Ok(TranslatedText {
                    detected_source_language: f0,
                    text: f1,
                })
            }
        }
        const FIELDS: &[&str] = &["detected_source_language", "text"];
        Deserializer::deserialize_struct(
            de,
            "TranslatedText",
            FIELDS,
            Visitor {
                marker: PhantomData::<TranslatedText>,
                lifetime: PhantomData,
            },
        )
    }
}

impl<'de> serde::Deserialize<'de> for TranslatedTextList {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        enum Field {
            Field0,
            Ignore,
        }
        struct FieldVisitor;

        impl<'de> serde::de::Visitor<'de> for FieldVisitor {
            type Value = Field;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "field identifier")
            }
            fn visit_u64<E: Error>(self, val: u64) -> Result<Self::Value, E> {
                match val {
                    0 => Ok(Field::Field0),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_str<E: Error>(self, val: &str) -> Result<Self::Value, E> {
                match val {
                    "translations" => Ok(Field::Field0),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_bytes<E: Error>(self, val: &[u8]) -> Result<Self::Value, E> {
                match val {
                    b"translations" => Ok(Field::Field0),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Deserializer::deserialize_identifier(de, FieldVisitor)
            }
        }
        struct Visitor<'de> {
            marker: PhantomData<TranslatedTextList>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> serde::de::Visitor<'de> for Visitor<'de> {
            type Value = TranslatedTextList;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "struct TranslatedTextList")
            }
            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let f0 = match SeqAccess::next_element::<Vec<TranslatedText>>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            0,
                            &"struct TranslatedTextList with 1 element",
                        ));
                    }
                };
                Ok(TranslatedTextList { translations: f0 })
            }
            #[inline]
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut f0: Option<Vec<TranslatedText>> = None;
                while let Some(key) = MapAccess::next_key::<Field>(&mut m)? {
                    match key {
                        Field::Field0 => {
                            if Option::is_some(&f0) {
                                return Err(<A::Error as Error>::duplicate_field("translations"));
                            }
                            f0 = Some(MapAccess::next_value::<Vec<TranslatedText>>(&mut m)?);
                        }
                        _ => {
                            let _ = MapAccess::next_value::<IgnoredAny>(&mut m)?;
                        }
                    }
                }
                let f0 = match f0 {
                    Some(f) => f,
                    None => missing_field("translations")?,
                };
                Ok(TranslatedTextList { translations: f0 })
            }
        }
        const FIELDS: &[&str] = &["translations"];
        Deserializer::deserialize_struct(
            de,
            "TranslatedTextList",
            FIELDS,
            Visitor {
                marker: PhantomData::<TranslatedTextList>,
                lifetime: PhantomData,
            },
        )
    }
}

impl<'de> serde::Deserialize<'de> for ServerErrorMessage {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        enum Field {
            Field0,
            Ignore,
        }
        struct FieldVisitor;

        impl<'de> serde::de::Visitor<'de> for FieldVisitor {
            type Value = Field;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "field identifier")
            }
            fn visit_u64<E: Error>(self, val: u64) -> Result<Self::Value, E> {
                match val {
                    0 => Ok(Field::Field0),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_str<E: Error>(self, val: &str) -> Result<Self::Value, E> {
                match val {
                    "message" => Ok(Field::Field0),
                    _ => Ok(Field::Ignore),
                }
            }
            fn visit_bytes<E: Error>(self, val: &[u8]) -> Result<Self::Value, E> {
                match val {
                    b"message" => Ok(Field::Field0),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Deserializer::deserialize_identifier(de, FieldVisitor)
            }
        }
        struct Visitor<'de> {
            marker: PhantomData<ServerErrorMessage>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> serde::de::Visitor<'de> for Visitor<'de> {
            type Value = ServerErrorMessage;
            fn expecting(&self, fmt: &mut Formatter) -> fmt::Result {
                Formatter::write_str(fmt, "struct ServerErrorMessage")
            }
            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let f0 = match SeqAccess::next_element::<String>(&mut s)? {
                    Some(v) => v,
                    None => {
                        return Err(Error::invalid_length(
                            0,
                            &"struct ServerErrorMessage with 1 element",
                        ));
                    }
                };
                Ok(ServerErrorMessage { message: f0 })
            }
            #[inline]
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut f0: Option<String> = None;
                while let Some(key) = MapAccess::next_key::<Field>(&mut m)? {
                    match key {
                        Field::Field0 => {
                            if Option::is_some(&f0) {
                                return Err(<A::Error as Error>::duplicate_field("message"));
                            }
                            f0 = Some(MapAccess::next_value::<String>(&mut m)?);
                        }
                        _ => {
                            let _ = MapAccess::next_value::<IgnoredAny>(&mut m)?;
                        }
                    }
                }
                let f0 = match f0 {
                    Some(f) => f,
                    None => missing_field("message")?,
                };
                Ok(ServerErrorMessage { message: f0 })
            }
        }
        const FIELDS: &[&str] = &["message"];
        Deserializer::deserialize_struct(
            de,
            "ServerErrorMessage",
            FIELDS,
            Visitor {
                marker: PhantomData::<ServerErrorMessage>,
                lifetime: PhantomData,
            },
        )
    }
}
