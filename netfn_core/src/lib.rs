#![warn(clippy::pedantic)]

use std::{future::Future, sync::Arc};

pub trait Service {
    const NAME: &'static str;
    type Request;
    type Response;

    fn dispatch(&self, request: Self::Request) -> impl Future<Output = Self::Response> + Send;
}

pub trait Transport<Req, Res> {
    type Error;

    fn dispatch(
        &self,
        name: &str,
        request: Req,
    ) -> impl Future<Output = Result<Res, Self::Error>> + Send;
}

impl<T, Req, Res, E> Transport<Req, Res> for Arc<T>
where
    T: Transport<Req, Res, Error = E>,
{
    type Error = E;

    fn dispatch(
        &self,
        name: &str,
        request: Req,
    ) -> impl Future<Output = Result<Res, Self::Error>> + Send {
        (&**self).dispatch(name, request)
    }
}
