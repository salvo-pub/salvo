use std::fmt::Debug;
use std::io::Error as IoError;
use std::str::Utf8Error;

use serde::de::value::Error as DeError;
use thiserror::Error;

use crate::http::{Request, Response, StatusError};
use crate::{async_trait, BoxedError, Depot, Writer};

/// Result type with `ParseError` has it's error type.
pub type ParseResult<T> = Result<T, ParseError>;

/// Errors happened when read data from http request.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ParseError {
    /// The Hyper request did not have a valid Content-Type header.
    #[error("The request did not have a valid Content-Type header.")]
    InvalidContentType,

    /// The Hyper request's body is empty.
    #[error("The request's body is empty.")]
    EmptyBody,

    /// The Hyper request's body is empty.
    #[error("Data is not exist.")]
    NotExist,

    /// Parse error when parse from str.
    #[error("Parse error when parse from str.")]
    ParseFromStr,

    //// A possible error value when converting a `StatusCode` from a `u16` or `&str`
    /// This error indicates that the supplied input was not a valid number, was less
    /// than 100, or was greater than 999.
    #[error("Invalid StatusCode.")]
    InvalidStatusCode(#[from] http::status::InvalidStatusCode),

    /// A possible error value when converting `Method` from bytes.
    #[error("Invalid http method.")]
    InvalidMethod(#[from] http::method::InvalidMethod),
    /// An error resulting from a failed attempt to construct a URI.
    #[error("Invalid uri.")]
    InvalidUri(#[from] http::uri::InvalidUri),
    /// An error resulting from a failed attempt to construct a URI.
    #[error("Invalid uri parts.")]
    InvalidUriParts(#[from] http::uri::InvalidUriParts),
    /// A possible error when converting a `HeaderName` from another type.
    #[error("Invalid header name.")]
    InvalidHeaderName(#[from] http::header::InvalidHeaderName),
    /// A possible error when converting a `HeaderValue` from a string or byte slice.
    #[error("Invalid header value.")]
    InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),

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
    pub fn other(error: impl Into<BoxedError>) -> Self {
        Self::Other(error.into())
    }
}

#[async_trait]
impl Writer for ParseError {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        res.render(
            StatusError::bad_request()
                .brief("parse http data failed.")
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
