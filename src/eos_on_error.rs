use futures::{Async, Poll, Stream};
use std::mem::replace;

pub(crate) struct EosOnError<S> {
    state: State<S>,
}

enum State<S> {
    Working(S),
    Terminated,
    Temp,
}

impl<S> EosOnError<S> {
    pub fn new(stream: S) -> Self {
        let state = State::Working(stream);
        Self { state }
    }
}

impl<S: Stream> Stream for EosOnError<S> {
    type Item = S::Item;
    type Error = !;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let state = replace(&mut self.state, State::Temp);
            match state {
                State::Terminated => {
                    self.state = State::Terminated;
                    return Ok(Async::Ready(None));
                }
                State::Working(mut stream) => {
                    let poll = stream.poll();
                    match poll {
                        Err(e) => {
                            self.state = State::Terminated;
                        }
                        Ok(Async::NotReady) => {
                            self.state = State::Working(stream);
                            return Ok(Async::NotReady);
                        }
                        Ok(Async::Ready(None)) => self.state = State::Terminated,
                        Ok(Async::Ready(Some(msg))) => {
                            self.state = State::Working(stream);
                            return Ok(Async::Ready(Some(msg)));
                        }
                    }
                }
                State::Temp => unreachable!("NoneError State::Temp reached"),
            }
        }
    }
}

pub(crate) trait EosOnErrorExt: Stream + Sized {
    fn eos_on_error(self) -> EosOnError<Self>;
}

impl<S> EosOnErrorExt for S
where
    S: Stream + Sized,
{
    fn eos_on_error(self) -> EosOnError<Self> {
        EosOnError::new(self)
    }
}
