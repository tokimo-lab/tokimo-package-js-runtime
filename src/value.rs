use std::collections::BTreeMap;

use rquickjs::{Ctx, FromJs, IntoJs, Object, Value};
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
    pub fn to_rust<T: for<'de> Deserialize<'de>>(&self) -> Result<T, JsError> {
        let json = serde_json::to_string(self).map_err(|e| JsError::TypeConversion(e.to_string()))?;
        serde_json::from_str(&json).map_err(|e| JsError::TypeConversion(e.to_string()))
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
