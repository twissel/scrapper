use futures::{Async, Poll, Stream};
use slog::Logger;
use std::mem::replace;

pub(crate) struct EosOnError<S> {
    state: State<S>,
    logger: Option<Logger>,
}

enum State<S> {
    Working(S),
    Terminated,
    Temp,
}

impl<S> EosOnError<S> {
    pub fn new(stream: S, logger: &Option<Logger>) -> Self {
        let state = State::Working(stream);
        let logger = match logger {
            &Some(ref logger) => Some(logger.clone()),
            &None => None,
        };
        Self { state, logger }
    }
}

impl<S> Stream for EosOnError<S>
where
    S: Stream,
    S::Error: ::fmt::Display,
{
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
                            if let Some(ref logger) = self.logger {
                                error!(logger, "error received"; "error" => %e);
                            }
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
    fn eos_on_error(self, logger: &Option<Logger>) -> EosOnError<Self>;
}

impl<S> EosOnErrorExt for S
where
    S: Stream + Sized,
{
    fn eos_on_error(self, logger: &Option<Logger>) -> EosOnError<Self> {
        EosOnError::new(self, logger)
    }
}
