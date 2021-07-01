use std::future::Future;

use crate::{
    error::{InternalError, SystemError},
    module::Event,
    request::payload::Payload,
    response::Responder,
    util::ready::{ready, Ready},
};

use crate::response::{EventResponse, ResponseBuilder};
use futures_core::ready;
use std::{
    fmt::Debug,
    hash::Hash,
    ops,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Clone, Debug)]
pub struct EventRequest {
    pub(crate) id: String,
    pub(crate) event: Event,
}

impl EventRequest {
    pub fn new<E>(event: E) -> EventRequest
    where
        E: Into<Event>,
    {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event: event.into(),
        }
    }
}

pub trait FromRequest: Sized {
    type Error: Into<SystemError>;
    type Future: Future<Output = Result<Self, Self::Error>>;

    fn from_request(req: &EventRequest, payload: &mut Payload) -> Self::Future;
}

#[doc(hidden)]
impl FromRequest for () {
    type Error = SystemError;
    type Future = Ready<Result<(), SystemError>>;

    fn from_request(_req: &EventRequest, _payload: &mut Payload) -> Self::Future { ready(Ok(())) }
}

#[doc(hidden)]
impl FromRequest for String {
    type Error = SystemError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &EventRequest, payload: &mut Payload) -> Self::Future {
        match &payload {
            Payload::None => ready(Err(unexpected_none_payload(req))),
            Payload::Bytes(buf) => ready(Ok(String::from_utf8_lossy(buf).into_owned())),
        }
    }
}

fn unexpected_none_payload(request: &EventRequest) -> SystemError {
    log::warn!(
        "Event: {:?} expected payload but payload is empty",
        &request.event
    );
    InternalError::new("Expected payload but payload is empty").into()
}

#[doc(hidden)]
impl<T> FromRequest for Result<T, T::Error>
where
    T: FromRequest,
{
    type Error = SystemError;
    type Future = FromRequestFuture<T::Future>;

    fn from_request(req: &EventRequest, payload: &mut Payload) -> Self::Future {
        FromRequestFuture {
            fut: T::from_request(req, payload),
        }
    }
}

#[pin_project::pin_project]
pub struct FromRequestFuture<Fut> {
    #[pin]
    fut: Fut,
}

impl<Fut, T, E> Future for FromRequestFuture<Fut>
where
    Fut: Future<Output = Result<T, E>>,
{
    type Output = Result<Result<T, E>, SystemError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let res = ready!(this.fut.poll(cx));
        Poll::Ready(Ok(res))
    }
}

pub struct Data<T>(pub T);

impl<T> Data<T> {
    pub fn into_inner(self) -> T { self.0 }
}

impl<T> ops::Deref for Data<T> {
    type Target = T;

    fn deref(&self) -> &T { &self.0 }
}

impl<T> ops::DerefMut for Data<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.0 }
}

#[cfg(feature = "use_serde")]
impl<T> FromRequest for Data<T>
where
    T: serde::de::DeserializeOwned + 'static,
{
    type Error = SystemError;
    type Future = Ready<Result<Self, SystemError>>;

    #[inline]
    fn from_request(req: &EventRequest, payload: &mut Payload) -> Self::Future {
        match payload {
            Payload::None => ready(Err(unexpected_none_payload(req))),
            Payload::Bytes(bytes) => {
                let data: T = bincode::deserialize(bytes).unwrap();
                ready(Ok(Data(data)))
            },
        }
    }
}

pub trait FromBytes: Sized {
    fn parse_from_bytes(bytes: &Vec<u8>) -> Result<Self, SystemError>;
}

#[cfg(not(feature = "use_serde"))]
impl<T> FromRequest for Data<T>
where
    T: FromBytes + 'static,
{
    type Error = SystemError;
    type Future = Ready<Result<Self, SystemError>>;

    #[inline]
    fn from_request(req: &EventRequest, payload: &mut Payload) -> Self::Future {
        match payload {
            Payload::None => ready(Err(unexpected_none_payload(req))),
            Payload::Bytes(bytes) => {
                let data = T::parse_from_bytes(bytes).unwrap();
                ready(Ok(Data(data)))
            },
        }
    }
}