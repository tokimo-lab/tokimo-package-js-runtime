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
