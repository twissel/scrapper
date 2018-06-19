#![feature(never_type)]
#![feature(conservative_impl_trait)]

extern crate ex_futures;
#[macro_use]
extern crate futures;
extern crate failure;
extern crate futures_cpupool;
extern crate reqwest;
extern crate select;
extern crate sha1;
extern crate tokio_core;
extern crate url;
#[macro_use]
extern crate failure_derive;

#[macro_use]
extern crate slog;
extern crate bytes;
extern crate sloggers;

mod body;
mod crawler;
mod eos_on_error;
mod fork;
mod request;
mod select_all;
mod sheduler;
mod spider;
mod utils;
use crawler::CrawlerBuilder;
use failure::Error;
use futures::future::{err, ok};
use futures::stream::{iter_ok, once};
use futures::Future;
use futures::Stream;
use request::Request;
use reqwest::unstable::async::{Client, Response};
use reqwest::Method;
use select::document::Document;
use select::predicate::{Attr, Class, Name, Predicate};
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use spider::Parse;
use std::fmt::{self, Display};
use url::{ParseError, Url};

#[allow(dead_code)]
struct Dummy;

#[derive(Debug)]
struct DummyItem;

impl fmt::Display for DummyItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Dummy Item")
    }
}

#[derive(Debug, Fail, Clone)]
struct DummyError;

impl Display for DummyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "An error occurred.")
    }
}

impl spider::Spider for Dummy {
    type Item = DummyItem;

    fn name(&self) -> &'static str {
        "DummySpider"
    }

    fn start(&mut self) -> Box<Future<Item = spider::RequestStream, Error = Error>> {
        let url = "https://google.com".parse().map_err(|e: ParseError| {
            let e: Error = e.into();
            e
        });
        let fut = match url {
            Ok(url) => {
                let req = Request::new(Method::Get, url);
                let stream: spider::RequestStream = Box::new(once::<_, Error>(Ok(Ok(req))));
                let fut = ok(stream);
                Box::new(fut)
            }
            Err(e) => Box::new(err(e)),
        };

        fut
    }

    fn parse(
        &mut self,
        _resp: Response,
    ) -> Box<Future<Item = spider::ParseStream<Self::Item>, Error = Error> + Send> {
        let req = "https://google.com"
            .parse()
            .map_err(|e: ParseError| e.into())
            .map(|url| Parse::Request(Request::new(Method::Get, url)));
        let items = vec![Ok(Parse::Item(DummyItem)), req];

        let stream: spider::ParseStream<Self::Item> = Box::new(iter_ok(items));
        let fut = ok(stream);
        Box::new(fut)
    }
}

#[derive(Debug)]
pub struct XnxxItem {
    url: Url,
}

impl fmt::Display for XnxxItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "XnxxItem(Url({}))", self.url)
    }
}

pub struct XnxxSpider<'a> {
    client: &'a Client,
}

impl<'a> XnxxSpider<'a> {
    fn new(client: &'a Client) -> Self {
        Self { client }
    }
}

impl<'a> spider::Spider for XnxxSpider<'a> {
    type Item = XnxxItem;

    fn name(&self) -> &'static str {
        "XnxxSpider"
    }

    fn start(&mut self) -> Box<Future<Item = spider::RequestStream, Error = Error>> {
        let url: Result<Url, ParseError> = "http://www.xnxx.com/tags".parse();
        match url {
            Ok(url) => {
                let req = Request::new(Method::Get, url.clone());
                let fut = self.client
                    .execute(req.into())
                    .map_err(|e| e.into())
                    .and_then(|resp| {
                        let body = resp.into_body();
                        body.concat2().map_err(|e| e.into())
                    })
                    .map(move |body| {
                        let body = body.to_owned();
                        let body = String::from_utf8_lossy(&body);
                        let doc = Document::from(body.as_ref());
                        let mut output = Vec::new();
                        for tag in doc.find(Attr("id", "tags").descendant(Name("a"))).take(1) {
                            let href = tag.attr("href");
                            if let Some(href) = href {
                                let new = url.join(href)
                                    .map(|url| Request::new(Method::Get, url))
                                    .map_err(|e| e.into());
                                output.push(new);
                            }
                        }
                        Box::new(iter_ok(output)) as spider::RequestStream
                    });
                Box::new(fut)
            }
            Err(e) => Box::new(err(e.into())),
        }
    }

    fn parse(
        &mut self,
        resp: Response,
    ) -> Box<Future<Item = spider::ParseStream<Self::Item>, Error = Error> + Send> {
        let url = resp.url().clone();

        let fut = resp.into_body()
            .concat2()
            .map_err(|e| e.into())
            .map(move |body| {
                let body = body.to_owned();
                let body = String::from_utf8_lossy(&body);
                let doc = Document::from(body.as_ref());
                let mut requests = Vec::new();
                //let mut items = Vec::new();
                for tag in doc.find(Class("pagination").descendant(Name("a"))) {
                    let href = tag.attr("href");
                    if let Some(href) = href {
                        let new = url.join(href).expect("Wrong url");
                        //let item = XnxxItem { url: new.clone() };
                        //items.push(Ok(spider::Parse::Item(item)));
                        requests.push(Ok(spider::Parse::Request(Request::new(Method::Get, new))));
                    }
                }
                let req_stream = iter_ok(requests.into_iter());
                //let item_stream = iter_ok(items.into_iter());
                //let stream = req_stream.select(item_stream);
                //Box::new(stream) as spider::ParseStream<Self::Item>
                Box::new(req_stream) as spider::ParseStream<Self::Item>
            });
        Box::new(fut)
    }
}

fn main() {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let client = Client::new(&core.handle());
    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Debug);
    builder.destination(Destination::Stderr);
    let logger = builder.build().unwrap();

    let sheduler = sheduler::GlobalLimitedSheduler::with_logger(&client, 2, logger.clone());
    let crawler = CrawlerBuilder::new(sheduler)
        .with_logger(logger)
        .build()
        .unwrap();
    let spider = XnxxSpider::new(&client);
    let crawl = crawler.crawl(spider);
    let crawl = crawl.for_each(|_item| Ok(()));

    let res = core.run(crawl);
    println!("{:?}", res);
}
