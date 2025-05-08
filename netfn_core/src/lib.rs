#![warn(clippy::pedantic)]

use std::{borrow::Cow, error::Error, fmt::Display};

#[doc(hidden)]
pub use serde;
use serde::{Deserialize, Serialize};

pub trait Service {
    const NAME: &'static str;
    type Request;
    type Response;

    fn call(
        &self,
        request: Self::Request,
    ) -> impl Future<Output = Self::Response> + compat::NetfnSend;
}

pub trait Transport {
    type Error;

    fn call<Req, Res>(
        &self,
        service: &'static str,
        request: Req,
    ) -> impl Future<Output = Result<Res, Self::Error>> + compat::NetfnSend
    where
        Req: compat::NetfnSend + Serialize,
        Res: compat::NetfnSend + serde::de::DeserializeOwned;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallResponseRequest<'a, T> {
    pub service: Cow<'a, str>,
    pub call: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenericError<'a> {
    pub code: Cow<'a, str>,
    pub message: Cow<'a, str>,
}

impl Display for GenericError<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl Error for GenericError<'_> {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelMessage<'a, T> {
    Request(TunnelRequest<'a, T>),
    Response(TunnelResponse<T>),
    StreamOpen(TunnelStreamOpen<'a, T>),
    StreamReady(TunnelStreamReady),
    StreamMessage(TunnelStreamMessage<T>),
    StreamClose(TunnelStreamClose),
    Error(TunnelCallError<'a>),
    StreamError(TunnelStreamError<'a>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelRequest<'a, T> {
    #[serde(rename = "ref")]
    pub msg_ref: u64,
    #[serde(flatten)]
    pub payload: CallResponseRequest<'a, T>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelResponse<T> {
    #[serde(rename = "ref")]
    pub msg_ref: u64,
    pub data: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelStreamOpen<'a, T> {
    #[serde(rename = "ref")]
    pub msg_ref: u64,
    #[serde(flatten)]
    pub payload: CallResponseRequest<'a, T>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelStreamReady {
    #[serde(rename = "ref")]
    pub msg_ref: u64,
    pub handle: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelStreamMessage<T> {
    pub handle: u64,
    pub data: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelStreamClose {
    pub handle: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelCallError<'a> {
    #[serde(rename = "ref")]
    pub msg_ref: u64,
    pub error: GenericError<'a>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelStreamError<'a> {
    pub handle: u64,
    pub error: GenericError<'a>,
}

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub mod compat {
    pub trait NetfnSync: Sync {}
    impl<T> NetfnSync for T where T: Sync {}

    pub trait NetfnSend: Send {}
    impl<T> NetfnSend for T where T: Send {}
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod compat {
    pub trait NetfnSync {}
    impl<T> NetfnSync for T {}

    pub trait NetfnSend {}
    impl<T> NetfnSend for T {}
}
