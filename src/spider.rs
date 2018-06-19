use failure::Error;
use futures::stream::Stream;
use futures::Future;
use request::Request;
use reqwest::unstable::async::Response;
use std::fmt::Display;

pub enum Parse<T: Send> {
    Request(Request),
    Item(T),
}

pub type RequestStream = Box<Stream<Item = Result<Request, Error>, Error = Error>>;
pub type ParseStream<T> = Box<Stream<Item = Result<Parse<T>, Error>, Error = Error> + Send>;
pub type ItemStream<T> = Box<Stream<Item = T, Error = !>>;
pub type InternalRequestStream = Box<Stream<Item = Request, Error = !>>;

pub trait Spider
where
    Self::Item: Sized + Display + Send + 'static,
{
    type Item;

    fn name(&self) -> &'static str;

    fn start(&mut self) -> Box<Future<Item = RequestStream, Error = Error>>;
    fn parse(
        &mut self,
        response: Response,
    ) -> Box<Future<Item = ParseStream<Self::Item>, Error = Error> + Send>;
}
