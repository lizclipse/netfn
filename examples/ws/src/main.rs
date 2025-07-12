use std::collections::HashMap;

use futures::{SinkExt as _, StreamExt as _, pin_mut};
use netfn_transport_stream::StreamTransport;
use netfn_transport_ws::{WebSocketCodec, WebSocketMessage};
use tokio_tungstenite::connect_async;

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(example());
}

pub async fn example() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::spawn(serve());
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // let transport: HttpTransport = "http://localhost:3210/".try_into().unwrap();
    let codec = WebSocketCodec::new();
    codec.set_use_json(true);
    let (transport, mut listener) = StreamTransport::new(codec, 16);
    let mut closer = listener.closer();

    let (ws, _) = connect_async("ws://localhost:3210").await.unwrap();
    tokio::spawn(async move {
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        let (sink, stream) = ws.split();

        let sink = sink.with(|msg: WebSocketMessage| async {
            let msg = match msg {
                WebSocketMessage::Json(msg) => WsMessage::Text(msg.into()),
                WebSocketMessage::MessagePack(msg) => WsMessage::Binary(msg.into()),
            };

            Ok::<WsMessage, tokio_tungstenite::tungstenite::Error>(msg)
        });
        pin_mut!(sink);

        let stream = stream.map(|msg| {
            msg.map(|msg| match msg {
                WsMessage::Text(msg) => WebSocketMessage::Json(msg.as_str().into()),
                WsMessage::Binary(msg) => WebSocketMessage::MessagePack(msg.into()),
                _ => todo!(),
            })
            .unwrap()
        });
        pin_mut!(stream);

        listener.listen(&mut sink, &mut stream).await;
    });

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

    closer.close();
}

#[cfg(not(target_arch = "wasm32"))]
async fn serve() {
    use axum::{Router, extract::WebSocketUpgrade, routing::any};

    let app = Router::new().route(
        "/",
        any(async |ws: WebSocketUpgrade| ws.on_upgrade(handle_socket)),
    );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3210").await.unwrap();
    println!("beginning listen on 3210");
    axum::serve(listener, app).await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_socket(mut socket: axum::extract::ws::WebSocket) {
    use axum::extract::ws::Message;
    use netfn::Service as _;

    let service = TestService.into_service();

    while let Some(Ok(Message::Text(msg))) = socket.recv().await {
        let msg: netfn::TunnelRequest<'static, test_api::TestApiRequest> =
            serde_json::from_str(msg.as_str()).unwrap();

        let res = service.call(msg.payload.call).await;
        let res = socket
            .send(Message::Text(
                serde_json::to_string(&netfn::TunnelResponse {
                    msg_ref: msg.msg_ref,
                    data: res,
                })
                .unwrap()
                .into(),
            ))
            .await;

        if let Err(res) = res {
            eprintln!("failed to send down ws: {res:#?}");
        }

        // let _ = socket.send(Message::Close(None)).await;
    }
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
