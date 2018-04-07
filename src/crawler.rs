use eos_on_error::EosOnErrorExt;
use ex_futures::stream::StreamExt;
use failure::Error;
use futures::stream::FuturesUnordered;
use futures::{Async, Future, Poll, Stream};
use select_all::SelectAll;
use sheduler::*;
use slog::Logger;
use spider::*;
use std::fmt::Display;

pub struct Crawler<SH>
where
    SH: Sheduler,
{
    sheduler: SH,
    logger: Option<Logger>,
}

pub struct Crawl<S, SH>
where
    S: Spider,
{
    spider: S,
    sheduler: SH,
    parsing: FuturesUnordered<Box<Future<Item = ParseStream<S::Item>, Error = Error>>>,
    output: SelectAll<ItemStream<S::Item>>,
    logger: Option<Logger>,
}

impl<S, SH> Stream for Crawl<S, SH>
where
    S: Spider,
    SH: Sheduler,
{
    type Item = S::Item;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Async::Ready(Some(resp)) = self.sheduler.poll()? {
            let parse_fut = self.spider.parse(resp);
            self.parsing.push(Box::new(parse_fut));
        }

        if let Async::Ready(Some(parsed)) = self.parsing.poll()? {
            let parsed = filter_and_log_errors(parsed, &self.logger).eos_on_error(&self.logger);
            let (new_requests, new_items) = parsed.unsync_fork(|item| match item {
                &Parse::Request(_) => true,
                _ => false,
            });

            let new_requests = new_requests.map(|item| match item {
                Parse::Request(req) => req,
                _ => unreachable!("requests stream got item"),
            });

            let mut new_items = new_items.map(|item| match item {
                Parse::Item(item) => item,
                _ => unreachable!("items stream got request"),
            });

            let new_items = match self.logger {
                Some(ref logger) => {
                    let log = logger.clone();
                    let inspect = new_items
                        .inspect(move |item| info!(log, "new item received"; "item" => %item));
                    Box::new(inspect) as ItemStream<Self::Item>
                }
                None => Box::new(new_items) as ItemStream<Self::Item>,
            };

            self.sheduler.shedule(Box::new(new_requests));
            self.output.push(new_items);
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
        let start_stream = spider.start().flatten_stream();
        let start_stream =
            filter_and_log_errors(start_stream, &self.logger).eos_on_error(&self.logger);

        let start_stream: InternalRequestStream = Box::new(start_stream);
        let parsing = FuturesUnordered::new();
        let output = SelectAll::new();
        let mut sheduler = self.sheduler;
        let name = spider.name();
        let logger = match self.logger {
            Some(ref logger) => Some(logger.new(o!("Crawler" => name))),
            None => None,
        };
        sheduler.shedule(start_stream);
        Crawl {
            spider,
            sheduler,
            parsing,
            output,
            logger,
        }
    }

    pub fn new(sheduler: SH) -> Crawler<SH> {
        let logger = None;
        Crawler { sheduler, logger }
    }

    pub fn with_logger(sheduler: SH, logger: Logger) -> Crawler<SH> {
        let logger = Some(logger);
        Crawler { sheduler, logger }
    }
}

fn filter_and_log_errors<S, T, E, SE>(
    stream: S,
    logger: &Option<Logger>,
) -> Box<Stream<Item = T, Error = SE>>
where
    S: Stream<Item = Result<T, E>, Error = SE> + 'static,
    E: Display,
{
    match logger {
        &Some(ref logger) => {
            let logger_clone = logger.clone();
            let stream = stream.filter_map(move |item| match item {
                Ok(req) => Some(req),
                Err(e) => {
                    error!(logger_clone, "error received"; "error" => %e);
                    None
                }
            });
            Box::new(stream) as Box<Stream<Item = T, Error = SE>>
        }
        &None => {
            let stream = stream.filter_map(|item| match item {
                Ok(req) => Some(req),
                Err(_) => None,
            });
            Box::new(stream)
        }
    }
}
