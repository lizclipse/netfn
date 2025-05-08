#![warn(clippy::pedantic)]

use std::convert::Infallible;

use netfn_core::{CallResponseRequest, GenericError, Transport};
use reqwest::Client;
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use url::{ParseError, Url};

const HANDLER_ERROR_CODE: u16 = 537;

#[derive(Debug, Clone)]
pub struct HttpTransport {
    url: Url,
    client: Client,
}

impl HttpTransport {
    #[must_use]
    pub fn new<U, E>(url: U, client: Client) -> Result<Self, Error<E>>
    where
        U: TryInto<Url, Error = E>,
    {
        let url: Url = url.try_into()?;

        if url.cannot_be_a_base() || !url.path().ends_with("/") {
            Err(Error::InvalidUrl(url))
        } else {
            Ok(Self { url, client })
        }
    }
}

impl<'a> TryFrom<&'a str> for HttpTransport {
    type Error = Error<ParseError>;

    fn try_from(url: &'a str) -> Result<Self, Self::Error> {
        Self::new(url, Client::default())
    }
}

impl TryFrom<Url> for HttpTransport {
    type Error = Error<Infallible>;

    fn try_from(url: Url) -> Result<Self, Self::Error> {
        Self::new(url, Client::default())
    }
}

#[derive(Error, Debug)]
pub enum Error<C> {
    #[error("url {0} is not supported for this transport")]
    InvalidUrl(Url),
    #[error("failed to convert url: {0}")]
    Convert(#[from] C),
}

impl Transport for HttpTransport {
    type Error = TransportError;

    async fn call<Req, Res>(&self, service: &'static str, request: Req) -> Result<Res, Self::Error>
    where
        Req: netfn_core::compat::NetfnSend + Serialize,
        Res: netfn_core::compat::NetfnSend + DeserializeOwned,
    {
        let response = self
            .client
            .post(self.url.clone())
            .json(&CallResponseRequest {
                service: service.into(),
                call: request,
            })
            .send()
            .await?;

        if response.status().as_u16() == HANDLER_ERROR_CODE {
            let err: GenericError<'static> = response.json().await?;
            return Err(err.into());
        }

        Ok(response.json().await?)
    }
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("failed to make request: {0}")]
    Request(#[from] reqwest::Error),
    #[error("{0}")]
    Handler(#[from] netfn_core::GenericError<'static>),
}
