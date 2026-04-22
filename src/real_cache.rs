use core::fmt;
use std::sync::LazyLock;

use async_compat::Compat;
use futures::AsyncRead;
use nix_types::{CacheName, NarFileName, NarInfo, NarInfoFileName, UserName};
use reqwest::{Method, StatusCode};
use tokio_util::io::ReaderStream;

use crate::protocol::StoreDir;
use crate::{AuthToken, context};

static SUB60_CACHE_URL: LazyLock<url::Url> =
    LazyLock::new(|| "https://cache.sub60.dev".parse().expect("valid URL"));

#[derive(Clone)]
pub(crate) struct RealCache {
    cache_url: url::Url,
    client: reqwest::Client,
}

#[derive(Debug)]
pub(crate) enum CacheConnectError {
    BuildHttpClient(reqwest::Error),
}

#[derive(Debug)]
pub(crate) enum CacheRequestError {
    Request(reqwest::Error),
    UnexpectedResponse { method: Method, status: StatusCode, url: url::Url },
}

impl RealCache {
    /// TODO: docs.
    pub(crate) async fn connect(
        owner: UserName,
        name: CacheName,
        _auth: AuthToken,
    ) -> Result<Self, CacheConnectError> {
        let mut cache_url = SUB60_CACHE_URL.clone();
        cache_url
            .path_segments_mut()
            .expect("cache URL can be a base")
            .push(&owner)
            .push(&name);

        let client = reqwest::Client::builder()
            .build()
            .map_err(CacheConnectError::BuildHttpClient)?;

        Ok(Self { cache_url, client })
    }

    fn nar_url(&self, nar_filename: &NarFileName) -> url::Url {
        let mut url = self.cache_url.clone();
        url.path_segments_mut()
            .expect("cache URL can be a base")
            .push("nar")
            .push(&nar_filename.to_string());
        url
    }

    fn narinfo_url(&self, narinfo_filename: &NarInfoFileName) -> url::Url {
        let mut url = self.cache_url.clone();
        url.path_segments_mut()
            .expect("cache URL can be a base")
            .push(&narinfo_filename.to_string());
        url
    }
}

impl context::Cache for RealCache {
    type Error = CacheRequestError;

    async fn has_nar(
        &self,
        nar_filename: &NarFileName,
    ) -> Result<bool, Self::Error> {
        let response = self
            .client
            .head(self.nar_url(nar_filename))
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        match response.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(CacheRequestError::UnexpectedResponse {
                method: Method::HEAD,
                status,
                url: self.nar_url(nar_filename),
            }),
        }
    }

    async fn has_narinfo(
        &self,
        narinfo_filename: &NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        let response = self
            .client
            .head(self.narinfo_url(narinfo_filename))
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        match response.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(CacheRequestError::UnexpectedResponse {
                method: Method::HEAD,
                status,
                url: self.narinfo_url(narinfo_filename),
            }),
        }
    }

    async fn write_nar(
        &self,
        nar_filename: NarFileName,
        nar_bytes: impl AsyncRead + Send + 'static,
    ) -> Result<(), Self::Error> {
        let body = reqwest::Body::wrap_stream(ReaderStream::with_capacity(
            Compat::new(nar_bytes),
            64 * 1024,
        ));

        let response = self
            .client
            .put(self.nar_url(&nar_filename))
            .body(body)
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(CacheRequestError::UnexpectedResponse {
                method: Method::PUT,
                status: response.status(),
                url: self.nar_url(&nar_filename),
            })
        }
    }

    async fn write_narinfo(
        &self,
        narinfo_filename: NarInfoFileName,
        narinfo: NarInfo<impl fmt::Display, StoreDir>,
    ) -> Result<u64, Self::Error> {
        let narinfo = narinfo.to_string();
        let narinfo_size = narinfo.len() as u64;
        let response = self
            .client
            .put(self.narinfo_url(&narinfo_filename))
            .body(narinfo)
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        if response.status().is_success() {
            Ok(narinfo_size)
        } else {
            Err(CacheRequestError::UnexpectedResponse {
                method: Method::PUT,
                status: response.status(),
                url: self.narinfo_url(&narinfo_filename),
            })
        }
    }
}

impl fmt::Display for CacheConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildHttpClient(err) => {
                write!(f, "couldn't build HTTP client: {err}")
            },
        }
    }
}

impl fmt::Display for CacheRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(err) => write!(f, "HTTP request failed: {err}"),
            Self::UnexpectedResponse { method, status, url } => {
                write!(f, "unexpected response from {method} {url}: {status}")
            },
        }
    }
}

impl core::error::Error for CacheConnectError {}

impl core::error::Error for CacheRequestError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Request(err) => Some(err),
            Self::UnexpectedResponse { .. } => None,
        }
    }
}
