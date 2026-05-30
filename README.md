# tokimo-package-js-runtime

QuickJS JavaScript runtime wrapper for Rust with async support and function injection.

## Features

- **Sync & Async execution** — evaluate JS code from Rust, with automatic Promise resolution
- **Function injection** — register Rust closures (sync/async) into the JS global scope
- **Serde-based type bridge** — `JsValue` enum with automatic conversion to/from Rust types via serde
- **Thread-safe async** — dedicated OS thread with isolated Tokio runtime for QuickJS (non-`Send`/`Sync` GC)

## Quick Start

```rust
use tokimo_package_js_runtime::JsRuntime;

let rt = JsRuntime::new().unwrap();
let result: i64 = rt.eval_as("1 + 2").unwrap();
assert_eq!(result, 3);
```

### Async

```rust
use tokimo_package_js_runtime::AsyncJsRuntime;

let rt = AsyncJsRuntime::new().unwrap();
// Top-level await is supported.
let result: i64 = rt.eval_as("await Promise.resolve(40) + 2").await.unwrap();
assert_eq!(result, 42);
```

### Function Injection

Register a Rust closure as a JS global — pass the closure directly:

```rust
use tokimo_package_js_runtime::JsRuntime;

let rt = JsRuntime::new().unwrap();
rt.register_fn("double", |x: i64| x * 2).unwrap();
let result: i64 = rt.eval_as("double(21)").unwrap();
assert_eq!(result, 42);
```

For closures that capture mutable state, wrap them in `MutFn`:

```rust
use tokimo_package_js_runtime::{JsRuntime, MutFn};

let rt = JsRuntime::new().unwrap();
let mut n = 0;
rt.register_fn("next", MutFn::new(move || { n += 1; n })).unwrap();
let a: i32 = rt.eval_as("next()").unwrap();
let b: i32 = rt.eval_as("next()").unwrap();
assert_eq!((a, b), (1, 2));
```

### Export to Custom Structs

A registered function can return a `JsValue`, which deserializes into any
serde type on the JS side:

```rust
use serde::Deserialize;
use std::collections::BTreeMap;
use tokimo_package_js_runtime::{JsRuntime, JsValue};

#[derive(Deserialize, Debug, PartialEq)]
struct Point { x: f64, y: f64 }

let rt = JsRuntime::new().unwrap();
rt.register_fn("make_point", || {
    JsValue::Object(BTreeMap::from([
        ("x".into(), JsValue::Float(3.0)),
        ("y".into(), JsValue::Float(4.0)),
    ]))
}).unwrap();
let p: Point = rt.eval_as("make_point()").unwrap();
assert_eq!(p, Point { x: 3.0, y: 4.0 });
```

## Architecture

| Type | Description |
|---|---|
| `JsRuntime` | Synchronous QuickJS wrapper — `eval`, `eval_as`, `register_fn`, `set_global` |
| `AsyncJsRuntime` | Async wrapper — dedicated OS thread, top-level await + auto-await Promises |
| `JsValue` | Enum bridging Rust ↔ JS values (`Bool`, `Int`, `Float`, `String`, `Array`, `Object`, …) |
| `JsError` | Error type with `QuickJs`, `TypeConversion`, `Channel` variants |
| `Func` / `Async` / `MutFn` / `OnceFn` | Re-exported rquickjs helpers for building functions to register |


## Dependencies

| Crate | Role |
|---|---|
| [rquickjs](https://crates.io/crates/rquickjs) | QuickJS engine bindings |
| serde | Value (de)serialization across the boundary |
| tokio | Async runtime for the worker thread |
| thiserror | Error derive macros |

## Testing

```bash
cargo test
```

Integration tests cover basic eval, function injection, value export
(including `NaN`/large-integer edge cases), and async execution with
top-level await.

## CI

Multi-platform (Linux, macOS, Windows) via GitHub Actions: fmt, clippy, test, build.

## License

MIT
