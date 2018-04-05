#![feature(never_type)]

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

mod crawler;
mod eos_on_error;
mod fork;
mod select_all;
mod sheduler;
mod spider;
mod unique;

use crawler::Crawler;
use failure::{Error, Fail};
use futures::Future;
use futures::Stream;
use futures::future::{err, ok};
use futures::stream::{empty, iter_ok, once};
use reqwest::Method;
use reqwest::unstable::async::{Client, Request, Response};
use select::document::Document;
use select::predicate::{Attr, Class, Name, Predicate};
use spider::Parse;
use std::fmt;
use std::sync::Arc;
use url::{ParseError, Url};

#[allow(dead_code)]
struct Dummy;

#[derive(Debug)]
struct DummyItem;

#[derive(Debug, Fail, Clone)]
struct DummyError;

impl fmt::Display for DummyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "An error occurred.")
    }
}

impl spider::Spider for Dummy {
    type Item = DummyItem;

    fn start(&mut self) -> Box<Future<Item = spider::RequestStream, Error = Error>> {
        let url = "https://google.com".parse().map_err(|e: ParseError| {
            let e: Error = e.into();
            e
        });
        let fut = match url {
            Ok(url) => {
                let req = Request::new(Method::Get, url);
                let stream: spider::RequestStream = Box::new(once::<_, Error>(Ok(req)));
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
    ) -> Box<Future<Item = spider::ParseStream<Self::Item>, Error = Error>> {
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

fn main() {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let client = Client::new(&core.handle());
    let sheduler = sheduler::GlobalLimitedSheduler::new(&client, 2);
    let crawler = Crawler::new(sheduler);
    let spider = Dummy;
    let crawl = crawler.crawl(spider);
    let crawl = crawl.for_each(|item| {
        println!("{:?}", item);
        Ok(())
    });

    let res = core.run(crawl);
    println!("{:?}", res);
}
