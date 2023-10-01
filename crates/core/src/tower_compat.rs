//! Tower service compat.
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::io::{Error as IoError, ErrorKind};
use std::task::{Context, Poll};

use futures_util::future::{BoxFuture, FutureExt};
use http_body_util::BodyExt;
use hyper::body::{Body, Bytes, Frame};
use tower::buffer::Buffer;
use tower::{Layer, Service, ServiceExt};

use crate::http::{ReqBody, ResBody, StatusError};
use crate::{async_trait, Depot, FlowCtrl, Handler, Request, Response};

/// Trait for tower service compat.
pub trait TowerServiceCompat<B, E, Fut> {
    /// Converts a tower service to a salvo handler.
    fn compat(self) -> TowerServiceHandler<Self>
    where
        Self: Sized,
    {
        TowerServiceHandler(self)
    }
}

impl<T, B, E, Fut> TowerServiceCompat<B, E, Fut> for T
where
    B: Body + Send + Sync + 'static,
    B::Data: Into<Bytes> + Send + fmt::Debug + 'static,
    B::Error: StdError + Send + Sync + 'static,
    E: StdError + Send + Sync + 'static,
    T: Service<hyper::Request<ReqBody>, Response = hyper::Response<B>, Future = Fut> + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<hyper::Response<B>, E>> + Send + 'static,
{
}

/// Tower service compat handler.
pub struct TowerServiceHandler<Svc>(Svc);

#[async_trait]
impl<Svc, B, E, Fut> Handler for TowerServiceHandler<Svc>
where
    B: Body + Send + Sync + 'static,
    B::Data: Into<Bytes> + Send + fmt::Debug + 'static,
    B::Error: StdError + Send + Sync + 'static,
    E: StdError + Send + Sync + 'static,
    Svc: Service<hyper::Request<ReqBody>, Response = hyper::Response<B>, Future = Fut> + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<hyper::Response<B>, E>> + Send + 'static,
{
    async fn handle(&self, req: &mut Request, _depot: &mut Depot, res: &mut Response, _ctrl: &mut FlowCtrl) {
        let mut svc = self.0.clone();
        if let Err(_) = svc.ready().await {
            tracing::error!("tower service not ready.");
            res.render(StatusError::internal_server_error().cause("tower service not ready."));
            return;
        }
        let hyper_req = match req.strip_to_hyper() {
            Ok(hyper_req) => hyper_req,
            Err(_) => {
                tracing::error!("strip request to hyper failed.");
                res.render(StatusError::internal_server_error().cause("strip request to hyper failed."));
                return;
            }
        };

        let hyper_res = match svc.call(hyper_req).await {
            Ok(hyper_res) => hyper_res,
            Err(_) => {
                tracing::error!("call tower service failed.");
                res.render(StatusError::internal_server_error().cause("call tower service failed."));
                return;
            }
        }
        .map(|res| {
            ResBody::Boxed(Box::pin(
                res.map_frame(|f| match f.into_data() {
                    //TODO: should use Frame::map_data after new version of hyper is released.
                    Ok(data) => Frame::data(data.into()),
                    Err(frame) => Frame::trailers(frame.into_trailers().expect("frame must be trailers")),
                })
                .map_err(|e| e.into()),
            ))
        });

        res.merge_hyper(hyper_res);
    }
}

struct FlowCtrlInContext {
    ctrl: FlowCtrl,
    request: Request,
    depot: Depot,
    response: Response,
}
impl FlowCtrlInContext {
    fn new(ctrl: FlowCtrl, request: Request, depot: Depot, response: Response) -> Self {
        Self {
            ctrl,
            request,
            depot,
            response,
        }
    }
}
struct FlowCtrlOutContext {
    ctrl: FlowCtrl,
    request: Request,
    depot: Depot,
}
impl FlowCtrlOutContext {
    fn new(ctrl: FlowCtrl, request: Request, depot: Depot) -> Self {
        Self { ctrl, request, depot }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct FlowCtrlService;
impl Service<hyper::Request<ReqBody>> for FlowCtrlService {
    type Response = hyper::Response<ResBody>;
    type Error = IoError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut hyper_req: hyper::Request<ReqBody>) -> Self::Future {
        let Some(FlowCtrlInContext {
            mut ctrl,
            mut request,
            mut depot,
            mut response,
        }) = hyper_req.extensions_mut().remove::<FlowCtrlInContext>()
        else {
            return futures_util::future::ready(Err(IoError::new(
                ErrorKind::Other,
                "`FlowCtrlInContext` should exists in request extension.".to_owned(),
            )))
            .boxed();
        };
        request.merge_hyper(hyper_req);
        Box::pin(async move {
            ctrl.call_next(&mut request, &mut depot, &mut response).await;
            response
                .extensions
                .insert(FlowCtrlOutContext::new(ctrl, request, depot));
            Ok(response.strip_to_hyper())
        })
    }
}

/// Trait for tower layer compat.
pub trait TowerLayerCompat {
    /// Converts a tower layer to a salvo handler.
    fn compat(self) -> TowerLayerHandler<Self::Service>
    where
        Self: Layer<FlowCtrlService> + Sized,
        Self::Service: tower::Service<hyper::Request<ReqBody>> + Sync + Send + 'static,
        <Self::Service as Service<hyper::Request<ReqBody>>>::Future: Send,
        <Self::Service as Service<hyper::Request<ReqBody>>>::Error: StdError + Send + Sync,
    {
        TowerLayerHandler(Buffer::new(self.layer(FlowCtrlService), 32))
    }
}

impl<T> TowerLayerCompat for T where T: Layer<FlowCtrlService> + Send + Sync + Sized + 'static {}

/// Tower service compat handler.
pub struct TowerLayerHandler<Svc: Service<hyper::Request<ReqBody>>>(Buffer<Svc, hyper::Request<ReqBody>>);

#[async_trait]
impl<Svc, B, E> Handler for TowerLayerHandler<Svc>
where
    B: Body + Send + Sync + 'static,
    B::Data: Into<Bytes> + Send + fmt::Debug + 'static,
    B::Error: StdError + Send + Sync + 'static,
    E: StdError + Send + Sync + 'static,
    Svc: Service<hyper::Request<ReqBody>, Response = hyper::Response<B>> + Send + 'static,
    Svc::Future: Future<Output = Result<hyper::Response<B>, E>> + Send + 'static,
    Svc::Error: StdError + Send + Sync,
{
    async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
        let mut svc = self.0.clone();
        if let Err(_) = svc.ready().await {
            tracing::error!("tower service not ready.");
            res.render(StatusError::internal_server_error().cause("tower service not ready."));
            return;
        }

        let mut hyper_req = match req.strip_to_hyper() {
            Ok(hyper_req) => hyper_req,
            Err(_) => {
                tracing::error!("strip request to hyper failed.");
                res.render(StatusError::internal_server_error().cause("strip request to hyper failed."));
                return;
            }
        };
        let ctx = FlowCtrlInContext::new(
            std::mem::take(ctrl),
            std::mem::take(req),
            std::mem::take(depot),
            std::mem::take(res),
        );
        hyper_req.extensions_mut().insert(ctx);

        let mut hyper_res = match svc.call(hyper_req).await {
            Ok(hyper_res) => hyper_res,
            Err(_) => {
                tracing::error!("call tower service failed.");
                res.render(StatusError::internal_server_error().cause("call tower service failed."));
                return;
            }
        }
        .map(|res| {
            ResBody::Boxed(Box::pin(
                res.map_frame(|f| match f.into_data() {
                    //TODO: should use Frame::map_data after new version of hyper is released.
                    Ok(data) => Frame::data(data.into()),
                    Err(frame) => Frame::trailers(frame.into_trailers().expect("frame must be trailers")),
                })
                .map_err(|e| e.into()),
            ))
        });
        let origin_depot = depot;
        let origin_ctrl = ctrl;
        if let Some(FlowCtrlOutContext { ctrl, request, depot }) =
            hyper_res.extensions_mut().remove::<FlowCtrlOutContext>()
        {
            *origin_depot = depot;
            *origin_ctrl = ctrl;
            *req = request;
        } else {
            tracing::error!("`FlowCtrlOutContext` should exists in response extensions.");
        }

        res.merge_hyper(hyper_res);
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::test::{ResponseExt, TestClient};
    use crate::{handler, Router};

    #[tokio::test]
    async fn test_tower_layer() {
        struct TestService<S> {
            inner: S,
        }

        impl<S, Req> tower::Service<Req> for TestService<S>
        where
            S: Service<Req>,
        {
            type Response = S::Response;
            type Error = S::Error;
            type Future = S::Future;

            fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                self.inner.poll_ready(cx)
            }

            fn call(&mut self, req: Req) -> Self::Future {
                self.inner.call(req)
            }
        }

        struct MyServiceLayer;

        impl<S> Layer<S> for MyServiceLayer {
            type Service = TestService<S>;

            fn layer(&self, inner: S) -> Self::Service {
                TestService { inner }
            }
        }

        #[handler]
        async fn hello() -> &'static str {
            "Hello World"
        }
        let router = Router::new().hoop(MyServiceLayer.compat()).get(hello);
        assert_eq!(
            TestClient::get("http://127.0.0.1:5800")
                .send(router)
                .await
                .take_string()
                .await
                .unwrap(),
            "Hello World"
        );
    }
}
