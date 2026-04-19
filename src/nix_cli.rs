use std::io;

use bytes::Bytes;
use nix_types::{
    CompressionAlgorithm,
    NarFileName,
    NarInfo,
    Nix32Digest,
    NixStorePath,
};
use smol_str::SmolStr;

use crate::context;
use crate::protocol::StoreDir;

#[derive(Clone)]
pub(crate) struct NixCli;

impl context::Nix for NixCli {
    type Error = io::Error;

    async fn get_narinfo(
        &self,
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<NarInfo<SmolStr, StoreDir>, Self::Error> {
        Ok(NarInfo {
            store_path: store_path.clone(),
            url: "".into(),
            compression: CompressionAlgorithm::None,
            file_hash: Nix32Digest::new(&[0; _]),
            file_size: 42.try_into().expect("not zero"),
            nar_hash: Nix32Digest::new(&[0; _]),
            nar_size: 42.try_into().expect("not zero"),
            references: Default::default(),
            deriver: None,
            signatures: Default::default(),
            content_address: None,
        })
    }

    async fn pack_nar(
        &self,
        _: &NixStorePath<StoreDir>,
    ) -> Result<(Bytes, NarFileName), Self::Error> {
        Ok((
            vec![42; 1].into(),
            NarFileName {
                file_hash: Nix32Digest::new(&[0; _]),
                extension: None,
            },
        ))
    }

    async fn store_closure(
        &self,
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<Vec<NixStorePath<StoreDir>>, Self::Error> {
        let mut ctx = nixb::contexts::c_context::CContext::create();
        let _init = nixb::store::init::<false>(&mut ctx);
        // let err = nixb_store::init(None);
        // println!("nixb_store::init: {err:?}");
        Ok(vec![store_path.clone()])
    }
}
