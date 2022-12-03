use salvo::macros::Extractible;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

#[handler]
async fn show(req: &mut Request, res: &mut Response) {
    let content = format!(
        r#"<!DOCTYPE html>
    <html>
        <head>
            <title>Parse data</title>
        </head>
        <body>
            <h1>Hello, fill your profile</h1>
            <form action="/{}?username=jobs" method="post">
                <label>First Name:</label><input type="text" name="first_name" />
                <label>Last Name:</label><input type="text" name="last_name" />
                <legend>What is Your Favorite Pet?</legend>      
                <input type="checkbox" name="lovers" value="Cats">Cats<br>      
                <input type="checkbox" name="lovers" value="Dogs">Dogs<br>      
                <input type="checkbox" name="lovers" value="Birds">Birds<br>    
                <input type="submit" value="Submit" />
            </form>
        </body>
    </html>
    "#,
        req.params().get("id").unwrap()
    );
    res.render(Text::Html(content));
}
#[handler]
async fn edit(req: &mut Request) -> String {
    let bad_man: BadMan = req.extract().await.unwrap();
    let bad_man = format!("Bad Man: {:#?}", bad_man);
    let good_man: GoodMan = req.extract().await.unwrap();
    let good_man = format!("Good Man: {:#?}", good_man);
    format!("{}\r\n\r\n\r\n{}", bad_man, good_man)
}

#[derive(Serialize, Deserialize, Extractible, Debug)]
#[extract(
    default_source(from = "query"),
    default_source(from = "param"),
    default_source(from = "body")
)]
struct BadMan<'a> {
    id: i64,
    username: &'a str,
    first_name: String,
    last_name: &'a str,
    lovers: Vec<String>,
}
#[derive(Serialize, Deserialize, Extractible, Debug)]
#[extract(
    default_source(from = "query"),
    default_source(from = "param"),
    default_source(from = "body")
)]
struct GoodMan<'a> {
    id: i64,
    username: &'a str,
    first_name: String,
    last_name: &'a str,
    #[extract(alias = "lovers")]
    lover: &'a str,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let router = Router::with_path("<id>").get(show).post(edit);

    println!("Example url: http://127.0.0.1:7878/95");
    Server::new(TcpListener::bind("127.0.0.1:7878")).serve(router).await;
}
