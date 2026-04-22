use core::ffi::CStr;
use core::fmt;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use nix_compat::nar::writer::r#async as nar_writer;
use nix_types::{
    CompressionAlgorithm,
    NarFileName,
    NarInfo,
    Nix32Digest,
    NixStoreBaseName,
    NixStorePath,
};
use nixb::store::GetFsClosureOpts;
use sha2::{Digest, Sha256};
use smol_str::SmolStr;
use tokio::fs;
use tokio::io::BufReader;

use crate::context;
use crate::protocol::StoreDir;

#[derive(Clone)]
pub(crate) struct NixStore {
    store: nixb::store::Store,
}

#[derive(Debug)]
pub(crate) enum NixStoreError {
    Io(io::Error),
    Nix(nixb::Error),
}

impl NixStore {
    pub(crate) fn open() -> nixb::Result<Self> {
        let mut ctx = nixb::contexts::c_context::CContext::create();
        let init = nixb::store::init::<true>(&mut ctx)?;
        let store = nixb::store::Store::open(init, c"", [], ctx)?;
        Ok(Self { store })
    }
}

impl context::Nix for NixStore {
    type Error = NixStoreError;

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
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<(Bytes, NarFileName), Self::Error> {
        let fs_path = PathBuf::from(store_path.to_string());
        let mut nar_bytes = Vec::new();
        let root = nar_writer::open(&mut nar_bytes).await?;
        write_nar_path(&fs_path, root).await?;
        let file_hash = Nix32Digest::new(&Sha256::digest(&nar_bytes).into());
        Ok((nar_bytes.into(), NarFileName { file_hash, extension: None }))
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

        res.map_err(NixStoreError::from)
    }
}

async fn write_nar_path(
    path: &Path,
    nar_node: nar_writer::Node<'_, '_>,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path).await?;
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        let mut directory = nar_node.directory().await?;
        for (name, entry_path) in read_dir_entries(path).await? {
            let child = directory.entry(&name).await?;
            Box::pin(write_nar_path(&entry_path, child)).await?;
        }
        directory.close().await?;
    } else if file_type.is_symlink() {
        let target = fs::read_link(path).await?;
        nar_node.symlink(target.as_os_str().as_bytes()).await?;
    } else if file_type.is_file() {
        // If it's executable by the user, it'll become executable. This matches
        // nix's dump() function behaviour.
        let executable = metadata.permissions().mode() & 0o100 != 0;
        let file = fs::File::open(path).await?;
        let mut reader = BufReader::new(file);
        nar_node.file(executable, metadata.len(), &mut reader).await?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported file type at {}", path.display()),
        ));
    }

    Ok(())
}

async fn read_dir_entries(path: &Path) -> io::Result<Vec<(Vec<u8>, PathBuf)>> {
    let mut read_dir = fs::read_dir(path).await?;
    let mut entries = vec![];

    while let Some(entry) = read_dir.next_entry().await? {
        entries.push((entry.file_name().into_vec(), entry.path()));
    }

    entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    Ok(entries)
}

impl fmt::Display for NixStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Nix(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for NixStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Nix(error) => Some(error),
        }
    }
}

impl From<io::Error> for NixStoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<nixb::Error> for NixStoreError {
    fn from(value: nixb::Error) -> Self {
        Self::Nix(value)
    }
}
