use futures::AsyncRead;
use nix_types::{NarInfo, NarInfoFileName, StorePath};

use crate::context::Cache;
use crate::noop_cache::NoopCache;
use crate::protocol::StoreDir;
use crate::run::RunArgs;
use crate::sub60_cache::{Sub60Cache, Sub60CacheConnectError};

#[derive(Clone)]
pub(crate) enum AnyCache {
    Noop(NoopCache),
    Sub60(Sub60Cache),
}

#[derive(Debug, Copy, Clone, clap::ValueEnum)]
pub(crate) enum AnyCacheKind {
    Noop,
    Sub60,
}

pub(crate) enum AnyCacheNarUploadState {
    Noop(<NoopCache as Cache>::NarUploadState),
    Sub60(<Sub60Cache as Cache>::NarUploadState),
}

#[derive(Debug, derive_more::Display, cauchy::Error)]
pub(crate) enum AnyCacheError {
    #[display("no-op cache error")]
    Noop(#[source] <NoopCache as Cache>::Error),
    #[display("Sub60 cache error")]
    Sub60(#[source] <Sub60Cache as Cache>::Error),
}

impl AnyCache {
    pub(crate) async fn new(
        args: &RunArgs,
    ) -> Result<Self, Sub60CacheConnectError> {
        match args.cache {
            AnyCacheKind::Noop => Ok(NoopCache::default().into()),
            AnyCacheKind::Sub60 => {
                Sub60Cache::connect(args).await.map(Into::into)
            },
        }
    }
}

impl Cache for AnyCache {
    type NarUploadState = AnyCacheNarUploadState;
    type Error = AnyCacheError;

    async fn has_narinfo(
        &self,
        narinfo_filename: &NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        match self {
            Self::Noop(cache) => {
                cache.has_narinfo(narinfo_filename).await.map_err(Into::into)
            },
            Self::Sub60(cache) => {
                cache.has_narinfo(narinfo_filename).await.map_err(Into::into)
            },
        }
    }

    async fn initiate_nar_upload(
        &self,
        store_path: &StorePath<StoreDir>,
    ) -> Result<Self::NarUploadState, Self::Error> {
        match self {
            Self::Noop(cache) => cache
                .initiate_nar_upload(store_path)
                .await
                .map(AnyCacheNarUploadState::from)
                .map_err(Into::into),
            Self::Sub60(cache) => cache
                .initiate_nar_upload(store_path)
                .await
                .map(AnyCacheNarUploadState::from)
                .map_err(Into::into),
        }
    }

    async fn upload_nar(
        &self,
        state: &mut Self::NarUploadState,
        nar_bytes: impl AsyncRead + Send + 'static,
    ) -> Result<(), Self::Error> {
        match (self, state) {
            (Self::Noop(cache), AnyCacheNarUploadState::Noop(state)) => {
                cache.upload_nar(state, nar_bytes).await.map_err(Into::into)
            },
            (Self::Sub60(cache), AnyCacheNarUploadState::Sub60(state)) => {
                cache.upload_nar(state, nar_bytes).await.map_err(Into::into)
            },
            _ => unreachable!(
                "AnyCache used with another cache variant's NAR upload state"
            ),
        }
    }

    async fn upload_narinfo(
        &self,
        narinfo_filename: NarInfoFileName,
        narinfo: NarInfo<(), StoreDir>,
        state: Self::NarUploadState,
    ) -> Result<u64, Self::Error> {
        match (self, state) {
            (Self::Noop(cache), AnyCacheNarUploadState::Noop(state)) => cache
                .upload_narinfo(narinfo_filename, narinfo, state)
                .await
                .map_err(Into::into),
            (Self::Sub60(cache), AnyCacheNarUploadState::Sub60(state)) => cache
                .upload_narinfo(narinfo_filename, narinfo, state)
                .await
                .map_err(Into::into),
            _ => unreachable!(
                "AnyCache used with another cache variant's NAR upload state"
            ),
        }
    }
}

impl From<NoopCache> for AnyCache {
    fn from(cache: NoopCache) -> Self {
        Self::Noop(cache)
    }
}

impl From<Sub60Cache> for AnyCache {
    fn from(cache: Sub60Cache) -> Self {
        Self::Sub60(cache)
    }
}

impl From<<NoopCache as Cache>::NarUploadState> for AnyCacheNarUploadState {
    fn from(state: <NoopCache as Cache>::NarUploadState) -> Self {
        Self::Noop(state)
    }
}

impl From<<Sub60Cache as Cache>::NarUploadState> for AnyCacheNarUploadState {
    fn from(state: <Sub60Cache as Cache>::NarUploadState) -> Self {
        Self::Sub60(state)
    }
}

impl From<<NoopCache as Cache>::Error> for AnyCacheError {
    fn from(error: <NoopCache as Cache>::Error) -> Self {
        Self::Noop(error)
    }
}

impl From<<Sub60Cache as Cache>::Error> for AnyCacheError {
    fn from(error: <Sub60Cache as Cache>::Error) -> Self {
        Self::Sub60(error)
    }
}
