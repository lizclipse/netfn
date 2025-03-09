#![warn(clippy::pedantic)]

use futures::{
    StreamExt as _,
    channel::{mpsc, oneshot},
    sink::SinkExt as _,
};
use netfn_core::{Service, Transport};
use thiserror::Error;

#[derive(Debug)]
pub struct ChannelTransport<Req, Res> {
    tx: mpsc::Sender<(Req, oneshot::Sender<Res>)>,
}

impl<Req, Res> ChannelTransport<Req, Res>
where
    Req: Send,
    Res: Send,
{
    #[must_use]
    pub fn new<S>(service: S, buffer_size: usize) -> (Self, ChannelListener<S, Req, Res>)
    where
        S: Service<Request = Req, Response = Res>,
    {
        let (tx, rx) = mpsc::channel(buffer_size);
        (Self { tx }, ChannelListener { rx, service })
    }
}

pub struct ChannelListener<S, Req, Res> {
    rx: mpsc::Receiver<(Req, oneshot::Sender<Res>)>,
    service: S,
}

impl<S, Req, Res> ChannelListener<S, Req, Res>
where
    S: Service<Request = Req, Response = Res>,
    Req: Send,
    Res: Send,
{
    pub async fn listen(mut self) {
        while let Some((req, tx)) = self.rx.next().await {
            let res = self.service.dispatch(req).await;
            let _ = tx.send(res);
        }
    }
}

impl<Req, Res> Transport<Req, Res> for ChannelTransport<Req, Res>
where
    Req: Send,
    Res: Send,
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
