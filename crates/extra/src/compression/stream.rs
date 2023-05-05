//! Compress the body of a response.
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{self, Error as IoError, ErrorKind, Write};
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use flate2::write::{GzEncoder, ZlibEncoder};
use futures_util::ready;
use futures_util::stream::{BoxStream, Stream};
use hyper::HeaderMap;
use tokio::task::{spawn_blocking, JoinHandle};
use tokio_stream::{self};
use tokio_util::io::{ReaderStream, StreamReader};
use zstd::stream::raw::Operation;
use zstd::stream::write::Encoder as ZstdEncoder;

use salvo_core::http::body::{Body, HyperBody, ResBody};
use salvo_core::http::header::{HeaderValue, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE};
use salvo_core::{async_trait, BoxedError, Depot, FlowCtrl, Handler, Request, Response};

use super::{CompressionAlgo, CompressionLevel, Encoder};

const MAX_CHUNK_SIZE_ENCODE_IN_PLACE: usize = 1024;

pub(super) struct EncodeStream<B> {
    encoder: Option<Encoder>,
    body: B,
    eof: bool,
    encoding: Option<JoinHandle<Result<Encoder, IoError>>>,
}

impl<B> EncodeStream<B> {
    pub(super) fn new(algo: CompressionAlgo, level: CompressionLevel, body: B) -> Self {
        Self {
            encoder: Some(Encoder::new(algo, level)),
            body,
            eof: false,
            encoding: None,
        }
    }
}
impl EncodeStream<BoxStream<'static, Result<Bytes, BoxedError>>> {
    #[inline]
    fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, IoError>>> {
        Stream::poll_next(Pin::new(&mut self.body), cx).map_err(|e| IoError::new(ErrorKind::Other, e))
    }
}
impl EncodeStream<HyperBody> {
    #[inline]
    fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, IoError>>> {
        match ready!(Body::poll_frame(Pin::new(&mut self.body), cx)) {
            Some(Ok(frame)) => Poll::Ready(frame.into_data().map(Ok).ok()),
            Some(Err(e)) => Poll::Ready(Some(Err(IoError::new(ErrorKind::Other, e)))),
            None => Poll::Ready(None),
        }
    }
}
impl EncodeStream<Option<Bytes>> {
    #[inline]
    fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, IoError>>> {
        if let Some(body) = Pin::new(&mut self.body).take() {
            Poll::Ready(Some(Ok(body)))
        } else {
            Poll::Ready(None)
        }
    }
}
impl EncodeStream<VecDeque<Bytes>> {
    #[inline]
    fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, IoError>>> {
        if let Some(body) = Pin::new(&mut self.body).pop_front() {
            Poll::Ready(Some(Ok(body)))
        } else {
            Poll::Ready(None)
        }
    }
}

macro_rules! impl_stream {
    ($name: ty) => {
        impl Stream for EncodeStream<$name> {
            type Item = Result<Bytes, IoError>;
            fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                let this = self.get_mut();
                loop {
                    if this.eof {
                        return Poll::Ready(None);
                    }
                    if let Some(encoding) = &mut this.encoding {
                        let mut encoder = ready!(Pin::new(encoding).poll(cx)).map_err(|e| {
                            IoError::new(
                                io::ErrorKind::Other,
                                format!("blocking task was cancelled unexpectedly: {e}"),
                            )
                        })??;

                        let chunk = encoder.take();
                        this.encoder = Some(encoder);
                        this.encoding.take();

                        if !chunk.is_empty() {
                            return Poll::Ready(Some(Ok(chunk)));
                        }
                    }
                    match this.poll_chunk(cx) {
                        Poll::Ready(Some(Ok(chunk))) => {
                            if let Some(mut encoder) = this.encoder.take() {
                                if chunk.len() < MAX_CHUNK_SIZE_ENCODE_IN_PLACE {
                                    encoder.write(&chunk)?;
                                    let chunk = encoder.take();
                                    this.encoder = Some(encoder);

                                    if !chunk.is_empty() {
                                        return Poll::Ready(Some(Ok(chunk)));
                                    }
                                } else {
                                    this.encoding = Some(spawn_blocking(move || {
                                        encoder.write(&chunk)?;
                                        Ok(encoder)
                                    }));
                                }
                            } else {
                                return Poll::Ready(Some(Ok(chunk)));
                            }
                        }
                        Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                        Poll::Ready(None) => {
                            if let Some(encoder) = this.encoder.take() {
                                let chunk = encoder.finish()?;
                                if chunk.is_empty() {
                                    return Poll::Ready(None);
                                } else {
                                    this.eof = true;
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                            } else {
                                return Poll::Ready(None);
                            }
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    };
}
impl_stream!(BoxStream<'static, Result<Bytes, BoxedError>>);
impl_stream!(HyperBody);
impl_stream!(Option<Bytes>);
impl_stream!(VecDeque<Bytes>);
