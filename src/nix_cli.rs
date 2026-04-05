use core::convert::Infallible;

use bytes::Bytes;
use nix_types::{NarInfo, NixStorePath};
use smol_str::SmolStr;

use crate::context;
use crate::protocol::StoreDir;

#[derive(Clone)]
pub(crate) struct NixCli;

impl context::Nix for NixCli {
    type Error = Infallible;

    async fn nar(
        &self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<Bytes, Self::Error> {
        todo!()
    }

    async fn narinfo(
        &self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<NarInfo<SmolStr, StoreDir>, Self::Error> {
        todo!()
    }

    async fn store_closure(
        &self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<Vec<NixStorePath<StoreDir>>, Self::Error> {
        todo!()
    }
}
