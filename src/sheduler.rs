use futures::stream::{empty, Fuse, FuturesUnordered, Stream};
use futures::{Async, Future, Poll};
use spider::RequestStream;
use std::convert::From;
use reqwest::unstable::async::{Client, Request, Response};
use std::collections::VecDeque;
use select_all::SelectAll;
use fork::{fork, Fork, Prong, Route};
use futures::task::{current, Task};

pub trait Sheduler: Stream<Error = (), Item = Response> {
    fn shedule(&mut self, requests: RequestStream);
    fn is_done(&self) -> bool;
}

struct ShedulerRequestStream(Option<Fuse<RequestStream>>, Option<Task>);

impl ShedulerRequestStream {
    pub fn chain(&mut self, requests: RequestStream) {
        println!("new requests to chain");
        let current_stream = self.0.take();
        match current_stream {
            Some(current_stream) => {
                let inner_stream = current_stream.into_inner();
                let requests = requests.filter(filter_request);
                let new_stream = (Box::new(inner_stream.chain(requests)) as RequestStream).fuse();
                self.0 = Some(new_stream);
                if let Some(ref task) = self.1 {
                    task.notify();
                }
            }
            None => unreachable!("ShedulerRequestStream current stream is None"),
        }
    }

    fn as_ref(&self) -> &Fuse<RequestStream> {
        self.0
            .as_ref()
            .expect("ShedulerRequestStream as ref failed")
    }

    fn as_mut(&mut self) -> &mut Fuse<RequestStream> {
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
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let task = current();
        self.1 = Some(task);
        self.as_mut().poll()
    }
}

impl From<RequestStream> for ShedulerRequestStream {
    fn from(stream: RequestStream) -> Self {
        ShedulerRequestStream(Some(stream.fuse()), None)
    }
}

#[allow(dead_code)]
pub struct UnlimitedSheduler<'a> {
    stream: ShedulerRequestStream,
    client: &'a Client,
    executing: FuturesUnordered<Box<Future<Item = Response, Error = ()>>>,
}

#[derive(Default)]
struct HostQueque {
    pending: VecDeque<Request>,
    num_executing: usize,
}

impl HostQueque {
    fn new() -> Self {
        HostQueque {
            pending: VecDeque::new(),
            num_executing: 0,
        }
    }

    fn push(&mut self, request: Request) {
        self.pending.push_back(request);
    }

    fn pop(&mut self) -> Option<Request> {
        self.pending.pop_front()
    }

    fn get_num_executing(&self) -> usize {
        self.num_executing
    }

    fn get_num_executing_mut(&mut self) -> &mut usize {
        &mut self.num_executing
    }
}

type HostRoute = Route<String>;
type HostRouter = fn(&Request) -> HostRoute;
type HostProng = Prong<ShedulerRequestStream, HostRouter, String>;

fn get_host_from_request(request: &Request) -> HostRoute {
    let host = request
        .url()
        .host_str()
        .expect("request has no host")
        .to_owned();
    Route(host)
}

#[allow(dead_code)]
pub struct HostLimitedSheduler<'a> {
    stream: Fork<ShedulerRequestStream, HostRouter, String>,
    client: &'a Client,
    limit: usize,
    output: SelectAll<Box<Stream<Item = Response, Error = ()> + 'a>>,
}

#[allow(dead_code)]
impl<'a> HostLimitedSheduler<'a> {
    pub fn new(client: &'a Client, limit: usize) -> Self {
        let stream = Box::new(empty()) as RequestStream;
        let stream = fork(stream.into(), get_host_from_request as HostRouter);
        let output = SelectAll::new();
        Self {
            client,
            stream,
            limit,
            output,
        }
    }
}

impl<'a> Sheduler for HostLimitedSheduler<'a> {
    fn shedule(&mut self, requests: RequestStream) {
        let mut input = self.stream.inner_mut();
        ShedulerRequestStream::chain(&mut input, requests);
    }

    fn is_done(&self) -> bool {
        let input = self.stream.inner();
        input.is_done() && self.output.is_empty()
    }
}

impl<'a> Stream for HostLimitedSheduler<'a> {
    type Item = Response;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        while let Async::Ready(Some(prong)) = self.stream.poll()? {
            let client = self.client;
            let prong = prong
                .map(move |req| client.execute(req).map_err(|_| ()))
                .buffer_unordered(self.limit);
            let prong = Box::new(prong) as Box<Stream<Item = Response, Error = ()> + 'a>;
            self.output.push(prong);
        }

        if let Some(val) = try_ready!(self.output.poll()) {
            return Ok(Async::Ready(Some(val)));
        }

        if self.is_done() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

#[allow(dead_code)]
impl<'a> UnlimitedSheduler<'a> {
    pub fn new(client: &'a Client) -> Self {
        let executing = FuturesUnordered::new();
        let stream = (Box::new(empty()) as RequestStream).into();
        Self {
            client,
            stream,
            executing,
        }
    }
}

impl<'a> Stream for UnlimitedSheduler<'a> {
    type Item = Response;
    type Error = ();
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        while let Async::Ready(Some(req)) = self.stream.as_mut().poll()? {
            let fut = self.client.execute(req).map_err(|_| ());
            self.executing.push(Box::new(fut));
        }

        if let Some(resp) = try_ready!(self.executing.poll()) {
            return Ok(Async::Ready(Some(resp)));
        }

        if self.stream.is_done() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct GlobalLimitedSheduler<'a> {
    stream: ShedulerRequestStream,
    client: &'a Client,
    limit: u64,
    executing: FuturesUnordered<Box<Future<Item = Response, Error = ()>>>,
}

impl<'a> GlobalLimitedSheduler<'a> {
    pub fn new(client: &'a Client, limit: u64) -> Self {
        let executing = FuturesUnordered::new();
        let stream = (Box::new(empty()) as RequestStream).into();

        Self {
            client,
            stream,
            limit,
            executing,
        }
    }
}

impl<'a> Sheduler for GlobalLimitedSheduler<'a> {
    fn shedule(&mut self, requests: RequestStream) {
        ShedulerRequestStream::chain(&mut self.stream, requests);
    }

    fn is_done(&self) -> bool {
        println!(
            "Global sheduler stream is done ? - {:?}",
            self.stream.is_done()
        );

        println!(
            "Global sheduler executing  is empty ? - {:?}",
            self.executing.is_empty()
        );

        self.stream.is_done() && self.executing.is_empty()
    }
}

impl<'a> Stream for GlobalLimitedSheduler<'a> {
    type Item = Response;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // nothing fancy here, just copy paste from BufferUnordered
        while self.executing.len() < self.limit as usize {
            let req = match self.stream.poll()? {
                Async::Ready(Some(s)) => s,
                Async::Ready(None) | Async::NotReady => break,
            };
            let fut = self.client.execute(req).map_err(|_| ());
            self.executing.push(Box::new(fut));
        }

        if let Some(resp) = try_ready!(self.executing.poll()) {
            return Ok(Async::Ready(Some(resp)));
        }

        if self.stream.as_ref().is_done() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

fn filter_request(req: &Request) -> bool {
    req.url().has_host()
}

fn execute(client: &Client, req: Request) -> Box<Future<Item = Response, Error = ()>> {
    Box::new(client.execute(req).map_err(|_| ()))
}
