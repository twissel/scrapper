use body::Body;
use reqwest::header::Headers;
use reqwest::unstable::async;
use reqwest::Method;
use std::convert::From;
use url::Url;

// wrapper around reqwest::Request
pub struct Request {
    inner: async::Request,
    body: Option<Body>,
}

impl ::fmt::Display for Request {
    fn fmt(&self, f: &mut ::fmt::Formatter) -> ::fmt::Result {
        write!(f, "Request(Url({}))", self.url())
    }
}

impl Request {
    /// Constructs a new request.
    #[inline]
    pub fn new(method: Method, url: Url) -> Self {
        let inner = async::Request::new(method, url);
        let body = None;
        Request { inner, body }
    }

    /// Get the method.
    #[inline]
    pub fn method(&self) -> &Method {
        self.inner.method()
    }

    /// Get a mutable reference to the method.
    #[inline]
    pub fn method_mut(&mut self) -> &mut Method {
        self.inner.method_mut()
    }

    /// Get the url.
    #[inline]
    pub fn url(&self) -> &Url {
        self.inner.url()
    }

    /// Get a mutable reference to the url.
    #[inline]
    pub fn url_mut(&mut self) -> &mut Url {
        self.inner.url_mut()
    }

    /// Get the headers.
    #[inline]
    pub fn headers(&self) -> &Headers {
        self.inner.headers()
    }

    /// Get a mutable reference to the headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut Headers {
        self.inner.headers_mut()
    }

    /// Get the body.
    #[inline]
    pub fn body(&self) -> Option<&Body> {
        self.body.as_ref()
    }

    /// Get a mutable reference to the body.
    #[inline]
    pub fn body_mut(&mut self) -> &mut Option<Body> {
        &mut self.body
    }
}

impl From<Request> for async::Request {
    #[inline]
    fn from(r: Request) -> async::Request {
        let mut request = r.inner;
        if let Some(body) = r.body {
            *request.body_mut() = Some(body.into())
        }
        request
    }
}
