use thiserror::Error;

#[derive(Error, Debug)]
pub enum JsError {
    #[error("QuickJS error: {0}")]
    QuickJs(String),

    #[error("Type conversion error: {0}")]
    TypeConversion(String),

    #[error("Channel error: {0}")]
    Channel(String),
}

impl From<rquickjs::Error> for JsError {
    fn from(e: rquickjs::Error) -> Self {
        JsError::QuickJs(e.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for JsError {
    fn from(e: tokio::sync::mpsc::error::SendError<T>) -> Self {
        JsError::Channel(e.to_string())
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for JsError {
    fn from(e: tokio::sync::oneshot::error::RecvError) -> Self {
        JsError::Channel(e.to_string())
    }
}
