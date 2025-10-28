use futures::task::Spawn;
use netfn_transport_stream::StreamCodec;
use serde::{Serialize, de::DeserializeOwned};
use tokio_tungstenite::tungstenite::Message;

use crate::{PartialRefs, WebSocketCodecToggler, WebSocketDecodeError, WebSocketEncodeError};

#[derive(Debug, Clone)]
pub struct TokioTungsteniteWebSocketCodec {
    use_json: WebSocketCodecToggler,
}

impl TokioTungsteniteWebSocketCodec {
    pub fn new() -> Self {
        Self {
            use_json: WebSocketCodecToggler::new(),
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

impl StreamCodec for TokioTungsteniteWebSocketCodec {
    type Format = Message;
    type EncodeError = WebSocketEncodeError;
    type DecodeError = WebSocketDecodeError;

    fn encode<T>(&self, value: &T) -> Result<Self::Format, Self::EncodeError>
    where
        T: Serialize,
    {
        Ok(match self.use_json() {
            false => Message::binary(rmp_serde::to_vec(value)?),
            true => Message::text(serde_json::to_string(value)?),
        })
    }

    fn decode<T>(&self, message: &Self::Format) -> Result<T, Self::DecodeError>
    where
        T: DeserializeOwned,
    {
        match message {
            Message::Binary(msg) => Ok(rmp_serde::from_slice(msg)?),
            Message::Text(msg) => Ok(serde_json::from_str(msg)?),
            _ => Err(WebSocketDecodeError::NotData),
        }
    }

    fn get_ref(&self, message: &Self::Format) -> Result<Option<u64>, Self::DecodeError> {
        let partial: PartialRefs = self.decode(message)?;
        return Ok(partial.msg_ref);
    }
}

pub struct TokioSpawner(tokio::runtime::Handle);

impl TokioSpawner {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        TokioSpawner(handle)
    }

    pub fn current() -> Self {
        TokioSpawner::new(tokio::runtime::Handle::current())
    }
}

impl Spawn for TokioSpawner {
    fn spawn_obj(
        &self,
        obj: futures::task::FutureObj<'static, ()>,
    ) -> Result<(), futures::task::SpawnError> {
        self.0.spawn(obj);
        Ok(())
    }
}
