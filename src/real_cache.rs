use core::convert::Infallible;
use core::fmt;

use bytes::Bytes;
use nix_types::{CacheName, NarFileName, NarInfo, NarInfoFileName, UserName};

use crate::protocol::StoreDir;
use crate::{AuthToken, context};

#[derive(Clone)]
pub(crate) struct RealCache {
    _owner: UserName,
    _name: CacheName,
}

#[derive(Debug)]
pub(crate) enum CacheConnectError {}

impl RealCache {
    /// TODO: docs.
    pub(crate) async fn connect(
        _owner: UserName,
        _name: CacheName,
        _auth: AuthToken,
    ) -> Result<Self, CacheConnectError> {
        Ok(Self { _owner, _name })
    }
}

impl context::Cache for RealCache {
    type Error = Infallible;

    async fn has_nar(
        &self,
        _nar_filename: NarFileName,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn has_narinfo(
        &self,
        _narinfo_filename: NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn write_nar(&self, _nar_bytes: Bytes) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn write_narinfo(
        &self,
        narinfo: NarInfo<impl fmt::Display, StoreDir>,
    ) -> Result<u64, Self::Error> {
        Ok(narinfo.to_string().len() as u64)
    }
}

impl fmt::Display for CacheConnectError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl core::error::Error for CacheConnectError {}
