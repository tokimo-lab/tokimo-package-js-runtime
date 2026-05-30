//! # tokimo-package-js-runtime
//!
//! A QuickJS JavaScript runtime wrapper for Rust with:
//! - Sync and async JS execution (the async runtime supports top-level await)
//! - Rust function injection into the JS global scope
//! - JS value export and parsing via serde

mod error;
mod runtime;
mod value;

pub use error::JsError;
pub use runtime::{AsyncJsRuntime, JsRuntime};
pub use value::JsValue;

/// Re-exported rquickjs helpers for building functions to register with
/// [`JsRuntime::register_fn`] / [`JsRuntime::set_global`] (and their
/// [`AsyncJsRuntime`] equivalents). Wrap a future-returning closure in
/// [`Async`] to inject a function that JavaScript can `await`.
pub use rquickjs::function::{Async, Func, MutFn, OnceFn};

/// Result type alias for this crate.
pub type JsResult<T> = Result<T, JsError>;
