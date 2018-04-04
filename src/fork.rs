use futures::{Async, Poll, Stream};
use std::collections::{HashMap, VecDeque};
use std::collections::hash_map::Entry;
use std::rc::Rc;
use std::cell::RefCell;
use std::hash::Hash;
use select_all::SelectAll;
use futures::task;
use std::fmt::Debug;
use std::cell::{Ref, RefMut};

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Route<R>(pub R);

pub struct Prong<S, F, R>
where
    S: Stream,
    Route<R>: Hash + Eq + PartialEq + Clone,
    F: FnMut(&S::Item) -> Route<R>,
{
    route: Route<R>,
    shared: Rc<RefCell<Shared<S, F, R>>>,
}

impl<S, F, R> Stream for Prong<S, F, R>
where
    S: Stream,
    S::Error: Clone,
    Route<R>: Hash + Eq + PartialEq + Clone,
    F: FnMut(&S::Item) -> Route<R>,
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        {
            let mut shared = self.shared.borrow_mut();
            let msg = shared
                .queques
                .get_queque_mut(&self.route)
                .unwrap()
                .pop_front();
            let poll = match msg {
                Some(Ok(Some(msg))) => return Ok(Async::Ready(Some(msg))),
                Some(Ok(None)) => return Ok(Async::Ready(None)),
                Some(Err(e)) => return Err(e),
                None => shared.stream.poll(),
            };

            match poll {
                Err(e) => {
                    shared.queques.push_err(e.clone());
                    shared.fork_stream.push_back(Err(e));
                    if let Some(ref task) = shared.task {
                        task.notify();
                    }
                }
                Ok(Async::Ready(Some(msg))) => {
                    let route = (&mut shared.router)(&msg);
                    match shared.queques.contains_queque(&route) {
                        true => {
                            let queque = shared.queques.get_queque_mut(&route).unwrap();
                            queque.push_back(Ok(Some(msg)));
                        }
                        false => {
                            let mut queque = ProngQueque::<S>::new();
                            queque.push_back(Ok(Some(msg)));
                            shared.queques.add_queque(route.clone(), queque);
                            let prong = Prong {
                                route: route,
                                shared: self.shared.clone(),
                            };
                            shared.fork_stream.push_back(Ok(Some(prong)));
                            if let Some(ref task) = shared.task {
                                task.notify();
                            }
                        }
                    }
                }
                Ok(Async::Ready(None)) => {
                    shared.queques.push_none();
                    shared.fork_stream.push_back(Ok(None));
                    if let Some(ref task) = shared.task {
                        task.notify();
                    }
                }
                Ok(Async::NotReady) => return Ok(Async::NotReady),
            }
        }
        self.poll()
    }
}

type ProngQueque<S: Stream> = VecDeque<Result<Option<S::Item>, S::Error>>;

struct Queques<S: Stream, R>(HashMap<Route<R>, ProngQueque<S>>);

impl<S, R> Queques<S, R>
where
    Route<R>: Hash + Eq + PartialEq,
    S: Stream,
{
    fn new() -> Self {
        Queques(HashMap::new())
    }

    fn get_queque_mut(&mut self, route: &Route<R>) -> Option<&mut ProngQueque<S>> {
        self.0.get_mut(route)
    }

    fn get_queque_entry(&mut self, route: Route<R>) -> Entry<Route<R>, ProngQueque<S>> {
        self.0.entry(route)
    }

    fn contains_queque(&self, route: &Route<R>) -> bool {
        self.0.contains_key(route)
    }

    fn push_err(&mut self, e: S::Error)
    where
        S::Error: Clone,
    {
        for queque in self.0.values_mut() {
            queque.push_back(Err(e.clone()));
        }
    }

    fn push_none(&mut self) {
        for queque in self.0.values_mut() {
            queque.push_back(Ok(None));
        }
    }

    fn add_queque(&mut self, route: Route<R>, queque: ProngQueque<S>) {
        self.0.insert(route, queque);
    }
}

struct Shared<S, F, R>
where
    S: Stream,
    Route<R>: Hash + Eq + PartialEq + Clone,
    F: FnMut(&S::Item) -> Route<R>,
{
    stream: S,
    router: F,
    queques: Queques<S, R>,
    fork_stream: VecDeque<Result<Option<Prong<S, F, R>>, S::Error>>,
    task: Option<task::Task>,
}

enum State {
    Initial,
    WithProngs,
}

pub struct Fork<S, F, R>
where
    S: Stream,
    Route<R>: Hash + Eq + PartialEq + Clone,
    F: FnMut(&S::Item) -> Route<R>,
{
    state: State,
    shared: Rc<RefCell<Shared<S, F, R>>>,
}

impl<S, F, R> Fork<S, F, R>
where
    S: Stream,
    Route<R>: Hash + Eq + PartialEq + Clone,
    F: FnMut(&S::Item) -> Route<R>,
{
    pub fn inner_mut(&self) -> RefMut<S> {
        let borrowed = self.shared.borrow_mut();
        RefMut::map(borrowed, |b| &mut b.stream)
    }

    pub fn inner(&self) -> Ref<S> {
        let borrowed = self.shared.borrow();
        Ref::map(borrowed, |b| &b.stream)
    }
}

impl<S, F, R> Stream for Fork<S, F, R>
where
    S: Stream,
    Route<R>: Hash + Eq + PartialEq + Clone,
    F: FnMut(&S::Item) -> Route<R>,
    S::Error: Clone,
{
    type Item = Prong<S, F, R>;
    type Error = <S as Stream>::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.state {
            State::Initial => {
                {
                    let mut shared = self.shared.borrow_mut();
                    let poll = try_ready!(shared.stream.poll());
                    match poll {
                        None => return Ok(Async::Ready(None)),
                        Some(msg) => {
                            let route = (&mut shared.router)(&msg);
                            let mut queque = ProngQueque::<S>::new();
                            queque.push_back(Ok(Some(msg)));
                            shared.queques.add_queque(route.clone(), queque);
                            let prong = Prong {
                                shared: self.shared.clone(),
                                route: route,
                            };
                            shared.fork_stream.push_back(Ok(Some(prong)));
                            shared.task = Some(task::current());
                            self.state = State::WithProngs;
                        }
                    }
                }
                self.poll()
            }
            State::WithProngs => {
                let mut shared = self.shared.borrow_mut();
                let msg = shared.fork_stream.pop_front();
                match msg {
                    Some(Ok(Some(msg))) => Ok(Async::Ready(Some(msg))),
                    Some(Ok(None)) => Ok(Async::Ready(None)),
                    Some(Err(e)) => Err(e),
                    None => Ok(Async::NotReady),
                }
            }
        }
    }
}

pub fn fork<S, F, R>(stream: S, router: F) -> Fork<S, F, R>
where
    S: Stream,
    S::Error: Clone,
    F: FnMut(&S::Item) -> Route<R>,
    Route<R>: Hash + Eq + PartialEq + Clone,
    Prong<S, F, R>: Stream,
{
    let shared = Shared {
        queques: Queques::new(),
        stream: stream,
        router: router,
        fork_stream: VecDeque::new(),
        task: None,
    };

    Fork {
        state: State::Initial,
        shared: Rc::new(RefCell::new(shared)),
    }
}
