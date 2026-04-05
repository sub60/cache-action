use core::convert::Infallible;

use bytes::Bytes;
use nix_types::{NarFileName, NarInfo, NixStorePath};
use smol_str::SmolStr;

use crate::context;
use crate::protocol::StoreDir;

#[derive(Clone)]
pub(crate) struct NixCli;

impl context::Nix for NixCli {
    type Error = Infallible;

    async fn get_narinfo(
        &self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<NarInfo<SmolStr, StoreDir>, Self::Error> {
        todo!()
    }

    async fn pack_nar(
        &self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<(Bytes, NarFileName), Self::Error> {
        todo!()
    }

    async fn store_closure(
        &self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<Vec<NixStorePath<StoreDir>>, Self::Error> {
        todo!()
    }
}
