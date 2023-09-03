use std::io::Error as IoError;
use std::str::Utf8Error;

use serde::de::value::Error as DeError;
use thiserror::Error;

use crate::http::StatusError;
use crate::{BoxedError, Scribe, Response};

/// Result type with `ParseError` has it's error type.
pub type ParseResult<T> = Result<T, ParseError>;

/// ParseError, errors happened when read data from http request.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ParseError {
    /// The Hyper request did not have a valid Content-Type header.
    #[error("The Hyper request did not have a valid Content-Type header.")]
    InvalidContentType,

    /// The Hyper request's body is empty.
    #[error("The Hyper request's body is empty.")]
    EmptyBody,

    /// Parse error when parse from str.
    #[error("Parse error when parse from str.")]
    ParseFromStr,

    /// Parse error when parse from str.
    #[error("Parse error when decode url.")]
    UrlDecode,

    /// Deserialize error when parse from request.
    #[error("Deserialize error.")]
    Deserialize(#[from] DeError),

    /// DuplicateKey.
    #[error("DuplicateKey.")]
    DuplicateKey,

    /// The Hyper request Content-Type top-level Mime was not `Multipart`.
    #[error("The Hyper request Content-Type top-level Mime was not `Multipart`.")]
    NotMultipart,

    /// The Hyper request Content-Type sub-level Mime was not `FormData`.
    #[error("The Hyper request Content-Type sub-level Mime was not `FormData`.")]
    NotFormData,

    /// InvalidRange.
    #[error("InvalidRange")]
    InvalidRange,

    /// An multer error.
    #[error("Multer error: {0}")]
    Multer(#[from] multer::Error),

    /// An I/O error.
    #[error("I/O error: {}", _0)]
    Io(#[from] IoError),

    /// An error was returned from hyper.
    #[error("Hyper error: {0}")]
    Hyper(#[from] hyper::Error),

    /// An error occurred during UTF-8 processing.
    #[error("UTF-8 processing error: {0}")]
    Utf8(#[from] Utf8Error),

    /// Serde json error.
    #[error("Serde json error: {0}")]
    SerdeJson(#[from] serde_json::error::Error),

    /// Custom error that does not fall under any other error kind.
    #[error("Other error: {0}")]
    Other(BoxedError),
}
impl ParseError {
    /// Create a custom error.
    #[inline]
    pub fn other(error: impl Into<BoxedError>) -> Self {
        Self::Other(error.into())
    }
}

impl Scribe for ParseError {
    #[inline]
    fn render(self, res: &mut Response) {
        res.render(
            StatusError::internal_server_error()
                .brief("http read error happened")
                .cause(self),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[tokio::test]
    async fn test_write_error() {
        let mut res = Response::default();
        let mut req = Request::default();
        let mut depot = Depot::new();
        let err = ParseError::EmptyBody;
        err.write(&mut req, &mut depot, &mut res).await;
    }
}
