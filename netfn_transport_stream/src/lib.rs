use futures_channel::{mpsc, oneshot};
use futures_core::Stream;
use futures_sink::Sink;
use futures_util::SinkExt as _;
use netfn_core::Transport;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug)]
pub struct StreamTransport<Req, Res> {
    tx: mpsc::Sender<(Req, oneshot::Sender<Res>)>,
}

impl<Req, Res> StreamTransport<Req, Res> {
    pub fn new<Tx, Rx>(
        sink: Tx,
        stream: Rx,
        buffer_size: usize,
    ) -> (Self, StreamListener<Req, Res, Tx, Rx>)
    where
        Tx: Sink<Req>,
        Rx: Stream<Item = Res>,
    {
        let (tx, rx) = mpsc::channel(buffer_size);
        (
            Self { tx },
            StreamListener {
                msg_rx: rx,
                tx: sink,
                rx: stream,
            },
        )
    }

    pub async fn listen(&self) {}
}

#[derive(Debug)]
pub struct StreamListener<Req, Res, Tx, Rx> {
    msg_rx: mpsc::Receiver<(Req, oneshot::Sender<Res>)>,
    tx: Tx,
    rx: Rx,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StreamReq<Req> {
    req: Req,
    id: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StreamRes<Res> {
    res: Res,
    id: u64,
}

impl<Req, Res> Transport<Req, Res> for StreamTransport<Req, Res>
where
    Req: Send + Serialize + std::fmt::Debug,
    Res: Send + DeserializeOwned,
{
    type Error = TransportError;

    async fn dispatch(&self, request: Req) -> Result<Res, Self::Error> {
        let mut tx = self.tx.clone();
        let (otx, orx) = oneshot::channel();
        tx.send((request, otx)).await?;
        Ok(orx.await?)
    }
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("failed to send request")]
    Send(#[from] mpsc::SendError),
    #[error("failed to receive response")]
    Receive(#[from] oneshot::Canceled),
}
