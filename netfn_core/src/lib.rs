#![warn(clippy::pedantic)]

use std::future::Future;

pub trait Service {
    const NAME: &'static str;
    type Request;
    type Response;

    fn dispatch(&self, request: Self::Request) -> impl Future<Output = Self::Response> + Send;
}

pub trait Transport<Req, Res> {
    type Error;

    fn dispatch(&self, request: Req) -> impl Future<Output = Result<Res, Self::Error>> + Send;
}
