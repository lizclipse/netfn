#![warn(clippy::pedantic)]

use futures_channel::{mpsc, oneshot};
use futures_util::{lock::Mutex, sink::SinkExt as _, StreamExt};
use netfn_core::{Service, Transport};
use thiserror::Error;

pub struct ChannelTransport<Req, Res> {
    tx: Mutex<mpsc::Sender<(Req, oneshot::Sender<Res>)>>,
}

impl<Req, Res> ChannelTransport<Req, Res>
where
    Req: Send,
    Res: Send,
{
    pub fn new<S>(service: S, buffer_size: usize) -> (Self, ChannelListener<S, Req, Res>)
    where
        S: Service<Request = Req, Response = Res>,
    {
        let (tx, rx) = mpsc::channel(buffer_size);
        (Self { tx: Mutex::new(tx) }, ChannelListener { rx, service })
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
    type Error = Error;

    async fn dispatch(&self, request: Req) -> Result<Res, Self::Error> {
        let (otx, orx) = oneshot::channel();
        self.tx.lock().await.send((request, otx)).await?;
        Ok(orx.await?)
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to send request")]
    Send(#[from] mpsc::SendError),
    #[error("failed to receive response")]
    Receive(#[from] oneshot::Canceled),
}
