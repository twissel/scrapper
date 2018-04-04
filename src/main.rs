extern crate ex_futures;
#[macro_use]
extern crate futures;
extern crate failure;
extern crate reqwest;
extern crate select;
extern crate tokio_core;
extern crate url;
#[macro_use]
extern crate failure_derive;

mod spider;
mod crawler;
mod select_all;
mod unique;
mod sheduler;
mod fork;

use futures::stream::{empty, iter_ok, once};
use futures::Stream;
use futures::Future;
use futures::future::ok;
use reqwest::unstable::async::{Client, Request, Response};
use select::document::Document;
use select::predicate::{Attr, Class, Name, Predicate};
use reqwest::Method;
use url::Url;
use crawler::Crawler;
use spider::Parse;

#[allow(dead_code)]
struct Dummy;
#[derive(Debug)]
struct DummyItem;

#[derive(Debug)]
pub struct XnxxItem {
    url: Url,
}

pub struct XnxxSpider<'a> {
    client: &'a Client,
    num_parsed: usize,
}

impl<'a> XnxxSpider<'a> {
    fn new(client: &'a Client) -> Self {
        let num_parsed = 0;
        Self { client, num_parsed }
    }
}

impl<'a> spider::Spider for XnxxSpider<'a> {
    type Item = XnxxItem;

    fn start(&mut self) -> Box<Future<Item = spider::RequestStream, Error = ()>> {
        let url: Url = "http://www.xnxx.com/tags".parse().unwrap();
        let fut = self.client
            .request(Method::Get, url.clone())
            .send()
            .map_err(|_| ())
            .and_then(|resp| {
                let body = resp.into_body();
                body.concat2().map_err(|_| ())
            })
            .map(move |body| {
                let body = body.to_owned();
                let body = String::from_utf8_lossy(&body);
                let doc = Document::from(body.as_ref());
                let mut requests = Vec::new();
                for tag in doc.find(Attr("id", "tags").descendant(Name("a"))).take(1) {
                    let href = tag.attr("href");
                    if let Some(href) = href {
                        let new = url.join(href).expect("Wrong url");
                        requests.push(Request::new(Method::Get, new.clone()));
                    }
                }
                Box::new(iter_ok::<_, ()>(requests.into_iter())) as spider::RequestStream
            });
        Box::new(fut)
    }

    fn parse(
        &mut self,
        resp: Response,
    ) -> Box<Future<Item = spider::ParseStream<Self::Item>, Error = ()>> {
        let url = resp.url().clone();
        self.num_parsed += 1;
        if self.num_parsed < 2 {
            let fut = resp.into_body().concat2().map_err(|_| ()).map(move |body| {
                let body = body.to_owned();
                let body = String::from_utf8_lossy(&body);
                let doc = Document::from(body.as_ref());
                let mut requests = Vec::<Parse<Self::Item>>::new();
                let mut items = Vec::<Parse<Self::Item>>::new();
                for tag in doc.find(Class("pagination").descendant(Name("a"))).take(1) {
                    let href = tag.attr("href");
                    if let Some(href) = href {
                        let new = url.join(href).expect("Wrong url");
                        let item = XnxxItem { url: new.clone() };
                        items.push(spider::Parse::Item(item));
                        requests.push(spider::Parse::Request(Request::new(Method::Get, new)));
                    }
                }
                let req_stream = iter_ok(requests.into_iter());
                let item_stream = iter_ok(items.into_iter());
                let stream = req_stream.select(item_stream);
                Box::new(stream) as spider::ParseStream<Self::Item>
            });
            Box::new(fut)
        } else {
            let stream = Box::new(empty()) as spider::ParseStream<Self::Item>;
            Box::new(ok(stream))
        }
    }
}

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
        //let req = Request::new(Method::Get, "https://google.com".parse().unwrap());
        let items = vec![Parse::Item(DummyItem)];

        let stream: spider::ParseStream<Self::Item> = Box::new(iter_ok(items));
        let fut = ok(stream);
        Box::new(fut)
    }
}

fn main() {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let client = Client::new(&core.handle());
    let sheduler = sheduler::GlobalLimitedSheduler::new(&client, 2);
    let crawler = Crawler::new(sheduler);
    let spider = XnxxSpider::new(&client);
    let crawl = crawler.crawl(spider).for_each(|item| {
        println!("{:?}", item);
        Ok(())
    });

    let res = core.run(crawl);
    println!("{:?}", res);
}
