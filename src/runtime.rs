use std::thread;

use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Context, FromJs, Function, Runtime,
    function::{Func, MutFn},
};
use tokio::sync::{mpsc, oneshot};

use crate::{JsError, JsResult, JsValue};

// ─── Sync Runtime ──────────────────────────────────────────────────────────

/// Synchronous JavaScript runtime wrapping QuickJS.
pub struct JsRuntime {
    #[allow(dead_code)]
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
            JsValue::from_js(&ctx, val).map_err(|e| JsError::TypeConversion(e.to_string()))
        })
    }

    /// Evaluate JS code and deserialize the result into a Rust type.
    pub fn eval_as<T: for<'de> serde::Deserialize<'de>>(&self, code: &str) -> JsResult<T> {
        let js_val = self.eval(code)?;
        js_val.to_rust()
    }

    /// Register a synchronous Rust function in JS global scope.
    ///
    /// # Example
    /// ```ignore
    /// use rquickjs::function::Func;
    /// runtime.register_fn("add", Func::new(|a: i32, b: i32| a + b));
    /// ```
    pub fn register_fn<F>(&self, name: &str, func: F) -> JsResult<()>
    where
        F: for<'js> rquickjs::IntoJs<'js>,
    {
        self.ctx.with(|ctx| {
            let global = ctx.globals();
            global.set(name, func)?;
            Ok(())
        })
    }

    /// Register a mutable synchronous Rust function in JS global scope.
    pub fn register_fn_mut<F>(&self, name: &str, func: F) -> JsResult<()>
    where
        F: FnMut() -> i32 + 'static,
    {
        self.ctx.with(|ctx| {
            let global = ctx.globals();
            global.set(name, Func::from(MutFn::from(func)))?;
            Ok(())
        })
    }

    /// Register a function created from `Function::new` directly.
    pub fn register_function<F, P>(&self, name: &str, func: F) -> JsResult<()>
    where
        F: for<'js> rquickjs::function::IntoJsFunc<'js, P> + 'static,
    {
        self.ctx.with(|ctx| {
            let js_func = Function::new(ctx.clone(), func)?;
            let global = ctx.globals();
            global.set(name, js_func)?;
            Ok(())
        })
    }
}

// ─── Async Runtime ─────────────────────────────────────────────────────────

enum AsyncRequest {
    Eval {
        code: String,
        reply: oneshot::Sender<Result<JsValue, String>>,
    },
    Shutdown,
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
                            let result: Result<JsValue, String> = async_ctx
                                .async_with(async |ctx| {
                                    // `eval_promise` evaluates with top-level-await
                                    // support and always yields a Promise that resolves
                                    // to `{ value: <result> }` (QuickJS async-eval shape).
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
                                .await;
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
}

impl Drop for AsyncJsRuntime {
    fn drop(&mut self) {
        let _ = self.tx.send(AsyncRequest::Shutdown);
    }
}
