use salvo::conn::native_tls::NativeTlsConfig;
use salvo::prelude::*;

#[handler]
async fn hello(res: &mut Response) {
    res.render(Text::Plain("Hello World"));
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let router = Router::new().get(hello);
    let config = NativeTlsConfig::new()
        .with_pkcs12(include_bytes!("../certs/identity.p12").to_vec())
        .with_password("mypass");
    Server::new(TcpListener::bind("127.0.0.1:7878")).serve(router).await;
}
