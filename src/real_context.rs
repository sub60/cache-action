use core::fmt;

use nix_types::{CacheName, UserName};

use crate::nix_cli::NixCli;
use crate::real_cache::RealCache;
use crate::tokio::TokioSpawner;
use crate::{AuthToken, context};

pub(crate) struct RealContext {
    spawner: TokioSpawner,
}

#[derive(Debug)]
pub(crate) enum CacheError {}

impl RealContext {
    pub(crate) fn new(spawner: TokioSpawner) -> Self {
        Self { spawner }
    }
}

impl context::Context for RealContext {
    type Cache = RealCache;
    type Nix = NixCli;
    type Spawner = TokioSpawner;
    type CacheError = CacheError;

    async fn cache(
        &mut self,
        _owner: UserName,
        _name: CacheName,
        _auth: AuthToken,
    ) -> Result<Self::Cache, Self::CacheError> {
        todo!()
    }

    fn nix(&self) -> &Self::Nix {
        &NixCli {}
    }

    fn spawner(&self) -> &Self::Spawner {
        &self.spawner
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl core::error::Error for CacheError {}
