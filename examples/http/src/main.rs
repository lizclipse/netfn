use std::{collections::HashMap, time::Duration};

use axum::{
    http::StatusCode,
    routing::{any, post},
    Json, Router,
};
use netfn::Service as _;
use netfn_transport_http::HttpTransport;
use serde_json::json;

#[tokio::main]
pub async fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::spawn(serve());
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let transport: HttpTransport = "http://localhost:3210/".try_into().unwrap();
    let client = TestApiClient::new(transport);

    let map = [("hello", "world"), ("bye", "world")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

    println!(">>>> foo");
    println!("{:#?}", client.foo().await);
    println!("<<<<\n");

    println!(">>>> bar");
    println!("{:#?}", client.bar(true).await);
    println!("<<<<\n");

    println!(">>>> baz");
    println!("{:#?}", client.baz().await);
    println!("<<<<\n");

    println!(">>>> qaz");
    println!("{:#?}", client.qaz("hello world".to_owned()).await);
    println!("<<<<\n");

    println!(">>>> qoz");
    println!("{:#?}", client.qoz(map, 9).await);
    println!("<<<<\n");

    println!(">>>> qoz");
    println!("{:#?}", client.qoz(HashMap::default(), 10).await);
    println!("<<<<\n");

    println!(
        "{}",
        serde_json::to_string_pretty(&test_api::TestApiRequest::Foo(test_api::TestApiFooArgs {}))
            .unwrap()
    );
}

#[cfg(not(target_arch = "wasm32"))]
async fn serve() {
    // build our application with a single route
    let app = Router::new()
        .route(
            &format!("/{}", TestService::NAME),
            post(|Json(req): Json<test_api::TestApiRequest>| async {
                let service = TestService;
                println!("{:#?}", req);
                Json(service.dispatch(req).await)
            }),
        )
        .fallback(any(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "message": "Not Found" })),
            )
        }));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3210").await.unwrap();
    println!("beginning listen on 3210");
    axum::serve(listener, app).await.unwrap();
}

#[netfn::service]
trait TestApi {
    /// Foo documentation
    ///
    /// More docs
    async fn foo(&self);

    /// Bar docs
    #[allow(clippy::unused_unit)]
    async fn bar(&self, inp: bool) -> ();

    async fn baz(&self) -> u32;

    async fn qaz(&self, inp: String) -> Vec<String>;

    async fn qoz(&self, inp: HashMap<String, String>, val: i16) -> Result<bool, String>;
}

struct TestService;

impl_service_for_test_api!(TestService, self);
impl TestApi for TestService {
    /// Foo documentation
    ///
    /// More docs
    async fn foo(&self) {
        println!("[foo]");
    }

    /// Bar docs
    #[allow(clippy::unused_unit)]
    async fn bar(&self, inp: bool) -> () {
        println!("[bar] inp: {inp}");
    }

    async fn baz(&self) -> u32 {
        println!("[baz]");
        42
    }

    async fn qaz(&self, inp: String) -> Vec<String> {
        println!("[qaz] inp: {inp}");
        vec!["a".to_owned(), "b".to_owned()]
    }

    async fn qoz(&self, inp: HashMap<String, String>, val: i16) -> Result<bool, String> {
        println!("[qoz] inp: {inp:#?}, val: {val}");
        if val == 10 {
            Err("10 is not allowed".to_owned())
        } else {
            Ok(true)
        }
    }
}
