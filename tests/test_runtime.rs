use std::collections::BTreeMap;
use tokimo_package_js_runtime::{AsyncJsRuntime, JsRuntime, JsValue};

// ─── Basic JS Execution ────────────────────────────────────────────────────

#[test]
fn test_eval_number() {
    let rt = JsRuntime::new().unwrap();
    let result: i64 = rt.eval_as("1 + 2").unwrap();
    assert_eq!(result, 3);
}

#[test]
fn test_eval_string() {
    let rt = JsRuntime::new().unwrap();
    let result: String = rt.eval_as("'hello' + ' world'").unwrap();
    assert_eq!(result, "hello world");
}

#[test]
fn test_eval_bool() {
    let rt = JsRuntime::new().unwrap();
    let result: bool = rt.eval_as("true && false").unwrap();
    assert!(!result);
}

#[test]
fn test_eval_null() {
    let rt = JsRuntime::new().unwrap();
    let result = rt.eval("null").unwrap();
    assert!(matches!(result, JsValue::Null));
}

#[test]
fn test_eval_undefined() {
    let rt = JsRuntime::new().unwrap();
    let result = rt.eval("undefined").unwrap();
    assert!(matches!(result, JsValue::Undefined));
}

#[test]
fn test_eval_array() {
    let rt = JsRuntime::new().unwrap();
    let result: Vec<i64> = rt.eval_as("[1, 2, 3]").unwrap();
    assert_eq!(result, vec![1, 2, 3]);
}

#[test]
fn test_eval_object_as_json() {
    let rt = JsRuntime::new().unwrap();
    let result = rt.eval("({name: 'test', value: 42})").unwrap();
    match result {
        JsValue::Object(map) => {
            let name = map.get("name").unwrap();
            assert!(matches!(name, JsValue::String(s) if s == "test"));
            let value = map.get("value").unwrap();
            assert!(matches!(value, JsValue::Int(42)));
        }
        _ => panic!("Expected object"),
    }
}

#[test]
fn test_eval_js_function() {
    let rt = JsRuntime::new().unwrap();
    let result: i64 = rt.eval_as("(function() { return 42; })()").unwrap();
    assert_eq!(result, 42);
}

#[test]
fn test_eval_arrow_function() {
    let rt = JsRuntime::new().unwrap();
    let result: i64 = rt.eval_as("(() => 100)()").unwrap();
    assert_eq!(result, 100);
}

#[test]
fn test_eval_error() {
    let rt = JsRuntime::new().unwrap();
    let result = rt.eval("throw new Error('test error')");
    assert!(result.is_err());
}

// ─── Regression: number & promise edge cases ───────────────────────────────

#[test]
fn test_eval_nan_as_float() {
    let rt = JsRuntime::new().unwrap();
    let result: f64 = rt.eval_as("0/0").unwrap();
    assert!(result.is_nan());
}

#[test]
fn test_eval_infinity_as_float() {
    let rt = JsRuntime::new().unwrap();
    let result: f64 = rt.eval_as("1/0").unwrap();
    assert!(result.is_infinite() && result.is_sign_positive());
}

#[test]
fn test_eval_large_integer_as_i64() {
    let rt = JsRuntime::new().unwrap();
    // 3_000_000_000 exceeds i32, so QuickJS represents it as a float.
    let result: i64 = rt.eval_as("3000000000").unwrap();
    assert_eq!(result, 3_000_000_000);
}

#[test]
fn test_eval_integral_float_as_u64() {
    let rt = JsRuntime::new().unwrap();
    let result: u64 = rt.eval_as("2 ** 40").unwrap();
    assert_eq!(result, 1_099_511_627_776);
}

#[test]
fn test_sync_eval_promise_errors() {
    let rt = JsRuntime::new().unwrap();
    // The sync runtime has no event loop, so a Promise result must error
    // clearly rather than silently yielding `undefined`.
    let result = rt.eval("Promise.resolve(42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Promise"));
}

// ─── Function Injection ────────────────────────────────────────────────────

#[test]
fn test_register_sync_function() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("add", |a: i32, b: i32| a + b).unwrap();
    let result: i32 = rt.eval_as("add(3, 4)").unwrap();
    assert_eq!(result, 7);
}

#[test]
fn test_register_string_function() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("greet", |name: String| format!("Hello, {}!", name))
        .unwrap();
    let result: String = rt.eval_as("greet('World')").unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_register_multiple_functions() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("add", |a: i32, b: i32| a + b).unwrap();
    rt.register_fn("mul", |a: i32, b: i32| a * b).unwrap();
    let result: i32 = rt.eval_as("mul(add(2, 3), 4)").unwrap();
    assert_eq!(result, 20);
}

#[test]
fn test_register_function_with_no_args() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("get_pi", || std::f64::consts::PI).unwrap();
    let result: f64 = rt.eval_as("get_pi()").unwrap();
    assert!((result - std::f64::consts::PI).abs() < 1e-10);
}

#[test]
fn test_register_function_with_complex_return() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("get_info", || vec![JsValue::String("name".into()), JsValue::Int(42)])
        .unwrap();
    let result: Vec<JsValue> = rt.eval_as("get_info()").unwrap();
    assert_eq!(result.len(), 2);
}

#[test]
fn test_register_function_interop() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("double", |n: i32| n * 2).unwrap();
    // Use injected function inside JS code
    let result: i32 = rt
        .eval_as(
            r#"
            const arr = [1, 2, 3, 4, 5];
            arr.map(x => double(x)).reduce((a, b) => a + b, 0);
        "#,
        )
        .unwrap();
    assert_eq!(result, 30); // 2+4+6+8+10
}

#[test]
fn test_register_mutable_function() {
    use tokimo_package_js_runtime::MutFn;
    let rt = JsRuntime::new().unwrap();
    let mut counter = 0i32;
    rt.register_fn(
        "next",
        MutFn::new(move || {
            counter += 1;
            counter
        }),
    )
    .unwrap();
    let a: i32 = rt.eval_as("next()").unwrap();
    let b: i32 = rt.eval_as("next()").unwrap();
    assert_eq!((a, b), (1, 2));
}

#[test]
fn test_set_global_value() {
    let rt = JsRuntime::new().unwrap();
    rt.set_global("answer", JsValue::Int(42)).unwrap();
    let result: i64 = rt.eval_as("answer + 1").unwrap();
    assert_eq!(result, 43);
}

#[test]
fn test_function_has_name() {
    let rt = JsRuntime::new().unwrap();
    rt.register_fn("myFunc", |x: i32| x).unwrap();
    let name: String = rt.eval_as("myFunc.name").unwrap();
    assert_eq!(name, "myFunc");
}

// ─── Value Export & Parsing ────────────────────────────────────────────────

#[test]
fn test_export_primitive_types() {
    let rt = JsRuntime::new().unwrap();
    let int_val: i64 = rt.eval_as("42").unwrap();
    let float_val: f64 = rt.eval_as("Math.PI").unwrap();
    let str_val: String = rt.eval_as("'hello'").unwrap();
    let bool_val: bool = rt.eval_as("true").unwrap();

    assert_eq!(int_val, 42);
    assert!((float_val - std::f64::consts::PI).abs() < 1e-10);
    assert_eq!(str_val, "hello");
    assert!(bool_val);
}

#[test]
fn test_export_array_to_vec() {
    let rt = JsRuntime::new().unwrap();
    let result: Vec<i64> = rt.eval_as("[10, 20, 30]").unwrap();
    assert_eq!(result, vec![10, 20, 30]);
}

#[test]
fn test_export_array_to_vec_string() {
    let rt = JsRuntime::new().unwrap();
    let result: Vec<String> = rt.eval_as("['a', 'b', 'c']").unwrap();
    assert_eq!(result, vec!["a", "b", "c"]);
}

#[test]
fn test_export_nested_array() {
    let rt = JsRuntime::new().unwrap();
    let result: Vec<Vec<i64>> = rt.eval_as("[[1, 2], [3, 4]]").unwrap();
    assert_eq!(result, vec![vec![1, 2], vec![3, 4]]);
}

#[test]
fn test_export_jsvalue_roundtrip() {
    let rt = JsRuntime::new().unwrap();
    let val = rt.eval("({x: 1, y: 'two', z: [3, 4]})").unwrap();
    // Verify it's an Object
    match &val {
        JsValue::Object(kvs) => {
            assert!(kvs.iter().any(|(k, _)| k == "x"));
            assert!(kvs.iter().any(|(k, _)| k == "y"));
            assert!(kvs.iter().any(|(k, _)| k == "z"));
        }
        _ => panic!("Expected Object variant"),
    }
    // Verify serde roundtrip
    let json = serde_json::to_string(&val).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["x"], 1);
    assert_eq!(parsed["y"], "two");
}

#[test]
fn test_export_struct_via_serde() {
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    let rt = JsRuntime::new().unwrap();
    let point: Point = rt.eval_as("({x: 10, y: 20})").unwrap();
    assert_eq!(point, Point { x: 10, y: 20 });
}

#[test]
fn test_export_struct_with_nested() {
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct Config {
        name: String,
        values: Vec<i32>,
        active: bool,
    }

    let rt = JsRuntime::new().unwrap();
    let config: Config = rt.eval_as("({name: 'test', values: [1, 2, 3], active: true})").unwrap();
    assert_eq!(config.name, "test");
    assert_eq!(config.values, vec![1, 2, 3]);
    assert!(config.active);
}

#[test]
fn test_function_returning_struct() {
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct User {
        id: i32,
        name: String,
    }

    let rt = JsRuntime::new().unwrap();
    rt.register_fn("createUser", |id: i32, name: String| {
        let mut map = BTreeMap::new();
        map.insert("id".to_string(), JsValue::Int(id.into()));
        map.insert("name".to_string(), JsValue::String(name));
        JsValue::Object(map)
    })
    .unwrap();

    let user: User = rt.eval_as("createUser(1, 'Alice')").unwrap();
    assert_eq!(
        user,
        User {
            id: 1,
            name: "Alice".to_string()
        }
    );
}

// ─── Async Execution ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_eval_basic() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: i64 = rt.eval_as("1 + 2").await.unwrap();
    assert_eq!(result, 3);
}

#[tokio::test]
async fn test_async_eval_string() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: String = rt.eval_as("'async' + ' ' + 'works'").await.unwrap();
    assert_eq!(result, "async works");
}

#[tokio::test]
async fn test_async_eval_promise() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: i64 = rt
        .eval_as(
            r#"
            new Promise((resolve) => {
                resolve(42);
            })
        "#,
        )
        .await
        .unwrap();
    assert_eq!(result, 42);
}

#[tokio::test]
async fn test_async_eval_async_await() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: String = rt
        .eval_as(
            r#"
            async function main() {
                return "hello from async";
            }
            main()
        "#,
        )
        .await
        .unwrap();
    assert_eq!(result, "hello from async");
}

#[tokio::test]
async fn test_async_eval_chained_promises() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: i64 = rt
        .eval_as(
            r#"
            Promise.resolve(1)
                .then(x => x + 1)
                .then(x => x * 3)
        "#,
        )
        .await
        .unwrap();
    assert_eq!(result, 6);
}

#[tokio::test]
async fn test_async_eval_error() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result = rt.eval("throw new Error('async error')").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_async_top_level_await() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: i64 = rt
        .eval_as("await Promise.resolve(40) + await Promise.resolve(2)")
        .await
        .unwrap();
    assert_eq!(result, 42);
}

#[tokio::test]
async fn test_async_top_level_await_with_function() {
    let rt = AsyncJsRuntime::new().unwrap();
    let result: String = rt
        .eval_as(
            r#"
            async function fetchData() {
                return "data";
            }
            const value = await fetchData();
            `got: ${value}`
        "#,
        )
        .await
        .unwrap();
    assert_eq!(result, "got: data");
}

#[tokio::test]
async fn test_async_error_message_preserved() {
    let rt = AsyncJsRuntime::new().unwrap();
    let err = rt.eval("throw new Error('boom')").await.unwrap_err();
    assert!(err.to_string().contains("boom"), "got: {err}");
}
