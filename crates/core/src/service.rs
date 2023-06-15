use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use headers::HeaderValue;
use http::header::{ALT_SVC, CONTENT_TYPE};
use http::uri::Scheme;
use hyper::service::Service as HyperService;
use hyper::{Method, Request as HyperRequest, Response as HyperResponse};

use crate::catcher::{write_error_default, Catcher};
use crate::conn::SocketAddr;
use crate::http::body::{ReqBody, ResBody};
use crate::http::{Mime, Request, Response, StatusCode};
use crate::routing::{FlowCtrl, PathState, Router};
use crate::Depot;

/// Service http request.
#[non_exhaustive]
pub struct Service {
    /// The router of this service.
    pub router: Arc<Router>,
    /// The catcher of this service.
    pub catcher: Option<Arc<Catcher>>,
    /// The allowed media types of this service.
    pub allowed_media_types: Arc<Vec<Mime>>,
}

impl Service {
    /// Create a new Service with a [`Router`].
    #[inline]
    pub fn new<T>(router: T) -> Service
    where
        T: Into<Arc<Router>>,
    {
        Service {
            router: router.into(),
            catcher: None,
            allowed_media_types: Arc::new(vec![]),
        }
    }

    /// Get router in this `Service`.
    #[inline]
    pub fn router(&self) -> Arc<Router> {
        self.router.clone()
    }

    /// When the response code is 400-600 and the body is empty, capture and set the error page content.
    /// If catchers is not set, the default error page will be used.
    ///
    /// # Example
    ///
    /// ```
    /// # use salvo_core::prelude::*;
    /// # use salvo_core::catcher::Catcher;
    ///
    /// #[handler]
    /// async fn handle404(&self, _req: &Request, _depot: &Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    ///     if let Some(StatusCode::NOT_FOUND) = res.status_code {
    ///         res.render("Custom 404 Error Page");
    ///         ctrl.skip_rest();
    ///     }
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     Service::new(Router::new()).catcher(Catcher::default().hoop(handle404));
    /// }
    /// ```
    #[inline]
    pub fn catcher(mut self, catcher: impl Into<Arc<Catcher>>) -> Self {
        self.catcher = Some(catcher.into());
        self
    }

    /// Sets allowed media types list and returns `Self` for write code chained.
    ///
    /// # Example
    ///
    /// ```
    /// # use salvo_core::prelude::*;
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let service = Service::new(Router::new()).allowed_media_types(vec![mime::TEXT_PLAIN]);
    /// # }
    /// ```
    #[inline]
    pub fn allowed_media_types<T>(mut self, allowed_media_types: T) -> Self
    where
        T: Into<Arc<Vec<Mime>>>,
    {
        self.allowed_media_types = allowed_media_types.into();
        self
    }

    #[doc(hidden)]
    #[inline]
    pub fn hyper_handler(
        &self,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        http_scheme: Scheme,
        alt_svc_h3: Option<HeaderValue>,
    ) -> HyperHandler {
        HyperHandler {
            local_addr,
            remote_addr,
            http_scheme,
            router: self.router.clone(),
            catcher: self.catcher.clone(),
            allowed_media_types: self.allowed_media_types.clone(),
            alt_svc_h3,
        }
    }
    /// Handle new request, this function only used for test.
    #[cfg(feature = "test")]
    #[inline]
    pub async fn handle(&self, request: impl Into<Request> + Send) -> Response {
        let request = request.into();
        self.hyper_handler(SocketAddr::Unknown, SocketAddr::Unknown, request.scheme.clone(), None)
            .handle(request)
            .await
    }
}

impl<T> From<T> for Service
where
    T: Into<Arc<Router>>,
{
    #[inline]
    fn from(router: T) -> Self {
        Service::new(router)
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct HyperHandler {
    pub(crate) local_addr: SocketAddr,
    pub(crate) remote_addr: SocketAddr,
    pub(crate) http_scheme: Scheme,
    pub(crate) router: Arc<Router>,
    pub(crate) catcher: Option<Arc<Catcher>>,
    pub(crate) allowed_media_types: Arc<Vec<Mime>>,
    pub(crate) alt_svc_h3: Option<HeaderValue>,
}
impl HyperHandler {
    /// Handle [`Request`] and returns [`Response`].
    #[inline]
    pub fn handle(&self, mut req: Request) -> impl Future<Output = Response> {
        let catcher = self.catcher.clone();
        let allowed_media_types = self.allowed_media_types.clone();
        req.local_addr = self.local_addr.clone();
        req.remote_addr = self.remote_addr.clone();
        #[cfg(not(feature = "cookie"))]
        let mut res = Response::new();
        #[cfg(feature = "cookie")]
        let mut res = Response::with_cookies(req.cookies.clone());
        if let Some(alt_svc_h3) = &self.alt_svc_h3 {
            if !res.headers().contains_key(ALT_SVC) {
                res.headers_mut().insert(ALT_SVC, alt_svc_h3.clone());
            }
        }
        let mut depot = Depot::new();
        let mut path_state = PathState::new(req.uri().path());
        let router = self.router.clone();

        async move {
            if let Some(dm) = router.detect(&mut req, &mut path_state) {
                req.params = path_state.params;
                let mut ctrl = FlowCtrl::new([&dm.hoops[..], &[dm.handler]].concat());
                ctrl.call_next(&mut req, &mut depot, &mut res).await;
                if res.status_code.is_none() {
                    res.status_code = Some(StatusCode::OK);
                }
            } else {
                res.status_code(StatusCode::NOT_FOUND);
            }

            let status = res.status_code.unwrap();
            let has_error = status.is_client_error() || status.is_server_error();
            if let Some(value) = res.headers().get(CONTENT_TYPE) {
                let mut is_allowed = false;
                if let Ok(value) = value.to_str() {
                    if allowed_media_types.is_empty() {
                        is_allowed = true;
                    } else {
                        let ctype: Result<Mime, _> = value.parse();
                        if let Ok(ctype) = ctype {
                            for mime in &*allowed_media_types {
                                if mime.type_() == ctype.type_() && mime.subtype() == ctype.subtype() {
                                    is_allowed = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                if !is_allowed {
                    res.status_code(StatusCode::UNSUPPORTED_MEDIA_TYPE);
                }
            } else if res.body.is_none()
                && !has_error
                && res.status_code != Some(StatusCode::NO_CONTENT)
                && [Method::GET, Method::POST, Method::PATCH, Method::PUT].contains(req.method())
            {
                // check for avoid warning when errors (404 etc.)
                tracing::warn!(
                    uri = ?req.uri(),
                    method = req.method().as_str(),
                    "http response content type header not set"
                );
            }
            if (res.body.is_none() || res.body.is_error()) && has_error {
                if let Some(catcher) = catcher {
                    catcher.catch(&mut req, &mut depot, &mut res).await;
                } else {
                    write_error_default(&req, &mut res, None);
                }
            }
            #[cfg(debug_assertions)]
            if let hyper::Method::HEAD = *req.method() {
                if !res.body.is_none() {
                    tracing::warn!("request with head method should not have body: https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/HEAD");
                }
            }
            #[cfg(feature = "quinn")]
            {
                use bytes::Bytes;
                use parking_lot::Mutex;
                if let Some(session) = req
                    .extensions
                    .remove::<crate::proto::WebTransportSession<h3_quinn::Connection, Bytes>>()
                {
                    res.extensions.insert(session);
                }
                if let Some(conn) = req
                    .extensions
                    .remove::<Mutex<h3::server::Connection<h3_quinn::Connection, Bytes>>>()
                {
                    res.extensions.insert(conn);
                }
                if let Some(stream) = req
                    .extensions
                    .remove::<h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>>()
                {
                    res.extensions.insert(stream);
                }
            }
            res
        }
    }
}

impl<B> HyperService<HyperRequest<B>> for HyperHandler
where
    B: Into<ReqBody>,
{
    type Response = HyperResponse<ResBody>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    #[inline]
    fn call(
        &mut self,
        #[cfg(not(feature = "fix-http1-request-uri"))] req: HyperRequest<B>,
        #[cfg(feature = "fix-http1-request-uri")] mut req: HyperRequest<B>,
    ) -> Self::Future {
        let scheme = req.uri().scheme().cloned().unwrap_or_else(|| self.http_scheme.clone());
        // https://github.com/hyperium/hyper/issues/1310
        #[cfg(feature = "fix-http1-request-uri")]
        if req.uri().scheme().is_none() {
            if let Some(host) = req
                .headers()
                .get(http::header::HOST)
                .and_then(|host| host.to_str().ok())
                .and_then(|host| host.parse::<http::uri::Authority>().ok())
            {
                let mut uri_parts = std::mem::take(req.uri_mut()).into_parts();
                uri_parts.scheme = Some(scheme.clone());
                uri_parts.authority = Some(host);
                if let Ok(uri) = http::uri::Uri::from_parts(uri_parts) {
                    *req.uri_mut() = uri;
                }
            }
        }
        let request = Request::from_hyper(req, scheme);
        let response = self.handle(request);
        Box::pin(async move { Ok(response.await.into_hyper()) })
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::test::{ResponseExt, TestClient};

    #[tokio::test]
    async fn test_service() {
        #[handler]
        async fn before1(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
            res.render(Text::Plain("before1"));
            if req.query::<String>("b").unwrap_or_default() == "1" {
                ctrl.skip_rest();
            } else {
                ctrl.call_next(req, depot, res).await;
            }
        }
        #[handler]
        async fn before2(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
            res.render(Text::Plain("before2"));
            if req.query::<String>("b").unwrap_or_default() == "2" {
                ctrl.skip_rest();
            } else {
                ctrl.call_next(req, depot, res).await;
            }
        }
        #[handler]
        async fn before3(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
            res.render(Text::Plain("before3"));
            if req.query::<String>("b").unwrap_or_default() == "3" {
                ctrl.skip_rest();
            } else {
                ctrl.call_next(req, depot, res).await;
            }
        }
        #[handler]
        async fn hello() -> Result<&'static str, ()> {
            Ok("hello")
        }
        let router = Router::with_path("level1").hoop(before1).push(
            Router::with_hoop(before2)
                .path("level2")
                .push(Router::with_hoop(before3).path("hello").handle(hello)),
        );
        let service = Service::new(router);

        async fn access(service: &Service, b: &str) -> String {
            TestClient::get(format!("http://127.0.0.1:5801/level1/level2/hello?b={}", b))
                .send(service)
                .await
                .take_string()
                .await
                .unwrap()
        }
        let content = access(&service, "").await;
        assert_eq!(content, "before1before2before3hello");
        let content = access(&service, "1").await;
        assert_eq!(content, "before1");
        let content = access(&service, "2").await;
        assert_eq!(content, "before1before2");
        let content = access(&service, "3").await;
        assert_eq!(content, "before1before2before3");
    }
}
