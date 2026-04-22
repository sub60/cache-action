use core::convert::Infallible;
use std::io;

use either::Either;

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
        nar_bytes: impl futures::AsyncRead + Send + 'static,
    ) -> Result<(), Either<io::Error, Self::Error>> {
        let mut sink = futures::io::sink();
        futures::io::copy(nar_bytes, &mut sink).await.map_err(Either::Left)?;
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
