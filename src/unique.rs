use futures::{Async, Poll, Stream};
use std::hash::Hash;
use std::collections::HashSet;

pub struct Unique<S>
where
    S: Stream,
    S::Item: Hash + Eq + PartialEq + Clone,
{
    stream: S,
    seen: HashSet<S::Item>,
}

impl<S> Unique<S>
where
    S: Stream,
    S::Item: Hash + Eq + PartialEq + Clone,
{
    fn get_stream(self) -> S {
        self.stream
    }

    fn get_seen(self) -> HashSet<S::Item> {
        self.seen
    }
}

impl<S> Stream for Unique<S>
where
    S: Stream,
    S::Item: Hash + Eq + PartialEq + Clone,
{
    type Item = S::Item;
    type Error = S::Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match try_ready!(self.stream.poll()) {
                Some(e) => {
                    if !self.seen.contains(&e) {
                        self.seen.insert(e.clone());
                        return Ok(Async::Ready(Some(e)));
                    }
                }
                None => return Ok(Async::Ready(None)),
            }
        }
    }
}

pub trait UniqueStream<S>
where
    S: Stream,
    S::Item: Hash + Eq + PartialEq + Clone,
{
    fn unique(self) -> Unique<S>;
}

impl<ST> UniqueStream<ST> for ST
where
    ST: Stream,
    ST::Item: Hash + Eq + PartialEq + Clone,
{
    fn unique(self) -> Unique<ST> {
        Unique {
            stream: self,
            seen: HashSet::new(),
        }
    }
}
