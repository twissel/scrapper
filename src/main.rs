#[macro_use]
extern crate futures;
extern crate ex_futures;
extern crate reqwest;
extern crate tokio_core;
extern crate url;

mod spider;
mod crawler;
mod select_all;

struct Dummy;
#[derive(Debug)]
struct DummyItem;

use futures::stream::{empty, iter_ok, once};
use futures::Stream;
use futures::Future;
use futures::future::ok;
use reqwest::unstable::async::{Client, Request, Response};
use reqwest::Method;
use url::Url;
use crawler::Crawler;
use spider::Parse;

impl spider::Spider for Dummy {
    type Item = DummyItem;

    fn start(&mut self) -> Box<Future<Item = spider::RequestStream, Error = ()>> {
        let req = Request::new(Method::Get, "https://google.com".parse().unwrap());
        let stream: spider::RequestStream = Box::new(once::<_, ()>(Ok(req)));
        let fut = ok(stream);
        Box::new(fut)
    }

    fn parse(
        &mut self,
        _resp: Response,
    ) -> Box<Future<Item = spider::ParseStream<Self::Item>, Error = ()>> {
        let req = Request::new(Method::Get, "https://google.com".parse().unwrap());
        let items = vec![Parse::Item(DummyItem), Parse::Request(req)];

        let stream: spider::ParseStream<Self::Item> = Box::new(iter_ok(items));
        let fut = ok(stream);
        Box::new(fut)
    }
}

fn main() {
    let spider = Dummy;
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let client = Client::new(&core.handle());
    let crawler = Crawler::new(&client);
    let crawl = crawler.crawl(spider).for_each(|item| {
        println!("{:?}", item);
        Ok(())
    });

    core.run(crawl).unwrap();
}
