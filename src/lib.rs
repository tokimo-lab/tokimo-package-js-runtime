//! # tokimo-package-js-runtime
//!
//! A QuickJS JavaScript runtime wrapper for Rust with:
//! - Sync/async JS execution
//! - Rust function injection (sync and async)
//! - JS value export and parsing via serde

mod error;
mod runtime;
mod value;

pub use error::JsError;
pub use runtime::{AsyncJsRuntime, JsRuntime};
pub use value::JsValue;

/// Result type alias for this crate.
pub type JsResult<T> = Result<T, JsError>;
