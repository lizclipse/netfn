#![warn(clippy::pedantic)]

use std::convert::Infallible;

use netfn_core::Transport;
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use url::{ParseError, Url};

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

impl<Req, Res> Transport<Req, Res> for HttpTransport
where
    Req: Send + Serialize + std::fmt::Debug,
    Res: Send + DeserializeOwned,
{
    type Error = TransportError;

    async fn dispatch(&self, name: &str, request: Req) -> Result<Res, Self::Error> {
        let url = self.url.join(name)?;
        let result = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;

        Ok(result)
    }
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("failed to parse url: {0}")]
    Parse(#[from] ParseError),
    #[error("failed to make request: {0}")]
    Request(#[from] reqwest::Error),
}
