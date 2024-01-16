//! native_tls module
use std::error::Error as StdError;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::marker::PhantomData;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::stream::{BoxStream, Stream, StreamExt};
use futures_util::task::noop_waker_ref;
use http::uri::Scheme;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_native_tls::TlsStream;

use crate::async_trait;
use crate::conn::{Accepted, Acceptor, HandshakeStream, Holding, HttpBuilder, IntoConfigStream, Listener};
use crate::http::{HttpConnection, Version};
use crate::service::HyperHandler;

use super::Identity;

/// NativeTlsListener
pub struct NativeTlsListener<S, C, T, E> {
    config_stream: S,
    inner: T,
    _phantom: PhantomData<(C, E)>,
}
impl<S, C, T, E> NativeTlsListener<S, C, T, E>
where
    S: IntoConfigStream<C> + Send + 'static,
    C: TryInto<Identity, Error = E> + Send + 'static,
    T: Listener + Send,
    E: StdError + Send,
{
    /// Create a new `NativeTlsListener`.
    #[inline]
    pub fn new(config_stream: S, inner: T) -> Self {
        NativeTlsListener {
            config_stream,
            inner,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<S, C, T, E> Listener for NativeTlsListener<S, C, T, E>
where
    S: IntoConfigStream<C> + Send + 'static,
    C: TryInto<Identity, Error = E> + Send + 'static,
    T: Listener + Send,
    T::Acceptor: Send + 'static,
    E: StdError + Send,
{
    type Acceptor = NativeTlsAcceptor<BoxStream<'static, C>, C, T::Acceptor, E>;

    async fn try_bind(self) -> crate::Result<Self::Acceptor> {
        Ok(NativeTlsAcceptor::new(
            self.config_stream.into_stream().boxed(),
            self.inner.try_bind().await?,
        ))
    }
}

impl<S> HttpConnection for TlsStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    async fn serve(
        self,
        handler: HyperHandler,
        builder: Arc<HttpBuilder>,
        idle_timeout: Option<Duration>,
    ) -> IoResult<()> {
        builder
            .serve_connection(self, handler, idle_timeout)
            .await
            .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))
    }
}

/// NativeTlsAcceptor
pub struct NativeTlsAcceptor<S, C, T, E> {
    config_stream: S,
    inner: T,
    holdings: Vec<Holding>,
    tls_acceptor: Option<tokio_native_tls::TlsAcceptor>,
    _phantom: PhantomData<(C, E)>,
}
impl<S, C, T, E> NativeTlsAcceptor<S, C, T, E>
where
    T: Acceptor,
    E: StdError + Send,
{
    /// Create a new `NativeTlsAcceptor`.
    pub fn new(config_stream: S, inner: T) -> NativeTlsAcceptor<S, C, T, E> {
        let holdings = inner
            .holdings()
            .iter()
            .map(|h| {
                let mut versions = h.http_versions.clone();
                #[cfg(feature = "http1")]
                if !versions.contains(&Version::HTTP_11) {
                    versions.push(Version::HTTP_11);
                }
                #[cfg(feature = "http2")]
                if !versions.contains(&Version::HTTP_2) {
                    versions.push(Version::HTTP_2);
                }
                Holding {
                    local_addr: h.local_addr.clone(),
                    http_versions: versions,
                    http_scheme: Scheme::HTTPS,
                }
            })
            .collect();
        NativeTlsAcceptor {
            config_stream,
            inner,
            holdings,
            tls_acceptor: None,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<S, C, T, E> Acceptor for NativeTlsAcceptor<S, C, T, E>
where
    S: Stream<Item = C> + Send + Unpin + 'static,
    C: TryInto<Identity, Error = E> + Send + 'static,
    T: Acceptor + Send + 'static,
    <T as Acceptor>::Conn: AsyncRead + AsyncWrite + Unpin + Send,
    E: StdError + Send,
{
    type Conn = HandshakeStream<TlsStream<T::Conn>>;

    #[inline]
    fn holdings(&self) -> &[Holding] {
        &self.holdings
    }

    #[inline]
    async fn accept(&mut self) -> IoResult<Accepted<Self::Conn>> {
        let config = {
            let mut config = None;
            while let Poll::Ready(Some(item)) = self
                .config_stream
                .poll_next_unpin(&mut Context::from_waker(noop_waker_ref()))
            {
                config = Some(item);
            }
            config
        };
        if let Some(config) = config {
            let identity = config
                .try_into()
                .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;
            let tls_acceptor = tokio_native_tls::native_tls::TlsAcceptor::new(identity);
            match tls_acceptor {
                Ok(tls_acceptor) => {
                    if self.tls_acceptor.is_some() {
                        tracing::info!("tls config changed.");
                    } else {
                        tracing::info!("tls config loaded.");
                    }
                    self.tls_acceptor = Some(tokio_native_tls::TlsAcceptor::from(tls_acceptor));
                }
                Err(e) => tracing::error!(error = ?e, "native_tls: invalid tls config"),
            }
        }

        let tls_acceptor = match &self.tls_acceptor {
            Some(tls_acceptor) => tls_acceptor.clone(),
            None => return Err(IoError::new(ErrorKind::Other, "native_tls: invalid tls config")),
        };
        let Accepted {
            conn,
            local_addr,
            remote_addr,
            http_version,
            http_scheme,
        } = self.inner.accept().await?;
        let conn = async move {
            tls_acceptor
                .accept(conn)
                .await
                .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))
        };
        Ok(Accepted {
            conn: HandshakeStream::new(conn),
            local_addr,
            remote_addr,
            http_version,
            http_scheme,
        })
    }
}
