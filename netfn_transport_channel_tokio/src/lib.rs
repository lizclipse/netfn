#![warn(clippy::pedantic)]

use netfn_core::{Service, Transport};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone)]
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
        while let Some((req, tx)) = self.rx.recv().await {
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
    type Error = TransportError<Req>;

    async fn dispatch(&self, _name: &str, request: Req) -> Result<Res, Self::Error> {
        let (otx, orx) = oneshot::channel();
        self.tx
            .send((request, otx))
            .await
            .map_err(|e| mpsc::error::SendError(e.0 .0))?;
        Ok(orx.await?)
    }
}

#[derive(Error, Debug)]
pub enum TransportError<Req> {
    #[error("failed to send request")]
    Send(#[from] mpsc::error::SendError<Req>),
    #[error("failed to receive response")]
    Receive(#[from] oneshot::error::RecvError),
}
