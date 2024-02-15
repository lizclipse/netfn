use std::future::Future;

pub use netfn_gen::*;
pub use netfn_macro::*;

pub struct Header {
    pub api: &'static str,
    pub method: &'static str,
}

pub enum BackendError<T> {
    ConnectionFailed,
    Res(T),
}

pub trait ClientBackend {
    type Encoding;

    fn send(
        &mut self,
        header: Header,
        params: Self::Encoding,
    ) -> impl Future<Output = Result<Self::Encoding, BackendError<Self::Encoding>>> + Send;
}
