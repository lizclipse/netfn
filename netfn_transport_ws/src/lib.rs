use std::sync::{Arc, atomic::AtomicBool};

use netfn_transport_stream::StreamCodec;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

pub enum WebSocketMessage {
    Json(String),
    MessagePack(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct WebSocketCodec {
    use_json: WebSocketCodecToggler,
}

impl WebSocketCodec {
    pub fn new() -> Self {
        Self {
            use_json: WebSocketCodecToggler(Arc::new(AtomicBool::new(false))),
        }
    }

    pub fn use_json(&self) -> bool {
        self.use_json.value()
    }

    pub fn set_use_json(&self, value: bool) {
        self.use_json.set(value);
    }

    pub fn use_json_toggle(&self) -> WebSocketCodecToggler {
        self.use_json.clone()
    }
}

impl StreamCodec for WebSocketCodec {
    type Format = WebSocketMessage;
    type EncodeError = WebSocketEncodeError;
    type DecodeError = WebSocketDecodeError;

    fn encode<T>(&self, value: &T) -> Result<Self::Format, Self::EncodeError>
    where
        T: Serialize,
    {
        Ok(match self.use_json() {
            false => WebSocketMessage::MessagePack(rmp_serde::to_vec(value)?),
            true => WebSocketMessage::Json(serde_json::to_string(value)?),
        })
    }

    fn decode<T>(&self, message: &Self::Format) -> Result<T, Self::DecodeError>
    where
        T: DeserializeOwned,
    {
        Ok(match message {
            WebSocketMessage::MessagePack(msg) => rmp_serde::from_slice(msg)?,
            WebSocketMessage::Json(msg) => serde_json::from_str(msg)?,
        })
    }

    fn get_ref(&self, message: &Self::Format) -> Result<Option<u64>, Self::DecodeError> {
        let partial: PartialRefs = self.decode(message)?;
        return Ok(partial.msg_ref);
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketCodecToggler(Arc<AtomicBool>);

impl WebSocketCodecToggler {
    pub fn value(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set(&self, value: bool) {
        self.0.store(value, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Error, Debug)]
pub enum WebSocketEncodeError {
    #[error("json encode error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("messagepack encode error: {0}")]
    MessagePack(#[from] rmp_serde::encode::Error),
}

#[derive(Error, Debug)]
pub enum WebSocketDecodeError {
    #[error("json decode error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("messagepack decode error: {0}")]
    MessagePack(#[from] rmp_serde::decode::Error),
}

#[derive(Deserialize)]
struct PartialRefs {
    #[serde(rename = "ref")]
    msg_ref: Option<u64>,
    // handle: Option<u64>,
}
