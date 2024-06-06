//! Provide proxy support for Savlo web framework.
//!
//! # Example
//! 
//! In this example, if the requested URL begins with <http://127.0.0.1:5800/>, the proxy goes to
//! <https://www.rust-lang.org>; if the requested URL begins with <http://localhost:5800/>, the proxy
//! goes to <https://www.rust-lang.org>.
//! 
//! ```no_run
//! use salvo::prelude::*;
//! 
//! #[tokio::main]
//! async fn main() {
//!     tracing_subscriber::fmt().init();
//! 
//!     let router = Router::new()
//!         .push(
//!             Router::new()
//!                 .host("127.0.0.1")
//!                 .path("<**rest>")
//!                 .goal(Proxy::use_hyper_client("https://www.rust-lang.org")),
//!         )
//!         .push(
//!             Router::new()
//!                 .host("localhost")
//!                 .path("<**rest>")
//!                 .goal(Proxy::use_hyper_client("https://crates.io")),
//!         );
//!
//!     let acceptor = TcpListener::new("0.0.0.0:5800").bind().await;
//!     Server::new(acceptor).serve(router).await;
//! }
//! ```
#![doc(html_favicon_url = "https://salvo.rs/favicon-32x32.png")]
#![doc(html_logo_url = "https://salvo.rs/images/logo.svg")]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::convert::Infallible;
use std::error::Error as StdError;
use std::future::Future;

use hyper::upgrade::OnUpgrade;
use percent_encoding::{utf8_percent_encode, CONTROLS};
use salvo_core::http::header::{HeaderMap, HeaderName, HeaderValue, CONNECTION, HOST, UPGRADE};
use salvo_core::http::uri::Uri;
use salvo_core::http::{ReqBody, ResBody, StatusCode};
use salvo_core::{async_trait, BoxedError, Depot, Error, FlowCtrl, Handler, Request, Response};

#[macro_use]
mod cfg;

cfg_feature! {
    #![feature = "hyper-client"]
    mod hyper_client;
    pub use hyper_client::*;
}
cfg_feature! {
    #![feature = "reqwest-client"]
    mod reqwest_client;
    pub use reqwest_client::*;
}

type HyperRequest = hyper::Request<ReqBody>;
type HyperResponse = hyper::Response<ResBody>;

/// Encode url path. This can be used when build your custom url path getter.
#[inline]
pub(crate) fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(|s| utf8_percent_encode(s, CONTROLS).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Client trait.
pub trait Client: Send + Sync + 'static {
    /// Error type.
    type Error: StdError + Send + Sync + 'static;
    /// Elect a upstream to process current request.
    fn execute(
        &self,
        req: HyperRequest,
        upgraded: Option<OnUpgrade>,
    ) -> impl Future<Output = Result<HyperResponse, Self::Error>> + Send;
}

/// Upstreams trait.
pub trait Upstreams: Send + Sync + 'static {
    /// Error type.
    type Error: StdError + Send + Sync + 'static;
    /// Elect a upstream to process current request.
    fn elect(&self) -> impl Future<Output = Result<&str, Self::Error>> + Send;
}
impl Upstreams for &'static str {
    type Error = Infallible;

    async fn elect(&self) -> Result<&str, Self::Error> {
        Ok(*self)
    }
}
impl Upstreams for String {
    type Error = Infallible;
    async fn elect(&self) -> Result<&str, Self::Error> {
        Ok(self.as_str())
    }
}

impl<const N: usize> Upstreams for [&'static str; N] {
    type Error = Error;
    async fn elect(&self) -> Result<&str, Self::Error> {
        if self.is_empty() {
            return Err(Error::other("upstreams is empty"));
        }
        let index = fastrand::usize(..self.len());
        Ok(self[index])
    }
}

impl<T> Upstreams for Vec<T>
where
    T: AsRef<str> + Send + Sync + 'static,
{
    type Error = Error;
    async fn elect(&self) -> Result<&str, Self::Error> {
        if self.is_empty() {
            return Err(Error::other("upstreams is empty"));
        }
        let index = fastrand::usize(..self.len());
        Ok(self[index].as_ref())
    }
}

/// Url part getter. You can use this to get the proxied url path or query.
pub type UrlPartGetter = Box<dyn Fn(&Request, &Depot) -> Option<String> + Send + Sync + 'static>;

/// Default url path getter. This getter will get the url path from request wildcard param, like `<**rest>`, `<*+rest>`.
pub fn default_url_path_getter(req: &Request, _depot: &Depot) -> Option<String> {
    let param = req.params().iter().find(|(key, _)| key.starts_with('*'));
    if let Some((_, rest)) = param {
        Some(encode_url_path(rest))
    } else {
        None
    }
}
/// Default url query getter. This getter just return the query string from request uri.
pub fn default_url_query_getter(req: &Request, _depot: &Depot) -> Option<String> {
    req.uri().query().map(Into::into)
}

/// Handler that can proxy request to other server.
#[non_exhaustive]
pub struct Proxy<U, C>
where
    U: Upstreams,
    C: Client,
{
    /// Upstreams list.
    pub upstreams: U,
    /// [`Client`] for proxy.
    pub client: C,
    /// Url path getter.
    pub url_path_getter: UrlPartGetter,
    /// Url query getter.
    pub url_query_getter: UrlPartGetter,
}

impl<U, C> Proxy<U, C>
where
    U: Upstreams,
    U::Error: Into<BoxedError>,
    C: Client,
{
    /// Create new `Proxy` with upstreams list.
    pub fn new(upstreams: U, client: C) -> Self {
        Proxy {
            upstreams,
            client,
            url_path_getter: Box::new(default_url_path_getter),
            url_query_getter: Box::new(default_url_query_getter),
        }
    }

    /// Set url path getter.
    #[inline]
    pub fn url_path_getter<G>(mut self, url_path_getter: G) -> Self
    where
        G: Fn(&Request, &Depot) -> Option<String> + Send + Sync + 'static,
    {
        self.url_path_getter = Box::new(url_path_getter);
        self
    }

    /// Set url query getter.
    #[inline]
    pub fn url_query_getter<G>(mut self, url_query_getter: G) -> Self
    where
        G: Fn(&Request, &Depot) -> Option<String> + Send + Sync + 'static,
    {
        self.url_query_getter = Box::new(url_query_getter);
        self
    }

    /// Get upstreams list.
    #[inline]
    pub fn upstreams(&self) -> &U {
        &self.upstreams
    }
    /// Get upstreams mutable list.
    #[inline]
    pub fn upstreams_mut(&mut self) -> &mut U {
        &mut self.upstreams
    }

    /// Get client reference.
    #[inline]
    pub fn client(&self) -> &C {
        &self.client
    }
    /// Get client mutable reference.
    #[inline]
    pub fn client_mut(&mut self) -> &mut C {
        &mut self.client
    }

    async fn build_proxied_request(&self, req: &mut Request, depot: &Depot) -> Result<HyperRequest, Error> {
        let upstream = self.upstreams.elect().await.map_err(Error::other)?;
        if upstream.is_empty() {
            tracing::error!("upstreams is empty");
            return Err(Error::other("upstreams is empty"));
        }

        let path = encode_url_path(&(self.url_path_getter)(req, depot).unwrap_or_default());
        let query = (self.url_query_getter)(req, depot);
        let rest = if let Some(query) = query {
            if query.starts_with('?') {
                format!("{}{}", path, query)
            } else {
                format!("{}?{}", path, query)
            }
        } else {
            path
        };
        let forward_url = if upstream.ends_with('/') && rest.starts_with('/') {
            format!("{}{}", upstream.trim_end_matches('/'), rest)
        } else if upstream.ends_with('/') || rest.starts_with('/') {
            format!("{}{}", upstream, rest)
        } else if rest.is_empty() {
            upstream.to_string()
        } else {
            format!("{}/{}", upstream, rest)
        };
        let forward_url: Uri = TryFrom::try_from(forward_url).map_err(Error::other)?;
        let mut build = hyper::Request::builder().method(req.method()).uri(&forward_url);
        for (key, value) in req.headers() {
            if key != HOST {
                build = build.header(key, value);
            }
        }
        if let Some(host) = forward_url.host().and_then(|host| HeaderValue::from_str(host).ok()) {
            build = build.header(HeaderName::from_static("host"), host);
        }
        // let x_forwarded_for_header_name = "x-forwarded-for";
        // // Add forwarding information in the headers
        // match request.headers_mut().entry(x_forwarded_for_header_name) {
        //     Ok(header_entry) => {
        //         match header_entry {
        //             hyper::header::Entry::Vacant(entry) => {
        //                 let addr = format!("{}", client_ip);
        //                 entry.insert(addr.parse().unwrap());
        //             },
        //             hyper::header::Entry::Occupied(mut entry) => {
        //                 let addr = format!("{}, {}", entry.get().to_str().unwrap(), client_ip);
        //                 entry.insert(addr.parse().unwrap());
        //             }
        //         }
        //     }
        //     // shouldn't happen...
        //     Err(_) => panic!("Invalid header name: {}", x_forwarded_for_header_name),
        // }
        build.body(req.take_body()).map_err(Error::other)
    }
}

#[async_trait]
impl<U, C> Handler for Proxy<U, C>
where
    U: Upstreams,
    U::Error: Into<BoxedError>,
    C: Client,
{
    async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, _ctrl: &mut FlowCtrl) {
        match self.build_proxied_request(req, depot).await {
            Ok(proxied_request) => {
                match self
                    .client
                    .execute(proxied_request, req.extensions_mut().remove())
                    .await
                {
                    Ok(response) => {
                        let (
                            salvo_core::http::response::Parts {
                                status,
                                // version,
                                headers,
                                // extensions,
                                ..
                            },
                            body,
                        ) = response.into_parts();
                        res.status_code(status);
                        for (name, value) in headers {
                            if let Some(name) = name {
                                res.headers.insert(name, value);
                            }
                        }
                        res.body(body);
                    }
                    Err(e) => {
                        tracing::error!( error = ?e, uri = ?req.uri(), "get response data failed: {}", e);
                        res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = ?e, "build proxied request failed");
            }
        }
    }
}
#[inline]
#[allow(dead_code)]
fn get_upgrade_type(headers: &HeaderMap) -> Option<&str> {
    if headers
        .get(&CONNECTION)
        .map(|value| {
            value
                .to_str()
                .unwrap_or_default()
                .split(',')
                .any(|e| e.trim() == UPGRADE)
        })
        .unwrap_or(false)
    {
        if let Some(upgrade_value) = headers.get(&UPGRADE) {
            tracing::debug!("Found upgrade header with value: {:?}", upgrade_value.to_str());
            return upgrade_value.to_str().ok();
        }
    }

    None
}

// Unit tests for Proxy
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_url_path() {
        let path = "/test/path";
        let encoded_path = encode_url_path(path);
        assert_eq!(encoded_path, "/test/path");
    }

    #[test]
    fn test_get_upgrade_type() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
        let upgrade_type = get_upgrade_type(&headers);
        assert_eq!(upgrade_type, Some("websocket"));
    }
}
