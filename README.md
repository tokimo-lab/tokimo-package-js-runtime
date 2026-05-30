# tokimo-package-js-runtime

QuickJS JavaScript runtime wrapper for Rust with async support and function injection.

## Features

- **Sync & Async execution** — evaluate JS code from Rust, with automatic Promise resolution
- **Function injection** — register Rust closures (sync/async) into the JS global scope
- **Execution timeouts** — interrupt runaway scripts (even `while (true) {}`) via `eval_with_timeout`
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

`AsyncJsRuntime` also supports function injection, including **async** Rust
functions that JavaScript can `await`:

```rust
use tokimo_package_js_runtime::{AsyncJsRuntime, Async};

let rt = AsyncJsRuntime::new().unwrap();
rt.register_fn(
    "delayedDouble",
    Async(|x: i32| async move {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        x * 2
    }),
)
.await
.unwrap();
let result: i64 = rt.eval_as("await delayedDouble(21)").await.unwrap();
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

### Timeouts

Guard against runaway scripts — `eval_with_timeout` interrupts execution once
the deadline passes, even for unyielding infinite loops. Both runtimes support
it (the async variant prevents a stuck script from hanging the worker thread):

```rust
use std::time::Duration;
use tokimo_package_js_runtime::JsRuntime;

let rt = JsRuntime::new().unwrap();
let result = rt.eval_with_timeout("while (true) {}", Duration::from_millis(100));
assert!(result.is_err()); // interrupted; the runtime stays usable afterwards
```

### Inject Global Objects

Expose Rust data to scripts by converting any `serde::Serialize` value into a
`JsValue` with `JsValue::from_rust`, then setting it as a global:

```rust
use serde::Serialize;
use tokimo_package_js_runtime::{JsRuntime, JsValue};

#[derive(Serialize)]
struct Args { xx: i64, bb: i64 }

let rt = JsRuntime::new().unwrap();
rt.set_global("args", JsValue::from_rust(&Args { xx: 11, bb: 22 }).unwrap()).unwrap();
let sum: i64 = rt.eval_as("args.xx + args.bb").unwrap();
assert_eq!(sum, 33);
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
