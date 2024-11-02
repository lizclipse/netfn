use std::{collections::HashMap, marker::PhantomData};

use futures::{
    channel::{mpsc, oneshot},
    future::Either,
    pin_mut, select, Sink, SinkExt as _, Stream, StreamExt as _,
};
use netfn_core::Transport;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

type BusMsg<Req, Res, SinkErr> = (Req, oneshot::Sender<Result<Res, SinkError<SinkErr>>>);

#[derive(Debug, Clone)]
pub struct StreamTransport<Req, Res, SinkErr> {
    tx: mpsc::Sender<BusMsg<Req, Res, SinkErr>>,
    _sink_err: PhantomData<SinkErr>,
}

impl<Req, Res, SinkErr> StreamTransport<Req, Res, SinkErr> {
    pub fn new<Tx, Rx>(
        sink: Tx,
        stream: Rx,
        buffer_size: usize,
    ) -> (Self, StreamListener<Req, Res, SinkErr, Tx, Rx>)
    where
        Tx: Sink<StreamReq<Req>, Error = SinkErr>,
        Rx: Stream<Item = StreamRes<Res>>,
    {
        let (tx, rx) = mpsc::channel(buffer_size);
        (
            Self {
                tx,
                _sink_err: Default::default(),
            },
            StreamListener {
                msg_rx: rx,
                tx: sink,
                rx: stream,
            },
        )
    }
}

#[derive(Debug)]
pub struct StreamListener<Req, Res, SinkErr, Tx, Rx> {
    msg_rx: mpsc::Receiver<BusMsg<Req, Res, SinkErr>>,
    tx: Tx,
    rx: Rx,
}

impl<Req, Res, SinkErr, Tx, Rx> StreamListener<Req, Res, SinkErr, Tx, Rx>
where
    Tx: Sink<StreamReq<Req>, Error = SinkErr>,
    Rx: Stream<Item = StreamRes<Res>>,
{
    pub async fn listen(mut self) {
        let tx = self.tx;
        pin_mut!(tx);
        let rx = self.rx.fuse();
        pin_mut!(rx);

        let mut cid = 0u64;
        let mut reqs = HashMap::new();
        // TODO: add some cancellation token
        while let Some(s) = select! {
            bus = self.msg_rx.next() => bus.map(Either::Left),
            stream = rx.next() => stream.map(Either::Right),
        } {
            match s {
                Either::Left((req, otx)) => {
                    let id = cid;
                    cid += 1;

                    let sreq = StreamReq { req, id };
                    match tx.send(sreq).await {
                        Ok(()) => {
                            reqs.insert(id, otx);
                        }
                        Err(err) => {
                            // If we cant respond, there's nothing we can do but sulk
                            // TODO: add some warn log here
                            let _ = otx.send(Err(SinkError(err)));
                        }
                    }
                }
                Either::Right(StreamRes { res, id }) => {
                    let Some(otx) = reqs.remove(&id) else {
                        // If the other end has send us something we have no idea about then we cant
                        // do anything
                        // TODO: add some warn log here
                        continue;
                    };
                    // If we cant respond, there's nothing we can do but sulk
                    // TODO: add some warn log here
                    let _ = otx.send(Ok(res));
                }
            }
        }
    }
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

impl<Req, Res, SinkErr> Transport<Req, Res> for StreamTransport<Req, Res, SinkErr>
where
    Req: Send + Serialize + std::fmt::Debug,
    Res: Send + DeserializeOwned,
    SinkErr: Send + Sync,
{
    type Error = TransportError<SinkErr>;

    async fn dispatch(&self, request: Req) -> Result<Res, Self::Error> {
        let mut tx = self.tx.clone();
        let (otx, orx) = oneshot::channel();
        tx.send((request, otx)).await?;
        Ok(orx.await??)
    }
}

#[derive(Error, Debug)]
pub enum TransportError<SinkErr> {
    #[error("failed to send request to message bus")]
    SendBus(#[from] mpsc::SendError),
    #[error("failed to receive response")]
    Receive(#[from] oneshot::Canceled),
    #[error("failed to send request to sink")]
    SendSink(SinkErr),
}

struct SinkError<E>(E);

impl<E> From<SinkError<E>> for TransportError<E> {
    fn from(value: SinkError<E>) -> Self {
        Self::SendSink(value.0)
    }
}
