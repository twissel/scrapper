use futures::{Async, Future, Poll, Stream};
use futures_cpupool::CpuPool;
use request::Request;
use sha1::{Digest, Sha1};
use slog::Logger;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt;
use std::iter::FromIterator;
use std::sync::{Arc, Mutex, TryLockError};
use url::Url;

pub(crate) fn filter_and_log_errors<S, T, E, SE>(
    stream: S,
    logger: &Option<Logger>,
) -> Box<Stream<Item = T, Error = SE>>
where
    S: Stream<Item = Result<T, E>, Error = SE> + 'static,
    E: ::fmt::Display,
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

pub(crate) struct CanonicalUrlView<'a> {
    scheme: &'a str,
    host_str: Option<&'a str>,
    path: &'a str,
    query_pairs: BTreeMap<Cow<'a, str>, Cow<'a, str>>,
}

impl<'a> From<&'a Url> for CanonicalUrlView<'a> {
    fn from(url: &'a Url) -> Self {
        let scheme = url.scheme();
        let path = url.path();
        let host_str = url.host_str();
        let query_pairs = BTreeMap::from_iter(url.query_pairs());
        CanonicalUrlView {
            scheme,
            host_str,
            path,
            query_pairs,
        }
    }
}

impl<'a> fmt::Display for CanonicalUrlView<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}://", self.scheme)?;
        write!(f, "{}", self.host_str.expect("url has no host"))?;
        write!(f, "{}", self.path)?;
        if self.query_pairs.len() > 0 {
            write!(f, "?")?;
            let mut iter = self.query_pairs.iter().peekable();
            loop {
                let pair = iter.next();
                match pair {
                    Some((key, val)) => {
                        if let Some(_) = iter.peek() {
                            write!(f, "{}={}&", key, val)?;
                        } else {
                            write!(f, "{}={}", key, val)?;
                        }
                    }
                    None => break,
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn canonicalize_url(url: &Url) -> CanonicalUrlView {
    url.into()
}

pub(crate) struct RFPFilter {
    seen: Arc<Mutex<HashSet<RequestDigest>>>,
    pool: CpuPool,
    logger: Option<Logger>,
}

impl RFPFilter {
    pub fn new(pool: CpuPool, logger: Option<Logger>) -> Self {
        let seen = Arc::new(Mutex::new(HashSet::new()));
        RFPFilter { seen, pool, logger }
    }
}

pub(crate) struct UniqueFuture {
    seen: Arc<Mutex<HashSet<RequestDigest>>>,
    digest: Option<RequestDigest>,
    request: Option<Request>,
}

impl Future for UniqueFuture {
    type Item = (bool, Request);
    type Error = !;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.seen.try_lock() {
            Err(TryLockError::WouldBlock) => {
                // Loop again
            }
            Err(TryLockError::Poisoned(_poisoned)) => {
                // Currently we just panic  if mutex get poisoned.
                panic!("Other unique future seemd to panic during processing.");
            }
            Ok(mut seen) => {
                let digest = self.digest
                    .take()
                    .expect("unique future poll called after ready");
                let contains = seen.contains(&digest);
                if !contains {
                    seen.insert(digest);
                }
                let request = self.request
                    .take()
                    .expect("unique future poll called after ready");
                return Ok(Async::Ready((contains, request)));
            }
        }
        self.poll()
    }
}

impl UniqueFuture {
    fn new(
        seen: Arc<Mutex<HashSet<RequestDigest>>>,
        digest: RequestDigest,
        request: Request,
    ) -> Self {
        let digest = Some(digest);
        let request = Some(request);
        UniqueFuture {
            seen,
            digest,
            request,
        }
    }
}

impl RFPFilter {
    pub fn unique<S: Stream<Item = (RequestDigest, Request), Error = !>>(
        &self,
        stream: S,
    ) -> impl Stream<Item = Request, Error = !> {
        let seen = self.seen.clone();
        let pool = self.pool.clone();
        let logger = self.logger.clone();
        let stream = stream
            .and_then(move |(digest, request)| {
                let fut_seen = seen.clone();
                let fut = UniqueFuture::new(fut_seen, digest, request);
                pool.spawn(fut)
            })
            .filter_map(move |(contains, request)| {
                if !contains {
                    Some(request)
                } else {
                    if let Some(ref log) = logger {
                        info!(log, "request filtered"; "request" => %request);
                    }
                    None
                }
            });
        stream
    }
}

#[derive(PartialEq, Eq, Hash)]
pub(crate) struct RequestDigest(Digest);

pub(crate) fn calculate_digest(r: &Request) -> RequestDigest {
    let mut sha = Sha1::new();
    let canonical = canonicalize_url(r.url());
    sha.update(canonical.scheme.as_bytes());
    sha.update(canonical.path.as_bytes());
    for (key, val) in canonical.query_pairs {
        sha.update(key.as_bytes());
        sha.update(val.as_bytes());
    }

    sha.update(r.method().as_ref().as_bytes());
    if let Some(body) = r.body() {
        sha.update(body.as_ref())
    }

    RequestDigest(sha.digest())
}

pub(crate) fn get_digest_and_request(req: Request) -> (RequestDigest, Request) {
    let digest = calculate_digest(&req);
    (digest, req)
}
