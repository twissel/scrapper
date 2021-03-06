use failure::Error;
use futures::stream::{empty, Fuse, FuturesUnordered, Stream};
use futures::task::{current, Task};
use futures::{Async, Future, Poll};
use request::Request;
use reqwest::unstable::async::{Client, Response};
use slog::Logger;
use spider::InternalRequestStream;
use std::convert::From;

pub trait Sheduler: Stream<Error = Error, Item = Response> {
    fn shedule(&mut self, requests: InternalRequestStream);
    fn is_done(&self) -> bool;
}

struct ShedulerRequestStream(Option<Fuse<InternalRequestStream>>, Option<Task>);

impl ShedulerRequestStream {
    pub fn chain(&mut self, requests: InternalRequestStream) {
        let current_stream = self.0.take();
        match current_stream {
            Some(current_stream) => {
                let inner_stream = current_stream.into_inner();
                let requests = requests.filter(filter_request);
                let new_stream =
                    (Box::new(inner_stream.chain(requests)) as InternalRequestStream).fuse();
                self.0 = Some(new_stream);
                if let Some(ref task) = self.1 {
                    task.notify();
                }
            }
            None => unreachable!("ShedulerRequestStream current stream is None"),
        }
    }

    fn as_ref(&self) -> &Fuse<InternalRequestStream> {
        self.0
            .as_ref()
            .expect("ShedulerRequestStream as ref failed")
    }

    fn as_mut(&mut self) -> &mut Fuse<InternalRequestStream> {
        self.0
            .as_mut()
            .expect("ShedulerRequestStream as mut failed")
    }

    fn is_done(&self) -> bool {
        self.as_ref().is_done()
    }
}

impl Stream for ShedulerRequestStream {
    type Item = Request;
    type Error = !;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let task = current();
        self.1 = Some(task);
        self.as_mut().poll()
    }
}

impl From<InternalRequestStream> for ShedulerRequestStream {
    fn from(stream: InternalRequestStream) -> Self {
        ShedulerRequestStream(Some(stream.fuse()), None)
    }
}

pub struct GlobalLimitedSheduler<'a> {
    stream: ShedulerRequestStream,
    client: &'a Client,
    limit: u64,
    executing: FuturesUnordered<Box<Future<Item = Response, Error = Error>>>,
    logger: Option<Logger>,
}

#[allow(dead_code)]
impl<'a> GlobalLimitedSheduler<'a> {
    pub fn new(client: &'a Client, limit: u64) -> Self {
        let executing = FuturesUnordered::new();
        let stream = (Box::new(empty()) as InternalRequestStream).into();
        let logger = None;
        Self {
            client,
            stream,
            limit,
            executing,
            logger,
        }
    }

    pub fn with_logger(client: &'a Client, limit: u64, logger: Logger) -> Self {
        let executing = FuturesUnordered::new();
        let stream = (Box::new(empty()) as InternalRequestStream).into();
        let logger = Some(logger);
        Self {
            client,
            stream,
            limit,
            executing,
            logger,
        }
    }
}

impl<'a> Sheduler for GlobalLimitedSheduler<'a> {
    fn shedule(&mut self, requests: InternalRequestStream) {
        ShedulerRequestStream::chain(&mut self.stream, requests);
    }

    fn is_done(&self) -> bool {
        self.stream.is_done() && self.executing.is_empty()
    }
}

impl<'a> Stream for GlobalLimitedSheduler<'a> {
    type Item = Response;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            // nothing fancy here, just copy paste from BufferUnordered
            while self.executing.len() < self.limit as usize {
                let req = match self.stream.poll()? {
                    Async::Ready(Some(s)) => s,
                    Async::Ready(None) | Async::NotReady => break,
                };
                let fut = self.client.execute(req.into()).map_err(|e| e.into());
                self.executing.push(Box::new(fut));
            }

            match self.executing.poll() {
                Err(e) => {
                    if let Some(ref logger) = self.logger {
                        error!(logger, "request failed"; "error" => %e);
                    }
                }
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(Some(resp))) => return Ok(Async::Ready(Some(resp))),
                Ok(Async::Ready(None)) => {
                    if self.stream.as_ref().is_done() {
                        return Ok(Async::Ready(None));
                    } else {
                        return Ok(Async::NotReady);
                    }
                }
            }
        }
    }
}

fn filter_request(req: &Request) -> bool {
    req.url().has_host()
}
