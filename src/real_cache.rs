use core::convert::Infallible;
use core::fmt;

use bytes::Bytes;
use nix_types::{NarFileName, NarInfo, NarInfoFileName};

use crate::context;
use crate::protocol::StoreDir;

pub(crate) struct RealCache {}

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
