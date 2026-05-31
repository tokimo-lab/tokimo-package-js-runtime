use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Context, Ctx, FromJs, Function, IntoJs, Runtime, Value,
    function::IntoJsFunc, qjs,
};
use tokio::sync::{mpsc, oneshot};

use crate::{JsError, JsResult, JsValue};

// ─── Sync Runtime ──────────────────────────────────────────────────────────

/// Synchronous JavaScript runtime wrapping QuickJS.
pub struct JsRuntime {
    rt: Runtime,
    ctx: Context,
}

impl JsRuntime {
    /// Create a new runtime with full intrinsics.
    pub fn new() -> JsResult<Self> {
        let rt = Runtime::new()?;
        rt.set_max_stack_size(1024 * 1024);
        let ctx = Context::full(&rt)?;
        Ok(Self { rt, ctx })
    }

    /// Evaluate JS code and return the result as a `JsValue`.
    pub fn eval(&self, code: &str) -> JsResult<JsValue> {
        self.ctx.with(|ctx| {
            let val: rquickjs::Value = ctx
                .eval(code)
                .catch(&ctx)
                .map_err(|e| JsError::QuickJs(e.to_string()))?;
            if val.is_promise() {
                return Err(JsError::QuickJs(
                    "evaluated to a Promise; use AsyncJsRuntime to await async code".to_string(),
                ));
            }
            JsValue::from_js(&ctx, val).map_err(|e| JsError::TypeConversion(e.to_string()))
        })
    }

    /// Evaluate JS code and deserialize the result into a Rust type.
    pub fn eval_as<T: for<'de> serde::Deserialize<'de>>(&self, code: &str) -> JsResult<T> {
        let js_val = self.eval(code)?;
        js_val.to_rust()
    }

    /// Compile-only syntax check: parses `code` as a global script WITHOUT
    /// executing it. Returns Err(JsError) carrying the SyntaxError message
    /// (include line:col if available) when the script is syntactically invalid.
    pub fn check_syntax(&self, code: &str) -> JsResult<()> {
        let source =
            CString::new(code).map_err(|e| JsError::QuickJs(format!("script contains interior NUL byte: {e}")))?;
        self.ctx.with(|ctx| {
            let raw_ctx = ctx.as_raw().as_ptr();
            let flags = (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32;
            let val = unsafe {
                qjs::JS_Eval(
                    raw_ctx,
                    source.as_ptr(),
                    code.len() as _,
                    c"workflow.js".as_ptr(),
                    flags,
                )
            };
            let is_exception = unsafe { qjs::JS_IsException(val) };
            let error = if is_exception {
                Some(JsError::QuickJs(format_js_exception(&ctx.catch())))
            } else {
                None
            };
            unsafe { qjs::JS_FreeValue(raw_ctx, val) };
            match error {
                Some(err) => Err(err),
                None => Ok(()),
            }
        })
    }

    /// Evaluate JS code, interrupting execution if it runs longer than `timeout`.
    ///
    /// Uses QuickJS's interrupt handler, so even an unyielding infinite loop
    /// (e.g. `while (true) {}`) is aborted once the deadline passes; the call
    /// then returns an error instead of blocking the thread forever.
    pub fn eval_with_timeout(&self, code: &str, timeout: Duration) -> JsResult<JsValue> {
        let deadline = Instant::now() + timeout;
        self.rt
            .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let result = self.eval(code);
        self.rt.set_interrupt_handler(None);
        result
    }

    /// Evaluate JS code with a timeout and deserialize the result.
    ///
    /// See [`eval_with_timeout`](Self::eval_with_timeout).
    pub fn eval_as_with_timeout<T: for<'de> serde::Deserialize<'de>>(
        &self,
        code: &str,
        timeout: Duration,
    ) -> JsResult<T> {
        self.eval_with_timeout(code, timeout)?.to_rust()
    }

    /// Register a Rust function as a JS global, callable from JavaScript.
    ///
    /// Accepts a bare closure directly (this mirrors [`rquickjs::Function::new`]).
    /// For closures that capture mutable state wrap them in [`MutFn`](crate::MutFn);
    /// for `FnOnce` closures use [`OnceFn`](crate::OnceFn).
    ///
    /// # Example
    /// ```
    /// # use tokimo_package_js_runtime::JsRuntime;
    /// let rt = JsRuntime::new().unwrap();
    /// rt.register_fn("add", |a: i32, b: i32| a + b).unwrap();
    /// let sum: i32 = rt.eval_as("add(3, 4)").unwrap();
    /// assert_eq!(sum, 7);
    /// ```
    pub fn register_fn<F, P>(&self, name: &str, func: F) -> JsResult<()>
    where
        F: for<'js> IntoJsFunc<'js, P> + 'static,
    {
        self.ctx.with(|ctx| {
            let js_func = Function::new(ctx.clone(), func)?;
            js_func.set_name(name)?;
            ctx.globals().set(name, js_func)?;
            Ok(())
        })
    }

    /// Set an arbitrary global value.
    ///
    /// Use this for plain values (numbers, strings, objects/arrays via
    /// [`JsValue`]) or for prebuilt function helpers such as
    /// [`Func`](crate::Func)/[`Async`](crate::Async).
    pub fn set_global<V>(&self, name: &str, value: V) -> JsResult<()>
    where
        V: for<'js> IntoJs<'js>,
    {
        self.ctx.with(|ctx| {
            ctx.globals().set(name, value)?;
            Ok(())
        })
    }
}

fn format_js_exception(value: &Value<'_>) -> String {
    let Some(obj) = value.as_object() else {
        return "JavaScript exception".to_string();
    };
    let message = obj
        .get::<_, String>("message")
        .unwrap_or_else(|_| "JavaScript exception".to_string());
    let message = obj
        .get::<_, String>("name")
        .ok()
        .filter(|name| !name.is_empty())
        .map_or(message.clone(), |name| format!("{name}: {message}"));
    let line = obj.get::<_, u32>("lineNumber").ok();
    let column = obj.get::<_, u32>("columnNumber").ok();
    match (line, column) {
        (Some(line), Some(column)) => format!("workflow.js:{line}:{column}: {message}"),
        (Some(line), None) => format!("workflow.js:{line}: {message}"),
        _ => message,
    }
}

// ─── Async Runtime ─────────────────────────────────────────────────────────

/// A boxed setup closure run on the worker thread with access to the JS
/// context. Used to inject globals/functions into the async runtime.
type SetupFn = Box<dyn for<'js> FnOnce(&Ctx<'js>) -> Result<(), String> + Send>;

enum AsyncRequest {
    Eval {
        code: String,
        reply: oneshot::Sender<Result<JsValue, String>>,
    },
    EvalWithTimeout {
        code: String,
        timeout: Duration,
        reply: oneshot::Sender<Result<JsValue, String>>,
    },
    Setup {
        setup: SetupFn,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// Evaluate async JS code on the given context with top-level-await support.
async fn run_eval(async_ctx: &AsyncContext, code: String) -> Result<JsValue, String> {
    async_ctx
        .async_with(async |ctx| {
            // `eval_promise` evaluates with top-level-await support and always
            // yields a Promise that resolves to `{ value: <result> }` (QuickJS
            // async-eval shape).
            let promise = match ctx.eval_promise(code.as_str()).catch(&ctx) {
                Ok(p) => p,
                Err(e) => return Err(e.to_string()),
            };
            let resolved = match promise.into_future::<rquickjs::Value>().await.catch(&ctx) {
                Ok(v) => v,
                Err(e) => return Err(e.to_string()),
            };
            let value = match resolved.as_object() {
                Some(obj) => match obj.get::<_, rquickjs::Value>("value") {
                    Ok(v) => v,
                    Err(e) => return Err(e.to_string()),
                },
                None => resolved,
            };
            // The script's own result may itself be a Promise.
            let value = if value.is_promise() {
                let inner = value.into_promise().unwrap();
                match inner.into_future::<rquickjs::Value>().await.catch(&ctx) {
                    Ok(v) => v,
                    Err(e) => return Err(e.to_string()),
                }
            } else {
                value
            };
            JsValue::from_js(&ctx, value).map_err(|e| e.to_string())
        })
        .await
}

/// Async JavaScript runtime with a dedicated worker thread.
///
/// Supports evaluating async JS code and injecting async Rust functions
/// that can be `await`ed in JavaScript.
pub struct AsyncJsRuntime {
    tx: mpsc::UnboundedSender<AsyncRequest>,
    _handle: Option<thread::JoinHandle<()>>,
}

impl AsyncJsRuntime {
    /// Create a new async runtime.
    pub fn new() -> JsResult<Self> {
        let (tx, rx) = mpsc::unbounded_channel::<AsyncRequest>();

        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            let local = tokio::task::LocalSet::new();

            local.block_on(&rt, async {
                let async_rt = AsyncRuntime::new().expect("Failed to create AsyncRuntime");
                let async_ctx = AsyncContext::full(&async_rt)
                    .await
                    .expect("Failed to create AsyncContext");

                let mut rx = rx;
                while let Some(req) = rx.recv().await {
                    match req {
                        AsyncRequest::Eval { code, reply } => {
                            let _ = reply.send(run_eval(&async_ctx, code).await);
                        }
                        AsyncRequest::EvalWithTimeout { code, timeout, reply } => {
                            let deadline = Instant::now() + timeout;
                            async_rt
                                .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)))
                                .await;
                            let result = run_eval(&async_ctx, code).await;
                            async_rt.set_interrupt_handler(None).await;
                            let _ = reply.send(result);
                        }
                        AsyncRequest::Setup { setup, reply } => {
                            let result: Result<(), String> = async_ctx.async_with(async |ctx| setup(&ctx)).await;
                            let _ = reply.send(result);
                        }
                        AsyncRequest::Shutdown => break,
                    }
                }
            });
        });

        Ok(Self {
            tx,
            _handle: Some(handle),
        })
    }

    /// Evaluate async JS code (may contain `await`).
    pub async fn eval(&self, code: &str) -> JsResult<JsValue> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx.send(AsyncRequest::Eval {
            code: code.to_string(),
            reply: reply_tx,
        })?;
        reply_rx.await.map_err(JsError::from)?.map_err(JsError::QuickJs)
    }

    /// Evaluate async JS code and deserialize the result.
    pub async fn eval_as<T: for<'de> serde::Deserialize<'de>>(&self, code: &str) -> JsResult<T> {
        let js_val = self.eval(code).await?;
        js_val.to_rust()
    }

    /// Evaluate async JS code, interrupting it if it runs longer than `timeout`.
    ///
    /// Uses QuickJS's interrupt handler, so even a synchronous infinite loop on
    /// the worker thread is aborted once the deadline passes and the call
    /// returns an error instead of hanging the worker (and every later request).
    pub async fn eval_with_timeout(&self, code: &str, timeout: Duration) -> JsResult<JsValue> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx.send(AsyncRequest::EvalWithTimeout {
            code: code.to_string(),
            timeout,
            reply: reply_tx,
        })?;
        reply_rx.await.map_err(JsError::from)?.map_err(JsError::QuickJs)
    }

    /// Evaluate async JS code with a timeout and deserialize the result.
    ///
    /// See [`eval_with_timeout`](Self::eval_with_timeout).
    pub async fn eval_as_with_timeout<T: for<'de> serde::Deserialize<'de>>(
        &self,
        code: &str,
        timeout: Duration,
    ) -> JsResult<T> {
        self.eval_with_timeout(code, timeout).await?.to_rust()
    }

    /// Register a Rust function as a JS global on the worker runtime.
    ///
    /// Accepts a bare closure directly. To inject an **async** function that
    /// JavaScript can `await`, wrap a future-returning closure in
    /// [`Async`](crate::Async). For `FnMut` state use [`MutFn`](crate::MutFn).
    ///
    /// The closure must be `Send` because it is moved to the dedicated worker
    /// thread that owns the QuickJS context.
    pub async fn register_fn<F, P>(&self, name: &str, func: F) -> JsResult<()>
    where
        F: for<'js> IntoJsFunc<'js, P> + Send + 'static,
        P: 'static,
    {
        let name = name.to_owned();
        let setup: SetupFn = Box::new(move |ctx: &Ctx| {
            let js_func = Function::new(ctx.clone(), func).map_err(|e| e.to_string())?;
            js_func.set_name(&name).map_err(|e| e.to_string())?;
            ctx.globals().set(name.as_str(), js_func).map_err(|e| e.to_string())?;
            Ok(())
        });
        self.send_setup(setup).await
    }

    /// Set an arbitrary global value on the worker runtime.
    pub async fn set_global<V>(&self, name: &str, value: V) -> JsResult<()>
    where
        V: for<'js> IntoJs<'js> + Send + 'static,
    {
        let name = name.to_owned();
        let setup: SetupFn =
            Box::new(move |ctx: &Ctx| ctx.globals().set(name.as_str(), value).map_err(|e| e.to_string()));
        self.send_setup(setup).await
    }

    async fn send_setup(&self, setup: SetupFn) -> JsResult<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx.send(AsyncRequest::Setup { setup, reply: reply_tx })?;
        reply_rx.await.map_err(JsError::from)?.map_err(JsError::QuickJs)
    }
}

impl Drop for AsyncJsRuntime {
    fn drop(&mut self) {
        let _ = self.tx.send(AsyncRequest::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_syntax_accepts_valid_script() {
        let rt = JsRuntime::new().expect("runtime");
        rt.check_syntax("const x = 1; function ok() { return x; }")
            .expect("valid syntax");
    }

    #[test]
    fn check_syntax_rejects_invalid_script() {
        let rt = JsRuntime::new().expect("runtime");
        let err = rt.check_syntax("const = ;").expect_err("invalid syntax");
        let msg = err.to_string();
        assert!(
            msg.contains("SyntaxError")
                || msg.contains("syntax")
                || msg.contains("unexpected")
                || msg.contains("workflow.js:"),
            "unexpected error message: {msg}"
        );
    }
}
