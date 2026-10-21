mod child;
mod map;
mod plain;
mod reader;
mod seq;
mod var;

use self::{
    child::ChildDeserializer,
    map::MapAccess,
    plain::PlainTextDeserializer,
    reader::{Event, RootReader},
    var::EnumAccess,
};
use crate::{
    config::SerdeXml,
    error::{Error, Result},
};
use log::trace;
use reader::Reader;
use serde::{de::Visitor, Deserialize};
use std::io::Read;
use xml::EventReader;

/// Tracks how deeply nested the deserializer currently is, so that deeply
/// nested XML cannot overflow the stack.
///
/// Each nested element consumes a stack frame while deserializing. `Depth` is
/// a cheap `Copy` token threaded through the deserializer; a new child element
/// calls [`Depth::descend`] which fails with [`Error::TooDeep`] once the
/// configured limit is exceeded instead of recursing further. Modelled on
/// serde_yaml's `remaining_depth` and tokio-prost's `RECURSION_LIMIT`.
#[derive(Clone, Copy)]
pub(crate) struct Depth {
    current: usize,
    limit: usize,
}

impl Depth {
    pub(crate) fn new(limit: usize) -> Self {
        Self { current: 0, limit }
    }

    /// Returns a `Depth` one level deeper, or [`Error::TooDeep`] if that would
    /// exceed the configured limit.
    pub(crate) fn descend(self) -> Result<Self> {
        let current = self.current + 1;
        if current > self.limit {
            Err(Error::TooDeep(self.limit))
        } else {
            Ok(Self {
                current,
                limit: self.limit,
            })
        }
    }
}

/// A convenience method for deserialize some object from a string.
///
/// ```rust
/// # use serde::{Deserialize, Serialize};
/// # use serde_xml_rs::from_str;
/// #[derive(Debug, Deserialize, PartialEq)]
/// struct Item {
///     name: String,
///     source: String,
/// }
/// # fn main() {
/// let s = r##"<item><name>hello</name><source>world.rs</source></item>"##;
/// let item: Item = from_str(s).unwrap();
/// assert_eq!(item, Item { name: "hello".to_string(),source: "world.rs".to_string()});
/// # }
/// ```
pub fn from_str<'de, T: Deserialize<'de>>(s: &str) -> Result<T> {
    from_reader(s.as_bytes())
}

/// A convenience method for deserialize some object from a reader.
///
/// ```rust
/// # use serde::Deserialize;
/// # use serde_xml_rs::from_reader;
/// #[derive(Debug, Deserialize, PartialEq)]
/// struct Item {
///     name: String,
///     source: String,
/// }
/// # fn main() {
/// let s = r##"<item><name>hello</name><source>world.rs</source></item>"##;
/// let item: Item = from_reader(s.as_bytes()).unwrap();
/// assert_eq!(item, Item { name: "hello".to_string(),source: "world.rs".to_string()});
/// # }
/// ```
pub fn from_reader<'de, T: Deserialize<'de>, R: Read>(reader: R) -> Result<T> {
    T::deserialize(&mut Deserializer::from_config(SerdeXml::default(), reader))
}

pub struct Deserializer<R: Read> {
    reader: RootReader<R>,
    depth: Depth,
}

impl<R: Read> Deserializer<R> {
    pub fn new(reader: EventReader<R>) -> Self {
        Self {
            reader: RootReader::new(reader, false),
            depth: Depth::new(SerdeXml::default().nesting_limit),
        }
    }

    pub fn new_from_reader(reader: R) -> Self {
        Self::from_config(SerdeXml::default(), reader)
    }

    pub fn from_config(config: SerdeXml, source: R) -> Self {
        let depth = Depth::new(config.nesting_limit);
        Self {
            reader: RootReader::new(
                config.parser.create_reader(source),
                config.overlapping_sequences,
            ),
            depth,
        }
    }
}

macro_rules! deserialize_type {
    ($deserialize:ident => $visit:ident) => {
        fn $deserialize<V: ::serde::de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
            self.reader.start_element()?;
            let value = self.reader.chars()?.parse()?;
            self.reader.end_element()?;
            visitor.$visit(value)
        }
    };
}

impl<'de, R: Read> serde::Deserializer<'de> for &mut Deserializer<R> {
    type Error = Error;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("deserialize any"))
    }

    deserialize_type!(deserialize_bool => visit_bool);
    deserialize_type!(deserialize_i8 => visit_i8);
    deserialize_type!(deserialize_i16 => visit_i16);
    deserialize_type!(deserialize_i32 => visit_i32);
    deserialize_type!(deserialize_i64 => visit_i64);
    deserialize_type!(deserialize_u8 => visit_u8);
    deserialize_type!(deserialize_u16 => visit_u16);
    deserialize_type!(deserialize_u32 => visit_u32);
    deserialize_type!(deserialize_u64 => visit_u64);
    deserialize_type!(deserialize_f32 => visit_f32);
    deserialize_type!(deserialize_f64 => visit_f64);

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.reader.start_element()?;
        let text = if matches!(self.reader.peek()?, Event::Text(_)) {
            self.reader.chars()?
        } else {
            "".to_string()
        };
        let value = visitor.visit_string::<Self::Error>(text)?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("bytes"))
    }

    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("byte buf"))
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.reader.peek()? {
            Event::EndElement => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root unit struct '{name}'");
        self.reader.start_element()?;
        let value = visitor.visit_unit::<Self::Error>()?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_newtype_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root newtype struct '{name}'");
        self.reader.start_element()?;
        let value = visitor
            .visit_newtype_struct(ChildDeserializer::new(self.reader.child(), self.depth))?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::Unsupported("sequence in document root"))
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root tuple");
        self.reader.start_element()?;
        let text = self.reader.chars()?;
        let value = visitor.visit_seq(PlainTextDeserializer::new(&text))?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_tuple_struct<V>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root tuple struct '{name}'");
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root map");
        self.reader.start_element()?;
        let value = visitor.visit_map(MapAccess::new_map(self.reader.child(), self.depth))?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root struct '{name}'");
        let element = self.reader.start_element()?;
        let value = visitor.visit_map(MapAccess::new_struct(
            self.reader.child(),
            element.attributes,
            fields,
            self.depth,
        ))?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        trace!("Root enum '{name}'");
        self.reader.start_element()?;
        let value = visitor.visit_enum(EnumAccess::new(self.reader.child(), self.depth))?;
        self.reader.end_element()?;
        Ok(value)
    }

    fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }
}
