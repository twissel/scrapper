use futures::Future;
use futures::stream::Stream;
use reqwest::unstable::async::{Request, Response};

pub enum Parse<T> {
    Request(Request),
    Item(T),
}

pub type RequestStream = Box<Stream<Item = Request, Error = ()>>;
pub type ResponseStream = Box<Stream<Item = Response, Error = ()>>;
pub type ParseStream<T> = Box<Stream<Item = Parse<T>, Error = ()>>;
pub type ItemStream<T> = Box<Stream<Item = T, Error = ()>>;

pub trait Spider
where
    Self::Item: Send + Sized + 'static,
{
    type Item;

    fn start(&mut self) -> Box<Future<Item = RequestStream, Error = ()>>;
    fn parse(
        &mut self,
        response: Response,
    ) -> Box<Future<Item = ParseStream<Self::Item>, Error = ()>>;
}
