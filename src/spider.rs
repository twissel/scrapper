use failure::{Error, Fail};
use futures::Future;
use futures::stream::Stream;
use reqwest::unstable::async::{Request, Response};

pub enum Parse<T> {
    Request(Request),
    Item(T),
}

pub type RequestStream = Box<Stream<Item = Result<Request, Error>, Error = Error>>;
pub type ResponseStream = Box<Stream<Item = Response, Error = Error>>;
pub type ParseStream<T> = Box<Stream<Item = Result<Parse<T>, Error>, Error = Error>>;
pub type ItemStream<T> = Box<Stream<Item = T, Error = !>>;
pub type InternalRequestStream = Box<Stream<Item = Request, Error = !>>;

pub trait Spider
where
    Self::Item: Sized + 'static,
{
    type Item;

    fn start(&mut self) -> Box<Future<Item = RequestStream, Error = Error>>;
    fn parse(
        &mut self,
        response: Response,
    ) -> Box<Future<Item = ParseStream<Self::Item>, Error = Error>>;
}
