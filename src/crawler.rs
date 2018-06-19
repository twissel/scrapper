use eos_on_error::EosOnErrorExt;
use ex_futures::stream::StreamExt;
use failure::Error;
use futures::stream::FuturesUnordered;
use futures::{Async, Future, Poll, Stream};
use futures_cpupool::CpuPool;
use select_all::SelectAll;
use sheduler::*;
use slog::Logger;
use spider::*;
use std::cell::RefCell;
use std::rc::Rc;
use utils::{filter_and_log_errors, get_digest_and_request, RFPFilter};

pub struct Crawler<SH>
where
    SH: Sheduler,
{
    sheduler: Rc<RefCell<SH>>,
    logger: Option<Logger>,
    pool: CpuPool,
    parse_settings: ParseSettings,
}

pub struct Crawl<S, SH>
where
    S: Spider,
{
    spider: S,
    sheduler: Rc<RefCell<SH>>,
    parsing: FuturesUnordered<Box<Future<Item = ParseStream<S::Item>, Error = Error>>>,
    output: SelectAll<ItemStream<S::Item>>,
    logger: Option<Logger>,
    pool: CpuPool,
    parse_settings: ParseSettings,
    rfp_filter: RFPFilter,
}

impl<S, SH> Crawl<S, SH>
where
    S: Spider,
{
    fn wrap_parse_future(
        &self,
        fut: Box<Future<Item = ParseStream<S::Item>, Error = Error> + Send>,
    ) -> Box<Future<Item = ParseStream<S::Item>, Error = Error> + Send> {
        match self.parse_settings {
            ParseSettings::OnPool => {
                let fut = self.pool.spawn(fut);
                Box::new(fut)
            }
            ParseSettings::SameThread => fut,
        }
    }
}

impl<S, SH> Stream for Crawl<S, SH>
where
    S: Spider,
    SH: Sheduler,
{
    type Item = S::Item;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        {
            let mut sheduler = self.sheduler.borrow_mut();
            if let Async::Ready(Some(resp)) = sheduler.poll()? {
                let parse_fut = self.spider.parse(resp);
                let parse_fut = self.wrap_parse_future(parse_fut);
                self.parsing.push(Box::new(parse_fut));
            }
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

            let pool = self.pool.clone();

            let new_requests = new_requests
                .map(move |req| pool.spawn_fn(|| Ok(get_digest_and_request(req))))
                .buffered(4);

            let new_requests = self.rfp_filter.unique(new_requests);

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

            {
                let mut sheduler = self.sheduler.borrow_mut();
                sheduler.shedule(Box::new(new_requests));
            }

            self.output.push(new_items);
        }

        if let Async::Ready(Some(item)) = self.output.poll()? {
            return Ok(Async::Ready(Some(item)));
        }

        let sheduler = self.sheduler.borrow();

        if sheduler.is_done() && self.parsing.is_empty() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

#[allow(dead_code)]
impl<SH> Crawler<SH>
where
    SH: Sheduler,
{
    pub fn crawl<S>(&self, mut spider: S) -> Crawl<S, SH>
    where
        S: Spider,
    {
        let pool = self.pool.clone();
        let cloned_pool = self.pool.clone();
        let start_stream = spider.start().flatten_stream();
        let start_stream = filter_and_log_errors(start_stream, &self.logger)
            .eos_on_error(&self.logger)
            .map(move |req| cloned_pool.spawn_fn(|| Ok(get_digest_and_request(req))))
            .buffered(4);

        let parsing = FuturesUnordered::new();
        let output = SelectAll::new();
        let sheduler = self.sheduler.clone();
        let name = spider.name();
        let parse_settings = self.parse_settings.clone();

        let logger = match self.logger {
            Some(ref logger) => Some(logger.new(o!("Crawler" => name))),
            None => None,
        };

        let rfp_filter = RFPFilter::new(pool.clone(), logger.clone());
        let start_stream: InternalRequestStream = Box::new(rfp_filter.unique(start_stream));

        {
            let mut borrowed = sheduler.borrow_mut();
            borrowed.shedule(start_stream);
        }

        Crawl {
            spider,
            sheduler,
            parsing,
            output,
            logger,
            pool,
            parse_settings,
            rfp_filter,
        }
    }
}

pub struct CrawlerBuilder<SH> {
    sheduler: SH,
    logger: Option<Logger>,
    pool: Option<CpuPool>,
    parse_settings: Option<ParseSettings>,
}

#[derive(Clone)]
pub enum ParseSettings {
    OnPool,
    SameThread,
}

impl<SH> CrawlerBuilder<SH>
where
    SH: Sheduler,
{
    pub fn new(sheduler: SH) -> Self {
        let logger = None;
        let pool = None;
        let parse_settings = None;
        Self {
            logger,
            sheduler,
            pool,
            parse_settings,
        }
    }

    pub fn with_logger(mut self, logger: Logger) -> Self {
        self.logger = Some(logger);
        self
    }

    pub fn build(self) -> Result<Crawler<SH>, Error> {
        let logger = self.logger;
        let sheduler = Rc::new(RefCell::new(self.sheduler));
        let pool = match self.pool {
            Some(pool) => pool,
            None => CpuPool::new_num_cpus(),
        };

        let parse_settings = match self.parse_settings {
            Some(settings) => settings,
            None => ParseSettings::OnPool,
        };

        Ok(Crawler {
            logger,
            sheduler,
            pool,
            parse_settings,
        })
    }
}
