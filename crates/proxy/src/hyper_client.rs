use hyper::upgrade::OnUpgrade;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::{connect::HttpConnector, Client as HyperUtilClient};
use hyper_util::rt::TokioExecutor;
use salvo_core::http::{ReqBody, ResBody, StatusCode};
use salvo_core::rt::tokio::TokioIo;
use salvo_core::Error;
use tokio::io::copy_bidirectional;

use crate::{Client, Proxy,BoxedError, HyperRequest, HyperResponse, Upstreams};

/// A [`Client`] implementation based on [`hyper_util::client::legacy::Client`].
#[derive(Clone, Debug)]
pub struct HyperClient {
    inner: HyperUtilClient<HttpsConnector<HttpConnector>, ReqBody>,
}

impl<U> Proxy<U, HyperClient>
where
    U: Upstreams,
    U::Error: Into<BoxedError>,
{
    /// Create new `Proxy` which use default hyper util client.
    pub fn default_hyper_client(upstreams: U) -> Self {
        Proxy::new(upstreams, HyperClient::default())
    }
}
impl Default for HyperClient {
    fn default() -> Self {
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("no native root CA certificates found")
            .https_only()
            .enable_http1()
            .build();
        Self {
            inner: HyperUtilClient::builder(TokioExecutor::new()).build(https),
        }
    }
}
impl HyperClient {
    /// Create a new `HyperClient` with the given `HyperClient`.
    pub fn new(inner: HyperUtilClient<HttpsConnector<HttpConnector>, ReqBody>) -> Self {
        Self { inner }
    }
}

impl Client for HyperClient {
    type Error = salvo_core::Error;

    async fn execute(
        &self,
        proxied_request: HyperRequest,
        request_upgraded: Option<OnUpgrade>,
    ) -> Result<HyperResponse, Self::Error> {
        let request_upgrade_type = crate::get_upgrade_type(proxied_request.headers()).map(|s| s.to_owned());

        let mut response = self.inner.request(proxied_request).await.map_err(Error::other)?;

        if response.status() == StatusCode::SWITCHING_PROTOCOLS {
            let response_upgrade_type = crate::get_upgrade_type(response.headers());
            if request_upgrade_type.as_deref() == response_upgrade_type {
                let response_upgraded = hyper::upgrade::on(&mut response).await?;
                if let Some(request_upgraded) = request_upgraded {
                    tokio::spawn(async move {
                        match request_upgraded.await {
                            Ok(request_upgraded) => {
                                let mut request_upgraded = TokioIo::new(request_upgraded);
                                let mut response_upgraded = TokioIo::new(response_upgraded);
                                if let Err(e) = copy_bidirectional(&mut response_upgraded, &mut request_upgraded).await
                                {
                                    tracing::error!(error = ?e, "coping between upgraded connections failed.");
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = ?e, "upgrade request failed.");
                            }
                        }
                    });
                } else {
                    return Err(Error::other("request does not have an upgrade extension."));
                }
            } else {
                return Err(Error::other("upgrade type mismatch"));
            }
        }
        Ok(response.map(ResBody::Hyper))
    }
}