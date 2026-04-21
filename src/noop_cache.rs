use core::convert::Infallible;

use crate::context::Cache;
use crate::protocol::StoreDir;

#[derive(Default, Clone, Copy)]
pub(crate) struct NoopCache {}

impl Cache for NoopCache {
    type Error = Infallible;

    async fn has_nar(
        &self,
        _: &nix_types::NarFileName,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn has_narinfo(
        &self,
        _: &nix_types::NarInfoFileName,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn write_nar(
        &self,
        _: nix_types::NarFileName,
        _: bytes::Bytes,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn write_narinfo(
        &self,
        _: nix_types::NarInfoFileName,
        narinfo: nix_types::NarInfo<impl core::fmt::Display + Send, StoreDir>,
    ) -> Result<u64, Self::Error> {
        Ok(narinfo.to_string().len() as u64)
    }
}
