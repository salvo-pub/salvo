use salvo::basic_auth::{BasicAuth, BasicAuthValidator};
use salvo::prelude::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();
    Server::new(TcpListener::bind("127.0.0.1:7878")).serve(route()).await;
}
fn route() -> Router {
    let auth_handler = BasicAuth::new(Validator);
    Router::with_hoop(auth_handler).handle(hello)
}
#[handler]
async fn hello() -> &'static str {
    "Hello"
}

struct Validator;
#[async_trait]
impl BasicAuthValidator for Validator {
    async fn validate(&self, username: &str, password: &str) -> bool {
        username == "root" && password == "pwd"
    }
}

#[cfg(test)]
mod tests {
    use salvo::prelude::*;
    use salvo::test::{ResponseExt, TestClient};

    #[tokio::test]
    async fn test_basic_auth() {
        let service = Service::new(super::route());

        let content = TestClient::get("http://127.0.0.1:7878/")
            .basic_auth("root", Some("pwd"))
            .send(&service)
            .await
            .take_string()
            .await
            .unwrap();
        assert!(content.contains("Hello"));

        let content = TestClient::get("http://127.0.0.1:7878/")
            .basic_auth("root", Some("pwd2"))
            .send(&service)
            .await
            .take_string()
            .await
            .unwrap();
        assert!(content.contains("Unauthorized"));
    }
}
