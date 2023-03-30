use salvo::catcher::Catcher;
use salvo::prelude::*;

#[handler]
async fn hello() -> &'static str {
    "Hello World"
}
#[handler]
async fn error500(res: &mut Response) {
    res.set_status_code(StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let acceptor = TcpListener::new("127.0.0.1:5800").bind().await;
    Server::new(acceptor).serve(create_service()).await;
}

fn create_service() -> Service {
    let router = Router::new().get(hello).push(Router::with_path("500").get(error500));
    Service::new(router).with_catcher(handle404).with_catcher(Catcher::new())
}

#[handler]
async fn handle404(&self, _req: &Request, _depot: &Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    if let Some(StatusCode::NOT_FOUND) = res.status_code() {
        res.render("Custom 404 Error Page");
        ctrl.skip_rest();
    }
}
