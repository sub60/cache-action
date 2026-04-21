use core::ffi::CStr;
use std::ffi::CString;
use std::path::Path;

use bytes::Bytes;
use nix_types::{
    CompressionAlgorithm,
    NarFileName,
    NarInfo,
    Nix32Digest,
    NixStoreBaseName,
    NixStorePath,
};
use nixb::store::GetFsClosureOpts;
use smol_str::SmolStr;

use crate::context;
use crate::protocol::StoreDir;

pub(crate) struct NixStore {
    store: nixb::store::Store,
}

impl NixStore {
    pub(crate) fn open(store_dir: &Path) -> nixb::Result<Self> {
        let mut ctx = nixb::contexts::c_context::CContext::create();
        let init = nixb::store::init::<true>(&mut ctx)?;
        let store_uri = cfg_select! {
            unix => {
                {
                    use std::os::unix::ffi::OsStrExt;
                    CString::new(store_dir.as_os_str().as_bytes())
                        .expect("paths don't contain NUL bytes")
                }
            },
            windows => {
                compile_error!(
                    "don't know how to convert path to store URI on Windows"
                )
            },
            _ => compile_error!("unsupported platform"),
        };
        let store = nixb::store::Store::open(init, store_uri, [], ctx)?;
        Ok(Self { store })
    }
}

impl Clone for NixStore {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl context::Nix for NixStore {
    type Error = nixb::Error;

    async fn get_narinfo(
        &mut self,
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
        &mut self,
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
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<Vec<NixStorePath<StoreDir>>, Self::Error> {
        let mut store_path_bytes = store_path.to_string().into_bytes();
        store_path_bytes.push(0);

        // SAFETY: we just pushed a trailing NUL byte, and both the StoreDir and
        // the store basename are guaranteed to not contain any interior NULs.
        let store_path_cstr =
            unsafe { CStr::from_bytes_with_nul_unchecked(&store_path_bytes) };

        let store_dir = store_path.store_dir();

        let store_path = self.store.parse_path(store_path_cstr)?;

        let mut res = Ok(vec![]);

        let push_path = |path: &nixb::store::StorePath| {
            let Ok(paths) = &mut res else { return };
            let hash = match path.hash() {
                Ok(hash) => hash,
                Err(err) => {
                    res = Err(err);
                    return;
                },
            };
            let basename = NixStoreBaseName {
                hash: Nix32Digest::new(&hash),
                name: path.with_name(|n| n.parse()).expect("valid store name"),
            };
            let store_path =
                NixStorePath::new(basename).with_store_dir(store_dir.clone());
            paths.push(store_path);
        };

        self.store.get_fs_closure(
            &store_path,
            push_path,
            GetFsClosureOpts::default(),
        )?;

        res
    }
}
