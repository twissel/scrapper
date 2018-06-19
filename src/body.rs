use bytes::Bytes;
use reqwest::unstable::async;
use std::convert::From;
use std::hash::{Hash, Hasher};

pub struct Body {
    bytes: Bytes,
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(v: Vec<u8>) -> Body {
        v.into()
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        s.into_bytes().into()
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body {
            bytes: Bytes::from_static(s),
        }
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        s.as_bytes().into()
    }
}

impl From<Body> for async::Body {
    fn from(b: Body) -> async::Body {
        b.bytes.into()
    }
}

impl Hash for Body {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl Body {
    #[inline]
    pub fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}
