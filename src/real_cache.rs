use core::convert::Infallible;
use core::fmt;

use bytes::Bytes;
use nix_types::{CacheName, NarFileName, NarInfo, NarInfoFileName, UserName};

use crate::protocol::StoreDir;
use crate::{AuthToken, context};

pub(crate) struct RealCache {}

#[derive(Debug)]
pub(crate) enum CacheConnectError {}

impl RealCache {
    /// TODO: docs.
    pub(crate) async fn connect(
        _owner: UserName,
        _name: CacheName,
        _auth: AuthToken,
    ) -> Result<Self, CacheConnectError> {
        todo!()
    }
}

impl context::Cache for RealCache {
    type Error = Infallible;

    async fn has_nar(
        &self,
        _nar_filename: &NarFileName,
    ) -> Result<bool, Self::Error> {
        todo!()
    }

    async fn has_narinfo(
        &self,
        _narinfo_filename: &NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        todo!()
    }

    async fn write_nar(&self, _nar_bytes: Bytes) -> Result<(), Self::Error> {
        todo!()
    }

    async fn write_narinfo(
        &self,
        _narinfo: NarInfo<impl fmt::Display, StoreDir>,
    ) -> Result<(), Self::Error> {
        todo!()
    }
}

impl fmt::Display for CacheConnectError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl core::error::Error for CacheConnectError {}
