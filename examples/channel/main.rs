#![warn(clippy::pedantic)]

use std::collections::HashMap;

use netfn_transport_channel::ChannelTransport;

#[tokio::main]
pub async fn main() {
    let service = TestService;

    let (transport, listener) = ChannelTransport::new(service, 128);

    tokio::spawn(listener.listen());

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
    async fn foo(&self) {
        println!("[foo]");
    }

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
