use std::{collections::HashMap, marker::PhantomData};

use futures::{
    Sink, SinkExt as _, Stream, StreamExt as _,
    channel::{mpsc, oneshot},
    select,
    task::{LocalSpawn, LocalSpawnExt as _, Spawn, SpawnError, SpawnExt as _},
};
use netfn_core::{CallResponseRequest, Transport, TunnelRequest, TunnelResponse};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

pub trait StreamCodec:
    Clone + netfn_core::compat::NetfnSend + netfn_core::compat::NetfnSync
{
    type Format;
    type EncodeError;
    type DecodeError;

    fn encode<T>(&self, value: &T) -> Result<Self::Format, Self::EncodeError>
    where
        T: Serialize;

    fn decode<T>(&self, message: &Self::Format) -> Result<T, Self::DecodeError>
    where
        T: DeserializeOwned;

    fn get_ref(&self, message: &Self::Format) -> Result<Option<u64>, Self::DecodeError>;
}

type BusMsg<Format, SinkErr> = (
    u64,
    Format,
    oneshot::Sender<Result<Format, BusError<SinkErr>>>,
);

#[derive(Debug, Clone)]
pub struct StreamTransport<Codec, Format, SinkError> {
    codec: Codec,
    ref_sx: mpsc::Sender<oneshot::Sender<u64>>,
    msg_sx: mpsc::Sender<BusMsg<Format, SinkError>>,
    _sink_err: PhantomData<SinkError>,
}

impl<Codec, SinkError> StreamTransport<Codec, Codec::Format, SinkError>
where
    Codec: StreamCodec,
{
    pub fn new(
        codec: Codec,
        buffer_size: usize,
    ) -> (Self, StreamListener<Codec, Codec::Format, SinkError>) {
        let (ref_sx, ref_rx) = mpsc::channel(buffer_size);
        let (msg_sx, msg_rx) = mpsc::channel(buffer_size);
        (
            Self {
                codec: codec.clone(),
                ref_sx,
                msg_sx,
                _sink_err: PhantomData::default(),
            },
            StreamListener {
                codec,
                ref_rx,
                msg_rx,
            },
        )
    }
}

#[derive(Debug)]
pub struct StreamListener<Codec, Format, SinkError> {
    codec: Codec,
    ref_rx: mpsc::Receiver<oneshot::Sender<u64>>,
    msg_rx: mpsc::Receiver<BusMsg<Format, SinkError>>,
}

impl<Codec, SinkError> StreamListener<Codec, Codec::Format, SinkError>
where
    Codec: StreamCodec,
{
    /// Sets up the main loop that sends and receives messages on the givin sink and stream.
    ///
    /// All client APIs that use the transport that this came from will communicate with this,
    /// meaning that nothing will happen until the loop has been started.
    pub fn start<Sx, Rx, StreamError, Executor>(
        mut self,
        sink: Sx,
        stream: Rx,
        executor: Executor,
    ) -> Result<StreamListenerHandle<Codec, Codec::Format, SinkError>, SpawnError>
    where
        Sx: Sink<Codec::Format, Error = SinkError> + Unpin + Send + 'static,
        Rx: Stream<Item = Result<Codec::Format, StreamError>> + Unpin + Send + 'static,
        Executor: Spawn,
        Codec: 'static,
        SinkError: Send + 'static,
        StreamError: Send,
        <Codec as StreamCodec>::Format: Send,
    {
        let (handle, close_rx, listener_sx) = StreamListenerHandle::new();
        executor.spawn(async move {
            self.main_loop(sink, stream, close_rx).await;
            let _ = listener_sx.send(self);
        })?;

        Ok(handle)
    }

    /// Sets up the main loop that sends and receives messages on the givin sink and stream.
    ///
    /// All client APIs that use the transport that this came from will communicate with this,
    /// meaning that nothing will happen until the loop has been started.
    pub fn start_local<Sx, Rx, StreamError, Executor>(
        mut self,
        sink: Sx,
        stream: Rx,
        executor: Executor,
    ) -> Result<StreamListenerHandle<Codec, Codec::Format, SinkError>, SpawnError>
    where
        Sx: Sink<Codec::Format, Error = SinkError> + Unpin + 'static,
        Rx: Stream<Item = Result<Codec::Format, StreamError>> + Unpin + 'static,
        Executor: LocalSpawn,
        Codec: 'static,
        SinkError: 'static,
    {
        let (handle, close_rx, listener_sx) = StreamListenerHandle::new();
        executor.spawn_local(async move {
            self.main_loop(sink, stream, close_rx).await;
            let _ = listener_sx.send(self);
        })?;

        Ok(handle)
    }

    /// This is not cancel-safe as the sink has to be closed once the loop ends.
    /// To get around this, the future should be spawned in an executor and the closer used to
    /// cancel the loop.
    async fn main_loop<Sx, Rx, StreamError>(
        &mut self,
        mut sink: Sx,
        stream: Rx,
        mut close_rx: oneshot::Receiver<()>,
    ) where
        Sx: Sink<Codec::Format, Error = SinkError> + Unpin,
        Rx: Stream<Item = Result<Codec::Format, StreamError>> + Unpin,
    {
        let mut stream = stream.fuse();

        let mut reqs = HashMap::new();
        let mut cref = 0;

        enum Bus<Format, SinkErr, StreamError> {
            RequestRef(oneshot::Sender<u64>),
            SendMessage(BusMsg<Format, SinkErr>),
            RecvMessage(Result<Format, StreamError>),
        }

        // We loop against all these together to maintain some form of sanity.
        // While this _could_ bottleneck requests, it means that clients cannot get out
        // of sync with each other, and by forcing clients to ask for reference IDs from
        // here it ensures that the counter is always unique for the current tunnel.
        // If the tunnel closes and reopens, in-flight requests get retried and the
        // IDs they need will only be given once the counter has been reset.
        while let Some(s) = select! {
            ref_req = self.ref_rx.next() => ref_req.map(Bus::RequestRef),
            bus = self.msg_rx.next() => bus.map(Bus::SendMessage),
            stream = stream.next() => stream.map(Bus::RecvMessage),
            _ = close_rx => None,
        } {
            match s {
                Bus::RequestRef(ref_req_sx) => {
                    let _ = ref_req_sx.send(cref);
                    cref += 1;
                }
                Bus::SendMessage((msg_ref, req, response_sx)) => {
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
                Bus::RecvMessage(Ok(res)) => {
                    let Ok(msg_ref) = self.codec.get_ref(&res) else {
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
                Bus::RecvMessage(Err(_)) => {
                    // println!("err recv");
                    // TODO: decide whether to actually do something useful here
                    break;
                }
            }
        }

        // Once closed we respond to any in-flight requests that the bus has closed to make them
        // try again.
        // Once they do, the first thing they will ask for is new IDs, which will only
        // be given once the bus reopens.
        while let Ok(Some((_, _, response_sx))) = self.msg_rx.try_next() {
            let _ = response_sx.send(Err(BusError::Closed));
        }

        // Nothing we can really do if the sink fails to close.
        let _ = sink.close().await;
    }
}

#[derive(Debug)]
#[must_use = "dropping the handle will close the listener"]
#[clippy::has_significant_drop]
pub struct StreamListenerHandle<Codec, Format, SinkError> {
    close_sx: oneshot::Sender<()>,
    listener_rx: oneshot::Receiver<StreamListener<Codec, Format, SinkError>>,
}

impl<Codec, Format, SinkError> StreamListenerHandle<Codec, Format, SinkError> {
    fn new() -> (
        Self,
        oneshot::Receiver<()>,
        oneshot::Sender<StreamListener<Codec, Format, SinkError>>,
    ) {
        let (close_sx, close_rx) = oneshot::channel();
        let (listener_sx, listener_rx) = oneshot::channel();
        (
            Self {
                close_sx,
                listener_rx,
            },
            close_rx,
            listener_sx,
        )
    }

    pub fn close(self) -> impl Future<Output = StreamListener<Codec, Format, SinkError>> {
        let _ = self.close_sx.send(());
        async move { self.listener_rx.await.unwrap() }
    }
}

impl<Codec, SinkError> Transport for StreamTransport<Codec, Codec::Format, SinkError>
where
    Codec: StreamCodec,
    Codec::Format: netfn_core::compat::NetfnSend + netfn_core::compat::NetfnSync,
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

            let result: TunnelResponse<Res> = codec
                .decode(&result)
                .map_err(|e| TransportError::DecodeError(e))?;
            break Ok(result.data);
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
