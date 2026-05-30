use std::collections::BTreeMap;

use rquickjs::{Ctx, FromJs, IntoJs, Object, Value};
use serde::de::{self, DeserializeOwned, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::JsError;

/// Wrapper for values crossing the Rust/JS boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<JsValue>),
    Object(BTreeMap<String, JsValue>),
}

impl JsValue {
    /// Convert to a Rust type using serde deserialization.
    ///
    /// Deserializes directly from the in-memory value (no JSON string
    /// round-trip), so `NaN`/`Infinity` survive and integer-valued floats
    /// (how QuickJS represents numbers larger than `i32`) deserialize into
    /// integer targets.
    pub fn to_rust<T: DeserializeOwned>(&self) -> Result<T, JsError> {
        T::deserialize(self)
    }

    /// Build a `JsValue` from any serde-serializable Rust value.
    ///
    /// Serializes directly into the in-memory value tree (no JSON string
    /// round-trip). Handy for injecting a struct or map as a JS global, e.g.
    /// `set_global("args", JsValue::from_rust(&args)?)`.
    pub fn from_rust<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
        value.serialize(JsValueSerializer)
    }
}

macro_rules! deserialize_int {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
            match self {
                JsValue::Int(i) => visitor.$visit(*i as $ty),
                JsValue::Float(f) if f.is_finite() && f.fract() == 0.0 => visitor.$visit(*f as $ty),
                _ => self.deserialize_any(visitor),
            }
        }
    };
}

impl<'de> Deserializer<'de> for &'de JsValue {
    type Error = JsError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self {
            JsValue::Undefined | JsValue::Null => visitor.visit_unit(),
            JsValue::Bool(b) => visitor.visit_bool(*b),
            JsValue::Int(i) => visitor.visit_i64(*i),
            JsValue::Float(f) => visitor.visit_f64(*f),
            JsValue::String(s) => visitor.visit_str(s),
            JsValue::Array(arr) => visitor.visit_seq(SeqDeserializer { iter: arr.iter() }),
            JsValue::Object(map) => visitor.visit_map(MapDeserializer {
                iter: map.iter(),
                value: None,
            }),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self {
            JsValue::Undefined | JsValue::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    deserialize_int!(deserialize_i8, visit_i8, i8);
    deserialize_int!(deserialize_i16, visit_i16, i16);
    deserialize_int!(deserialize_i32, visit_i32, i32);
    deserialize_int!(deserialize_i64, visit_i64, i64);
    deserialize_int!(deserialize_u8, visit_u8, u8);
    deserialize_int!(deserialize_u16, visit_u16, u16);
    deserialize_int!(deserialize_u32, visit_u32, u32);
    deserialize_int!(deserialize_u64, visit_u64, u64);

    serde::forward_to_deserialize_any! {
        bool f32 f64 char str string bytes byte_buf unit unit_struct
        newtype_struct seq tuple tuple_struct map struct enum identifier
        ignored_any
    }
}

struct SeqDeserializer<'de> {
    iter: std::slice::Iter<'de, JsValue>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'de> {
    type Error = JsError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, JsError>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(v) => seed.deserialize(v).map(Some),
            None => Ok(None),
        }
    }
}

struct MapDeserializer<'de> {
    iter: std::collections::btree_map::Iter<'de, String, JsValue>,
    value: Option<&'de JsValue>,
}

impl<'de> MapAccess<'de> for MapDeserializer<'de> {
    type Error = JsError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, JsError>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                let key_de: de::value::StrDeserializer<'de, JsError> = k.as_str().into_deserializer();
                seed.deserialize(key_de).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, JsError>
    where
        V: de::DeserializeSeed<'de>,
    {
        let v = self.value.take().expect("next_value_seed called before next_key_seed");
        seed.deserialize(v)
    }
}

impl<'js> IntoJs<'js> for JsValue {
    fn into_js(self, ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
        match self {
            JsValue::Undefined => Ok(Value::new_undefined(ctx.clone())),
            JsValue::Null => Ok(Value::new_null(ctx.clone())),
            JsValue::Bool(b) => b.into_js(ctx),
            JsValue::Int(i) => i.into_js(ctx),
            JsValue::Float(f) => f.into_js(ctx),
            JsValue::String(s) => s.into_js(ctx),
            JsValue::Array(arr) => {
                let arr_obj = rquickjs::Array::new(ctx.clone())?;
                for (i, item) in arr.into_iter().enumerate() {
                    arr_obj.set(i, item)?;
                }
                Ok(arr_obj.into_value())
            }
            JsValue::Object(map) => {
                let obj = Object::new(ctx.clone())?;
                for (k, v) in map {
                    obj.set(k.as_str(), v)?;
                }
                Ok(obj.into_value())
            }
        }
    }
}

impl<'js> FromJs<'js> for JsValue {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<Self> {
        match value.type_of() {
            rquickjs::Type::Undefined => Ok(JsValue::Undefined),
            rquickjs::Type::Null => Ok(JsValue::Null),
            rquickjs::Type::Bool => {
                let b = bool::from_js(ctx, value)?;
                Ok(JsValue::Bool(b))
            }
            rquickjs::Type::Int => {
                let i = i64::from_js(ctx, value)?;
                Ok(JsValue::Int(i))
            }
            rquickjs::Type::Float => {
                let f = f64::from_js(ctx, value)?;
                Ok(JsValue::Float(f))
            }
            rquickjs::Type::String => {
                let s = String::from_js(ctx, value)?;
                Ok(JsValue::String(s))
            }
            rquickjs::Type::Array => {
                let arr = value.as_array().unwrap();
                let mut items = Vec::new();
                for i in 0..arr.len() {
                    let v: Value = arr.get(i)?;
                    items.push(JsValue::from_js(ctx, v)?);
                }
                Ok(JsValue::Array(items))
            }
            rquickjs::Type::Object => {
                let obj = value.as_object().unwrap();
                let mut map = BTreeMap::new();
                for key in obj.keys::<String>() {
                    let key = key?;
                    let v: Value = obj.get(&key)?;
                    map.insert(key, JsValue::from_js(ctx, v)?);
                }
                Ok(JsValue::Object(map))
            }
            _ => Ok(JsValue::Undefined),
        }
    }
}

// ─── Serializer: any `Serialize` value → `JsValue` ─────────────────────────

use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple, SerializeTupleStruct,
    SerializeTupleVariant, Serializer,
};

struct JsValueSerializer;

impl Serializer for JsValueSerializer {
    type Ok = JsValue;
    type Error = JsError;
    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = SeqSerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = MapSerializer;
    type SerializeStruct = StructSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    fn serialize_bool(self, v: bool) -> Result<JsValue, JsError> {
        Ok(JsValue::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v as i64))
    }
    fn serialize_i16(self, v: i16) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v as i64))
    }
    fn serialize_i32(self, v: i32) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v as i64))
    }
    fn serialize_i64(self, v: i64) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v))
    }
    fn serialize_u8(self, v: u8) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v as i64))
    }
    fn serialize_u16(self, v: u16) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v as i64))
    }
    fn serialize_u32(self, v: u32) -> Result<JsValue, JsError> {
        Ok(JsValue::Int(v as i64))
    }
    fn serialize_u64(self, v: u64) -> Result<JsValue, JsError> {
        // Values beyond i64 range fall back to Float (JS numbers are f64).
        match i64::try_from(v) {
            Ok(i) => Ok(JsValue::Int(i)),
            Err(_) => Ok(JsValue::Float(v as f64)),
        }
    }
    fn serialize_f32(self, v: f32) -> Result<JsValue, JsError> {
        Ok(JsValue::Float(v as f64))
    }
    fn serialize_f64(self, v: f64) -> Result<JsValue, JsError> {
        Ok(JsValue::Float(v))
    }
    fn serialize_char(self, v: char) -> Result<JsValue, JsError> {
        Ok(JsValue::String(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<JsValue, JsError> {
        Ok(JsValue::String(v.to_owned()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<JsValue, JsError> {
        Ok(JsValue::Array(v.iter().map(|b| JsValue::Int(*b as i64)).collect()))
    }
    fn serialize_none(self) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }
    fn serialize_some<T: ?Sized + ser::Serialize>(self, value: &T) -> Result<JsValue, JsError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<JsValue, JsError> {
        Ok(JsValue::String(variant.to_owned()))
    }
    fn serialize_newtype_struct<T: ?Sized + ser::Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<JsValue, JsError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + ser::Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<JsValue, JsError> {
        let mut map = BTreeMap::new();
        map.insert(variant.to_owned(), value.serialize(JsValueSerializer)?);
        Ok(JsValue::Object(map))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer, JsError> {
        Ok(SeqSerializer {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, JsError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqSerializer, JsError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<TupleVariantSerializer, JsError> {
        Ok(TupleVariantSerializer {
            variant,
            items: Vec::with_capacity(len),
        })
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer, JsError> {
        Ok(MapSerializer {
            map: BTreeMap::new(),
            next_key: None,
        })
    }
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<StructSerializer, JsError> {
        Ok(StructSerializer { map: BTreeMap::new() })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSerializer, JsError> {
        Ok(StructVariantSerializer {
            variant,
            map: BTreeMap::new(),
        })
    }
}

struct SeqSerializer {
    items: Vec<JsValue>,
}

impl SerializeSeq for SeqSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_element<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<(), JsError> {
        self.items.push(value.serialize(JsValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<JsValue, JsError> {
        Ok(JsValue::Array(self.items))
    }
}

impl SerializeTuple for SeqSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_element<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<(), JsError> {
        SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<JsValue, JsError> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for SeqSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_field<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<(), JsError> {
        SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<JsValue, JsError> {
        SerializeSeq::end(self)
    }
}

struct TupleVariantSerializer {
    variant: &'static str,
    items: Vec<JsValue>,
}

impl SerializeTupleVariant for TupleVariantSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_field<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<(), JsError> {
        self.items.push(value.serialize(JsValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<JsValue, JsError> {
        let mut map = BTreeMap::new();
        map.insert(self.variant.to_owned(), JsValue::Array(self.items));
        Ok(JsValue::Object(map))
    }
}

struct MapSerializer {
    map: BTreeMap<String, JsValue>,
    next_key: Option<String>,
}

impl SerializeMap for MapSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_key<T: ?Sized + ser::Serialize>(&mut self, key: &T) -> Result<(), JsError> {
        self.next_key = Some(object_key(key.serialize(JsValueSerializer)?)?);
        Ok(())
    }
    fn serialize_value<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<(), JsError> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| JsError::TypeConversion("serialize_value called before serialize_key".into()))?;
        self.map.insert(key, value.serialize(JsValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<JsValue, JsError> {
        Ok(JsValue::Object(self.map))
    }
}

struct StructSerializer {
    map: BTreeMap<String, JsValue>,
}

impl SerializeStruct for StructSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_field<T: ?Sized + ser::Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), JsError> {
        self.map.insert(key.to_owned(), value.serialize(JsValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<JsValue, JsError> {
        Ok(JsValue::Object(self.map))
    }
}

struct StructVariantSerializer {
    variant: &'static str,
    map: BTreeMap<String, JsValue>,
}

impl SerializeStructVariant for StructVariantSerializer {
    type Ok = JsValue;
    type Error = JsError;
    fn serialize_field<T: ?Sized + ser::Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), JsError> {
        self.map.insert(key.to_owned(), value.serialize(JsValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<JsValue, JsError> {
        let mut outer = BTreeMap::new();
        outer.insert(self.variant.to_owned(), JsValue::Object(self.map));
        Ok(JsValue::Object(outer))
    }
}

/// JS object keys are strings; coerce a serialized key into one.
fn object_key(key: JsValue) -> Result<String, JsError> {
    match key {
        JsValue::String(s) => Ok(s),
        JsValue::Int(i) => Ok(i.to_string()),
        JsValue::Float(f) => Ok(f.to_string()),
        JsValue::Bool(b) => Ok(b.to_string()),
        other => Err(JsError::TypeConversion(format!("invalid object key type: {other:?}"))),
    }
}
