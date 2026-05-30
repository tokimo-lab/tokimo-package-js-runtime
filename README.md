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

let mut rt = JsRuntime::new();
let result: i64 = rt.eval_as("1 + 2").unwrap();
assert_eq!(result, 3);
```

### Async

```rust
use tokimo_package_js_runtime::AsyncJsRuntime;

let rt = AsyncJsRuntime::new().await;
let result: String = rt.eval_as("'hello' + ' world'").await.unwrap();
assert_eq!(result, "hello world");
```

### Function Injection

```rust
use tokimo_package_js_runtime::JsRuntime;

let mut rt = JsRuntime::new();
rt.register_fn("double", |x: i64| x * 2);
let result: i64 = rt.eval_as("double(21)").unwrap();
assert_eq!(result, 42);
```

### Export to Custom Structs

```rust
use serde::Deserialize;
use tokimo_package_js_runtime::JsRuntime;

#[derive(Deserialize, Debug, PartialEq)]
struct Point { x: f64, y: f64 }

let mut rt = JsRuntime::new();
rt.register_function("make_point", |_args| {
    tokimo_package_js_runtime::JsValue::Object(std::collections::BTreeMap::from([
        ("x".into(), tokimo_package_js_runtime::JsValue::Float(3.0)),
        ("y".into(), tokimo_package_js_runtime::JsValue::Float(4.0)),
    ]))
});
let p: Point = rt.eval_as("make_point()").unwrap();
assert_eq!(p, Point { x: 3.0, y: 4.0 });
```

## Architecture

| Type | Description |
|---|---|
| `JsRuntime` | Synchronous QuickJS wrapper — `eval`, `eval_as`, `register_fn` |
| `AsyncJsRuntime` | Async wrapper — dedicated OS thread, auto-await Promises |
| `JsValue` | Enum bridging Rust ↔ JS values (`Bool`, `Int`, `Float`, `String`, `Array`, `Object`, …) |
| `JsError` | Error type with `QuickJs`, `TypeConversion`, `Channel` variants |

## Dependencies

| Crate | Role |
|---|---|
| [rquickjs](https://crates.io/crates/rquickjs) | QuickJS engine bindings |
| serde / serde_json | Value serialization across the boundary |
| tokio | Async runtime for the worker thread |
| thiserror | Error derive macros |

## Testing

```bash
cargo test
```

25 integration tests covering basic eval, function injection, value export, and async execution.

## CI

Multi-platform (Linux, macOS, Windows) via GitHub Actions: fmt, clippy, test, build.

## License

MIT
