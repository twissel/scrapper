use futures::Future;
use futures::stream::Stream;
use reqwest::unstable::async::{Request, Response};
use failure::Fail;

pub enum Parse<T> {
    Request(Request),
    Item(T),
}

pub type RequestStream = Box<Stream<Item = Request, Error = Fail>>;
pub type ResponseStream = Box<Stream<Item = Response, Error = Fail>>;
pub type ParseStream<T> = Box<Stream<Item = Parse<T>, Error = Fail>>;
pub type ItemStream<T> = Box<Stream<Item = T, Error = Fail>>;

pub trait Spider
where
    Self::Item: Send + Sized + 'static,
{
    type Item;

    fn start(&mut self) -> Box<Future<Item = RequestStream, Error = Fail>>;
    fn parse(
        &mut self,
        response: Response,
    ) -> Box<Future<Item = ParseStream<Self::Item>, Error = Fail>>;
}
