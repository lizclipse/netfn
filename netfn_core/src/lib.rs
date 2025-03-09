#![warn(clippy::pedantic)]

use std::sync::Arc;

pub trait Service {
    type Request;
    type Response;

    fn dispatch(&self, request: Self::Request) -> impl Future<Output = Self::Response> + Send;
}

pub trait Transport<Req, Res> {
    type Error;

    #[cfg(target_arch = "wasm32")]
    fn dispatch(&self, request: Req) -> impl Future<Output = Result<Res, Self::Error>>;

    #[cfg(not(target_arch = "wasm32"))]
    fn dispatch(&self, request: Req) -> impl Future<Output = Result<Res, Self::Error>> + Send;
}

impl<T, Req, Res, E> Transport<Req, Res> for Arc<T>
where
    T: Transport<Req, Res, Error = E>,
{
    type Error = E;

    #[cfg(target_arch = "wasm32")]
    fn dispatch(&self, request: Req) -> impl Future<Output = Result<Res, Self::Error>> {
        (&**self).dispatch(request)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dispatch(&self, request: Req) -> impl Future<Output = Result<Res, Self::Error>> + Send {
        (&**self).dispatch(request)
    }
}
