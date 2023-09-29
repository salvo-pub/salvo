//! Http body.

use std::boxed::Box;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::pin::Pin;
use std::task::{self, Context, Poll};

use futures_util::stream::{BoxStream, Stream};
use hyper::body::{Body, Frame, Incoming, SizeHint};

use bytes::Bytes;

use crate::error::BoxedError;
use crate::prelude::StatusError;

/// Response body type.
#[allow(clippy::type_complexity)]
#[non_exhaustive]
pub enum ResBody {
    /// None body.
    None,
    /// Once bytes body.
    Once(Bytes),
    /// Chunks body.
    Chunks(VecDeque<Bytes>),
    /// Hyper default body.
    Hyper(Incoming),
    /// Inner body.
    Boxed(Pin<Box<dyn Body<Data = Bytes, Error = BoxedError> + Send + Sync + 'static>>),
    /// Stream body.
    Stream(BoxStream<'static, Result<Bytes, BoxedError>>),
    /// Error body will be process in catcher.
    Error(StatusError),
}
impl ResBody {
    /// Check is that body is not set.
    #[inline]
    pub fn is_none(&self) -> bool {
        matches!(*self, ResBody::None)
    }
    /// Check is that body is once.
    #[inline]
    pub fn is_once(&self) -> bool {
        matches!(*self, ResBody::Once(_))
    }
    /// Check is that body is chunks.
    #[inline]
    pub fn is_chunks(&self) -> bool {
        matches!(*self, ResBody::Chunks(_))
    }
    /// Check is that body is stream.
    #[inline]
    pub fn is_boxed(&self) -> bool {
        matches!(*self, ResBody::Boxed(_))
    }
    /// Check is that body is stream.
    #[inline]
    pub fn is_stream(&self) -> bool {
        matches!(*self, ResBody::Stream(_))
    }
    /// Check is that body is error will be process in catcher.
    pub fn is_error(&self) -> bool {
        matches!(*self, ResBody::Error(_))
    }
    /// Get body's size.
    #[inline]
    pub fn size(&self) -> Option<u64> {
        match self {
            ResBody::None => Some(0),
            ResBody::Once(bytes) => Some(bytes.len() as u64),
            ResBody::Chunks(chunks) => Some(chunks.iter().map(|bytes| bytes.len() as u64).sum()),
            ResBody::Hyper(_) => None,
            ResBody::Boxed(_) => None,
            ResBody::Stream(_) => None,
            ResBody::Error(_) => None,
        }
    }

    /// Set body to none and returns current body.
    #[inline]
    pub fn take(&mut self) -> ResBody {
        std::mem::replace(self, ResBody::None)
    }
}

impl Stream for ResBody {
    type Item = IoResult<Bytes>;

    #[inline]
    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            ResBody::None => Poll::Ready(None),
            ResBody::Once(bytes) => {
                if bytes.is_empty() {
                    Poll::Ready(None)
                } else {
                    let bytes = std::mem::replace(bytes, Bytes::new());
                    Poll::Ready(Some(Ok(bytes)))
                }
            }
            ResBody::Chunks(chunks) => Poll::Ready(chunks.pop_front().map(Ok)),
            ResBody::Hyper(body) => match Body::poll_frame(Pin::new(body), cx) {
                Poll::Ready(Some(Ok(frame))) => Poll::Ready(frame.into_data().map(Ok).ok()),
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(IoError::new(ErrorKind::Other, e)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
            ResBody::Boxed(body) => match Body::poll_frame(Pin::new(body), cx) {
                Poll::Ready(Some(Ok(frame))) => Poll::Ready(frame.into_data().map(Ok).ok()),
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(IoError::new(ErrorKind::Other, e)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
            ResBody::Stream(stream) => stream
                .as_mut()
                .poll_next(cx)
                .map_err(|e| IoError::new(ErrorKind::Other, e)),
            ResBody::Error(_) => Poll::Ready(None),
        }
    }
}

impl Body for ResBody {
    type Data = Bytes;
    type Error = IoError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, <ResBody as Body>::Error>>> {
        match self.poll_next(_cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            ResBody::None => true,
            ResBody::Once(bytes) => bytes.is_empty(),
            ResBody::Chunks(chunks) => chunks.is_empty(),
            ResBody::Hyper(body) => body.is_end_stream(),
            ResBody::Boxed(body) => body.is_end_stream(),
            ResBody::Stream(_) => false,
            ResBody::Error(_) => true,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            ResBody::None => SizeHint::with_exact(0),
            ResBody::Once(bytes) => SizeHint::with_exact(bytes.len() as u64),
            ResBody::Chunks(chunks) => {
                let size = chunks.iter().map(|bytes| bytes.len() as u64).sum();
                SizeHint::with_exact(size)
            }
            ResBody::Hyper(recv) => recv.size_hint(),
            ResBody::Boxed(recv) => recv.size_hint(),
            ResBody::Stream(_) => SizeHint::default(),
            ResBody::Error(_) => SizeHint::with_exact(0),
        }
    }
}

impl From<()> for ResBody {
    fn from(_value: ()) -> ResBody {
        ResBody::None
    }
}
impl From<Bytes> for ResBody {
    fn from(value: Bytes) -> ResBody {
        ResBody::Once(value)
    }
}
impl From<Incoming> for ResBody {
    fn from(value: Incoming) -> ResBody {
        ResBody::Hyper(value)
    }
}
impl From<String> for ResBody {
    #[inline]
    fn from(value: String) -> ResBody {
        ResBody::Once(value.into())
    }
}

impl From<&'static [u8]> for ResBody {
    fn from(value: &'static [u8]) -> ResBody {
        ResBody::Once(value.into())
    }
}

impl From<&'static str> for ResBody {
    fn from(value: &'static str) -> ResBody {
        ResBody::Once(value.into())
    }
}

impl From<Vec<u8>> for ResBody {
    fn from(value: Vec<u8>) -> ResBody {
        ResBody::Once(value.into())
    }
}

impl From<Box<[u8]>> for ResBody {
    fn from(value: Box<[u8]>) -> ResBody {
        ResBody::Once(value.into())
    }
}

impl Debug for ResBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResBody::None => write!(f, "ResBody::None"),
            ResBody::Once(bytes) => write!(f, "ResBody::Once({:?})", bytes),
            ResBody::Chunks(chunks) => write!(f, "ResBody::Chunks({:?})", chunks),
            ResBody::Hyper(_) => write!(f, "ResBody::Hyper(_)"),
            ResBody::Boxed(_) => write!(f, "ResBody::Boxed(_)"),
            ResBody::Stream(_) => write!(f, "ResBody::Stream(_)"),
            ResBody::Error(_) => write!(f, "ResBody::Error(_)"),
        }
    }
}
