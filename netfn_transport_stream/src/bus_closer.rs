use futures::channel::mpsc;

#[derive(Clone, Debug)]
pub struct BusListenerCloser {
    sx: mpsc::Sender<()>,
}

pub type BusListenerCloserReceiver = mpsc::Receiver<()>;

impl BusListenerCloser {
    pub fn new() -> (BusListenerCloserReceiver, Self) {
        let (sx, rx) = mpsc::channel(1);
        (rx, Self { sx })
    }

    pub fn close(&mut self) {
        let _ = self.sx.try_send(());
    }
}
