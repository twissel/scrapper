use eos_on_error::EosOnErrorExt;
use ex_futures::stream::StreamExt;
use failure::Error;
use futures::stream::FuturesUnordered;
use futures::{Async, Future, Poll, Stream};
use select_all::SelectAll;
use sheduler::*;
use spider::*;

pub struct Crawler<SH>
where
    SH: Sheduler,
{
    sheduler: SH,
}

pub struct Crawl<S, SH>
where
    S: Spider,
{
    spider: S,
    sheduler: SH,
    parsing: FuturesUnordered<Box<Future<Item = ParseStream<S::Item>, Error = Error>>>,
    output: SelectAll<ItemStream<S::Item>>,
}

impl<S, SH> Stream for Crawl<S, SH>
where
    S: Spider,
    SH: Sheduler,
{
    type Item = S::Item;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // try to parse  as much responses, as we can.
        while let Async::Ready(Some(resp)) = self.sheduler.poll()? {
            let parse_fut = self.spider.parse(resp);
            self.parsing.push(Box::new(parse_fut));
        }

        while let Async::Ready(Some(parsed)) = self.parsing.poll()? {
            let parsed = parsed.eos_on_error().filter_map(|item| {
                match item {
                    Ok(item) => Some(item),
                    Err(_) => {
                        // TODO: log errors
                        None
                    }
                }
            });
            let (new_requests, new_items) = parsed.unsync_fork(|item| match item {
                &Parse::Request(_) => true,
                _ => false,
            });

            let new_requests = new_requests.map(|item| match item {
                Parse::Request(req) => req,
                _ => unreachable!("requests stream got item"),
            });

            let new_items = new_items.map(|item| match item {
                Parse::Item(item) => item,
                _ => unreachable!("items stream got request"),
            });

            self.sheduler.shedule(Box::new(new_requests));
            self.output.push(Box::new(new_items));
        }

        if let Async::Ready(Some(item)) = self.output.poll()? {
            return Ok(Async::Ready(Some(item)));
        }

        if self.sheduler.is_done() && self.parsing.is_empty() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

impl<SH> Crawler<SH>
where
    SH: Sheduler,
{
    pub fn crawl<S>(self, mut spider: S) -> Crawl<S, SH>
    where
        S: Spider,
    {
        let start_stream = spider
            .start()
            .map_err(|e| {
                let err: Error = e.into();
                err
            })
            .map(|stream| {
                stream.map_err(|e| {
                    let err: Error = e.into();
                    err
                })
            });

        let start_stream: InternalRequestStream =
            Box::new(start_stream.flatten_stream().eos_on_error());
        let parsing = FuturesUnordered::new();
        let output = SelectAll::new();
        let mut sheduler = self.sheduler;
        sheduler.shedule(start_stream);
        Crawl {
            spider,
            sheduler,
            parsing,
            output,
        }
    }

    pub fn new(sheduler: SH) -> Crawler<SH> {
        Crawler { sheduler }
    }
}
