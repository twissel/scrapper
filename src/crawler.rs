use ex_futures::stream::StreamExt;
use futures::stream::empty;
use futures::stream::{Chain, Fuse, FuturesUnordered};
use futures::{Async, Future, Poll, Stream};
use reqwest::unstable::async::{Client, Request, Response};
use select_all::SelectAll;
use spider::*;
use std::mem;

pub struct Crawler<'a> {
    client: &'a Client,
}

pub struct Crawl<'a, S>
where
    S: Spider,
{
    spider: S,
    sheduler: Sheduler,
    client: &'a Client,
    executing: FuturesUnordered<Box<Future<Item = Response, Error = ()>>>,
    parsing: FuturesUnordered<Box<Future<Item = ParseStream<S::Item>, Error = ()>>>,
    output: SelectAll<ItemStream<S::Item>>,
}

struct Sheduler {
    queque: Option<Fuse<RequestStream>>,
}

impl Sheduler {
    fn add_requests(&mut self, requests: RequestStream) {
        let current_queque = self.queque.take();
        match current_queque {
            Some(queque) => {
                let new_queue = (Box::new(queque.chain(requests)) as RequestStream).fuse();
                self.queque = Some(new_queue);
            }
            None => unreachable!("current query is None"),
        }
    }

    fn new(requests: RequestStream) -> Sheduler {
        let queque = requests.fuse();
        Sheduler {
            queque: Some(queque),
        }
    }

    fn is_empty(&self) -> bool {
        self.queque.as_ref().unwrap().is_done()
    }
}

impl Stream for Sheduler {
    type Item = Request;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.queque.as_mut().unwrap().poll()
    }
}

impl<'a, S> Stream for Crawl<'a, S>
where
    S: Spider,
{
    type Item = S::Item;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // try to execute  as much requests, as we can.
        while let Async::Ready(Some(req)) = self.sheduler.poll()? {
            let fut = self.client.execute(req).map_err(|_| ());
            self.executing.push(Box::new(fut));
        }

        // try to parse  as much responses, as we can.
        while let Async::Ready(Some(resp)) = self.executing.poll()? {
            let parse_fut = self.spider.parse(resp);
            self.parsing.push(Box::new(parse_fut));
        }

        if let Async::Ready(Some(parsed)) = self.parsing.poll()? {
            let (new_requests, new_items) = parsed.fork(|item| match item {
                &Parse::Request(_) => true,
                _ => false,
            });

            let new_requests = new_requests.map(|item| match item {
                Parse::Request(req) => req,
                _ => unreachable!("requests stream got item"),
            });

            let new_items = new_items.map(|item| match item {
                Parse::Item(item) => item,
                _ => unreachable!("items stream got requests"),
            });

            self.sheduler.add_requests(Box::new(new_requests));
            self.output.push(Box::new(new_items));
        }

        if let Async::Ready(Some(item)) = self.output.poll()? {
            return Ok(Async::Ready(Some(item)));
        }

        if self.sheduler.is_empty() && self.executing.is_empty() && self.parsing.is_empty() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

impl<'a> Crawler<'a> {
    pub fn crawl<S>(&self, mut spider: S) -> Crawl<S>
    where
        S: Spider,
    {
        let client = self.client;
        let start_stream: RequestStream = Box::new(spider.start().flatten_stream());
        let sheduler = Sheduler::new(start_stream);
        let executing = FuturesUnordered::new();

        let parsing = FuturesUnordered::new();
        let output = SelectAll::new();
        Crawl {
            spider,
            sheduler,
            client,
            executing,
            parsing,
            output,
        }
    }

    pub fn new(client: &Client) -> Crawler {
        Crawler { client }
    }
}
