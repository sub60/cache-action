use core::fmt;
use core::num::{NonZeroU16, NonZeroU32};
use core::pin::pin;
use std::collections::VecDeque;
use std::io;
use std::sync::LazyLock;

use bytes::{Bytes, BytesMut};
use futures::AsyncRead;
use nix_types::sub60::nar_multiparts::{
    CompleteRequestBody,
    ETag,
    NarMultipartsResponseBody,
    PartsRequestBody,
    PartsResponseBody,
    UploadId,
};
use nix_types::sub60::{CacheName, UserName};
use nix_types::{NarInfo, NarInfoFileName, StoreBasename, StorePath};
use reqwest::{Method, StatusCode, header};
use smallvec::SmallVec;

use crate::async_read_ext::AsyncReadExt;
use crate::context;
use crate::protocol::StoreDir;

pub(crate) type AuthToken = String;

static SUB60_CACHE_URL: LazyLock<url::Url> =
    LazyLock::new(|| "https://cache.sub60.dev".parse().expect("valid URL"));

#[derive(Clone)]
pub(crate) struct Sub60Cache {
    cache_url: url::Url,
    client: reqwest::Client,
}

#[derive(Debug)]
pub(crate) enum CacheConnectError {
    BuildHttpClient(reqwest::Error),
}

#[derive(Debug)]
pub(crate) enum CacheRequestError {
    ParseEtag(<ETag as core::str::FromStr>::Err),
    InvalidResponseHeader {
        method: Method,
        url: url::Url,
        header: header::HeaderName,
        source: header::ToStrError,
    },
    MissingResponseHeader {
        method: Method,
        url: url::Url,
        header: header::HeaderName,
    },
    ParseNarMultipartsResponseBody(
        <NarMultipartsResponseBody as core::str::FromStr>::Err,
    ),
    ParsePartsResponseBody(<PartsResponseBody as core::str::FromStr>::Err),
    ReadNar(io::Error),
    Request(reqwest::Error),
    TooManyNarParts,
    UnexpectedResponse {
        method: Method,
        status: StatusCode,
        url: url::Url,
    },
}

pub(crate) struct NarUploadState {
    part_size: NonZeroU32,
    part_urls: VecDeque<url::Url>,
    store_basename: StoreBasename,
    upload_id: UploadId,
}

impl Sub60Cache {
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

    async fn complete_nar_upload(
        &self,
        upload_id: &UploadId,
        etags: SmallVec<[ETag; 8]>,
        store_basename: StoreBasename,
    ) -> Result<(), CacheRequestError> {
        let url = self.nar_multiparts_complete_url(upload_id);

        let response = self
            .client
            .post(url.clone())
            .body(CompleteRequestBody { etags, store_basename }.to_string())
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(CacheRequestError::UnexpectedResponse {
                method: Method::POST,
                status: response.status(),
                url,
            })
        }
    }

    fn narinfo_url(&self, narinfo_filename: &NarInfoFileName) -> url::Url {
        let mut url = self.cache_url.clone();
        url.path_segments_mut()
            .expect("cache URL can be a base")
            .push(&narinfo_filename.to_string());
        url
    }

    fn narinfo_upload_url(
        &self,
        narinfo_filename: &NarInfoFileName,
        upload_id: &UploadId,
    ) -> url::Url {
        let mut url = self.narinfo_url(narinfo_filename);
        url.query_pairs_mut().append_pair("upload_id", upload_id);
        url
    }

    fn nar_multiparts_url(&self) -> url::Url {
        let mut url = self.cache_url.clone();
        url.path_segments_mut()
            .expect("cache URL can be a base")
            .push("nar-multiparts");
        url
    }

    fn nar_multiparts_parts_url(&self, upload_id: &UploadId) -> url::Url {
        let mut url = self.nar_multiparts_url();
        url.path_segments_mut()
            .expect("URL can be a base")
            .push(upload_id)
            .push("parts");
        url
    }

    fn nar_multiparts_complete_url(&self, upload_id: &UploadId) -> url::Url {
        let mut url = self.nar_multiparts_url();
        url.path_segments_mut()
            .expect("URL can be a base")
            .push(upload_id)
            .push("complete");
        url
    }

    async fn request_part_urls(
        &self,
        upload_id: &UploadId,
        part_numbers: SmallVec<[NonZeroU16; 8]>,
        store_basename: StoreBasename,
    ) -> Result<SmallVec<[url::Url; 8]>, CacheRequestError> {
        let url = self.nar_multiparts_parts_url(upload_id);

        let response = self
            .client
            .post(url.clone())
            .body(PartsRequestBody { part_numbers, store_basename }.to_string())
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        if !response.status().is_success() {
            return Err(CacheRequestError::UnexpectedResponse {
                method: Method::POST,
                status: response.status(),
                url,
            });
        }

        Ok(response
            .text()
            .await
            .map_err(CacheRequestError::Request)?
            .parse::<PartsResponseBody>()
            .map_err(CacheRequestError::ParsePartsResponseBody)?
            .urls)
    }

    async fn upload_part(
        &self,
        url: url::Url,
        part_bytes: Bytes,
    ) -> Result<ETag, CacheRequestError> {
        let response = self
            .client
            .put(url.clone())
            .body(part_bytes)
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        if !response.status().is_success() {
            return Err(CacheRequestError::UnexpectedResponse {
                method: Method::PUT,
                status: response.status(),
                url,
            });
        }

        let etag = response.headers().get(header::ETAG).ok_or_else(|| {
            CacheRequestError::MissingResponseHeader {
                method: Method::PUT,
                url: url.clone(),
                header: header::ETAG,
            }
        })?;

        etag.to_str()
            .map_err(|source| CacheRequestError::InvalidResponseHeader {
                method: Method::PUT,
                url: url.clone(),
                header: header::ETAG,
                source,
            })?
            .parse()
            .map_err(CacheRequestError::ParseEtag)
    }
}

impl context::Cache for Sub60Cache {
    type NarUploadState = NarUploadState;
    type Error = CacheRequestError;

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

    async fn initiate_nar_upload(
        &self,
        store_path: &StorePath<StoreDir>,
    ) -> Result<Self::NarUploadState, Self::Error> {
        let url = self.nar_multiparts_url();

        let response = self
            .client
            .post(url.clone())
            .body(store_path.basename().to_string())
            .send()
            .await
            .map_err(CacheRequestError::Request)?;

        if !response.status().is_success() {
            return Err(CacheRequestError::UnexpectedResponse {
                method: Method::POST,
                status: response.status(),
                url,
            });
        }

        let body = response
            .text()
            .await
            .map_err(CacheRequestError::Request)?
            .parse::<NarMultipartsResponseBody>()
            .map_err(CacheRequestError::ParseNarMultipartsResponseBody)?;

        Ok(NarUploadState {
            part_size: body.part_size,
            part_urls: body.parts_urls.into_iter().collect(),
            store_basename: store_path.basename().clone(),
            upload_id: body.upload_id,
        })
    }

    async fn upload_nar(
        &self,
        state: &mut Self::NarUploadState,
        nar_bytes: impl AsyncRead + Send + 'static,
    ) -> Result<(), Self::Error> {
        let part_size = state.part_size.get() as usize;

        let mut nar_bytes = pin!(nar_bytes);
        let mut part_number = NonZeroU16::MIN;
        let mut etags = SmallVec::new();
        let mut part_bytes_buf = BytesMut::zeroed(part_size);

        loop {
            let Some(part_url) = state.part_urls.pop_front() else {
                let new_part_numbers =
                    (part_number..part_number.saturating_add(4)).collect();
                if new_part_numbers.is_empty() {
                    return Err(CacheRequestError::TooManyNarParts);
                }
                let new_urls = self
                    .request_part_urls(
                        &state.upload_id,
                        new_part_numbers,
                        state.store_basename.clone(),
                    )
                    .await?;
                state.part_urls.extend(new_urls);
                continue;
            };

            let num_read = nar_bytes
                .as_mut()
                .try_fill(&mut part_bytes_buf[..])
                .await
                .map_err(CacheRequestError::ReadNar)?;

            if num_read == 0 {
                break;
            }

            part_bytes_buf.truncate(num_read);

            let part_bytes = part_bytes_buf.freeze();

            let etag = self.upload_part(part_url, part_bytes.clone()).await?;

            etags.push(etag);

            if num_read < part_size {
                break;
            }

            part_bytes_buf = part_bytes
                .try_into_mut()
                .unwrap_or_else(|_| BytesMut::zeroed(part_size));

            part_number = part_number
                .checked_add(1)
                .ok_or(CacheRequestError::TooManyNarParts)?;
        }

        self.complete_nar_upload(
            &state.upload_id,
            etags,
            state.store_basename.clone(),
        )
        .await
    }

    async fn upload_narinfo(
        &self,
        narinfo_filename: NarInfoFileName,
        narinfo: NarInfo<(), StoreDir>,
        nar_upload_state: Self::NarUploadState,
    ) -> Result<u64, Self::Error> {
        let narinfo = narinfo.with_url("").to_string();

        let narinfo_size = narinfo.len() as u64;

        let url = self
            .narinfo_upload_url(&narinfo_filename, &nar_upload_state.upload_id);

        let response = self
            .client
            .put(url.clone())
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
                url,
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
            Self::ParseEtag(err) => write!(f, "couldn't parse ETag: {err}"),
            Self::InvalidResponseHeader { method, url, header, source } => {
                write!(
                    f,
                    "invalid {header} header from {method} {url}: {source}"
                )
            },
            Self::MissingResponseHeader { method, url, header } => {
                write!(f, "missing {header} header from {method} {url}")
            },
            Self::ParseNarMultipartsResponseBody(err) => {
                write!(f, "couldn't parse NAR multipart response body: {err}")
            },
            Self::ParsePartsResponseBody(err) => {
                write!(f, "couldn't parse multipart parts response body: {err}")
            },
            Self::ReadNar(err) => write!(f, "couldn't read NAR bytes: {err}"),
            Self::Request(err) => write!(f, "HTTP request failed: {err}"),
            Self::TooManyNarParts => {
                write!(f, "NAR upload has too many multipart parts")
            },
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
            Self::ParseEtag(err) => Some(err),
            Self::InvalidResponseHeader { source, .. } => Some(source),
            Self::MissingResponseHeader { .. } => None,
            Self::ParseNarMultipartsResponseBody(err) => Some(err),
            Self::ParsePartsResponseBody(err) => Some(err),
            Self::ReadNar(err) => Some(err),
            Self::Request(err) => Some(err),
            Self::TooManyNarParts | Self::UnexpectedResponse { .. } => None,
        }
    }
}
