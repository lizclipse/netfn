use std::{collections::HashMap, marker::PhantomData};

use futures::{
    Sink, SinkExt as _, Stream, StreamExt as _,
    channel::{mpsc, oneshot},
    select,
};
use netfn_core::{CallResponseRequest, Transport, TunnelRequest};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

pub enum WebSocketMessage {
    Json(String),
    MessagePack(Vec<u8>),
}

pub trait WebSocketCodec:
    Clone + netfn_core::compat::NetfnSend + netfn_core::compat::NetfnSync
{
    type EncodeError;
    type DecodeError;

    fn encode<T>(&self, value: &T) -> Result<WebSocketMessage, Self::EncodeError>
    where
        T: Serialize;
    fn decode<T>(&self, message: &WebSocketMessage) -> Result<T, Self::DecodeError>
    where
        T: DeserializeOwned;
}

type BusMsg<SinkErr> = (
    u64,
    WebSocketMessage,
    oneshot::Sender<Result<WebSocketMessage, BusError<SinkErr>>>,
);

#[derive(Debug, Clone)]
pub struct WebSocketTransport<Codec, SinkError> {
    codec: Codec,
    ref_sx: mpsc::Sender<oneshot::Sender<u64>>,
    msg_sx: mpsc::Sender<BusMsg<SinkError>>,
    _sink_err: PhantomData<SinkError>,
}

impl<Codec, SinkError> WebSocketTransport<Codec, SinkError> {
    pub fn new(codec: Codec, buffer_size: usize) -> (Self, WebSocketListener<Codec, SinkError>)
    where
        Codec: WebSocketCodec,
    {
        let (ref_sx, ref_rx) = mpsc::channel(buffer_size);
        let (msg_sx, msg_rx) = mpsc::channel(buffer_size);
        let (close_sx, close_rx) = mpsc::channel(1);
        (
            Self {
                codec: codec.clone(),
                ref_sx,
                msg_sx,
                _sink_err: PhantomData::default(),
            },
            WebSocketListener {
                codec,
                ref_rx,
                msg_rx,
                close_sx,
                close_rx,
            },
        )
    }
}

#[derive(Debug)]
pub struct WebSocketListener<Codec, SinkError> {
    codec: Codec,
    ref_rx: mpsc::Receiver<oneshot::Sender<u64>>,
    msg_rx: mpsc::Receiver<BusMsg<SinkError>>,
    close_sx: mpsc::Sender<()>,
    close_rx: mpsc::Receiver<()>,
}

impl<Codec, SinkError> WebSocketListener<Codec, SinkError>
where
    Codec: WebSocketCodec,
{
    pub fn closer(&self) -> WebSocketListenerCloser {
        WebSocketListenerCloser {
            close_sx: self.close_sx.clone(),
        }
    }

    pub async fn listen<Sx, Rx>(&mut self, sink: &mut Sx, stream: &mut Rx)
    where
        Sx: Sink<WebSocketMessage, Error = SinkError> + Unpin,
        Rx: Stream<Item = WebSocketMessage> + Unpin,
    {
        let mut stream = stream.fuse();

        let mut reqs = HashMap::new();
        let mut cref = 0;

        enum Bus<SinkErr> {
            RefRequest(oneshot::Sender<u64>),
            Message(BusMsg<SinkErr>),
            Stream(WebSocketMessage),
        }

        // We loop against all these together to maintain some form of sanity.
        // While this _could_ bottleneck requests, it means that clients cannot get out
        // of sync with each other, and by forcing clients to ask for reference IDs from
        // here it ensures that the counter is always unique for the current tunnel.
        // If the tunnel closes and reopens, in-flight requests get retried and the
        // IDs they need will only be given once the counter has been reset.
        while let Some(s) = select! {
            ref_req = self.ref_rx.next() => ref_req.map(Bus::RefRequest),
            bus = self.msg_rx.next() => bus.map(Bus::Message),
            stream = stream.next() => stream.map(Bus::Stream),
            _ = self.close_rx.next() => None,
        } {
            match s {
                Bus::RefRequest(ref_req_sx) => {
                    let _ = ref_req_sx.send(cref);
                    cref += 1;
                }
                Bus::Message((msg_ref, req, response_sx)) => {
                    match sink.send(req).await {
                        Ok(()) => {
                            reqs.insert(msg_ref, response_sx);
                        }
                        Err(err) => {
                            // If we cant respond, there's nothing we can do but sulk
                            // TODO: add some warn log here
                            let _ = response_sx.send(Err(BusError::Sink(SinkError(err))));
                        }
                    }
                }
                Bus::Stream(res) => {
                    let Ok(PartialRefs { msg_ref, .. }) = self.codec.decode(&res) else {
                        // TODO: log err somewhere
                        continue;
                    };

                    let Some(id) = msg_ref else {
                        // TODO: check handle
                        continue;
                    };

                    let Some(response_sx) = reqs.remove(&id) else {
                        // If the other end has send us something we have no idea about then we cant
                        // do anything
                        // TODO: add some warn log here
                        continue;
                    };
                    // If we cant respond, there's nothing we can do but sulk
                    // TODO: add some warn log here
                    let _ = response_sx.send(Ok(res));
                }
            }
        }

        // We respond to any in-flight requests that the bus has closed to make them
        // try again.
        // Once they do, the first thing they will ask for is new IDs, which will only
        // be given once the bus reopens.
        while let Ok(Some((_, _, response_sx))) = self.msg_rx.try_next() {
            let _ = response_sx.send(Err(BusError::Closed));
        }

        // TODO: we should bubble this back up
        let _ = sink.close().await;
    }
}

#[derive(Clone, Debug)]
pub struct WebSocketListenerCloser {
    close_sx: mpsc::Sender<()>,
}

impl WebSocketListenerCloser {
    pub fn close(&mut self) {
        let _ = self.close_sx.try_send(());
    }
}

impl<Codec, SinkError> Transport for WebSocketTransport<Codec, SinkError>
where
    Codec: WebSocketCodec,
    SinkError: netfn_core::compat::NetfnSend + netfn_core::compat::NetfnSync,
{
    type Error = TransportError<Codec::EncodeError, Codec::DecodeError, SinkError>;

    async fn call<Req, Res>(&self, service: &'static str, request: Req) -> Result<Res, Self::Error>
    where
        Req: Serialize,
        Res: DeserializeOwned,
    {
        let codec = self.codec.clone();
        let mut ref_sx = self.ref_sx.clone();
        let mut msg_sx = self.msg_sx.clone();

        loop {
            // First request an available reference ID
            let (ref_req_sx, ref_req_rx) = oneshot::channel();
            ref_sx.send(ref_req_sx).await?;
            let msg_ref = ref_req_rx.await?;

            // Construct the request and send it
            let (response_sx, response_rx) = oneshot::channel();
            let request = codec
                .encode(&TunnelRequest {
                    msg_ref,
                    payload: CallResponseRequest {
                        service: service.into(),
                        call: &request,
                    },
                })
                .map_err(|e| TransportError::EncodeError(e))?;
            msg_sx.send((msg_ref, request, response_sx)).await?;

            // Wait on the response, looping back if the bus closed mid-request.
            let result = match response_rx.await? {
                Ok(result) => result,
                Err(BusError::Sink(err)) => return Err(err.into()),
                Err(BusError::Closed) => continue,
            };

            let result = codec
                .decode(&result)
                .map_err(|e| TransportError::DecodeError(e))?;
            break Ok(result);
        }
    }
}

#[derive(Error, Debug)]
pub enum TransportError<EncodeError, DecodeError, SinkError> {
    #[error("failed to send request to message bus")]
    SendBus(#[from] mpsc::SendError),
    #[error("failed to receive response")]
    Receive(#[from] oneshot::Canceled),
    #[error("failed to send request to sink")]
    SendSink(#[source] SinkError),
    #[error("failed to encode message")]
    EncodeError(#[source] EncodeError),
    #[error("failed to decode message")]
    DecodeError(#[source] DecodeError),
}

struct SinkError<E>(E);

impl<EncodeError, DecodeError, E> From<SinkError<E>>
    for TransportError<EncodeError, DecodeError, E>
{
    fn from(value: SinkError<E>) -> Self {
        Self::SendSink(value.0)
    }
}

enum BusError<E> {
    Sink(SinkError<E>),
    Closed,
}

#[derive(Deserialize)]
struct PartialRefs {
    #[serde(rename = "ref")]
    msg_ref: Option<u64>,
    // handle: Option<u64>,
}
