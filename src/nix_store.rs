use core::ffi::CStr;
use core::fmt;
use core::pin::pin;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use async_compat::Compat;
use either::Either;
use nix_compat::nar::writer::r#async as nar_writer;
use nix_types::{
    Nix32Digest,
    NixHashFromStrError,
    NixStoreBaseName,
    NixStorePath,
};
use nixb::store::GetFsClosureOpts;
use tokio::fs;
use tokio::io::{AsyncWriteExt as _, BufReader};

use crate::context;
use crate::protocol::StoreDir;

#[derive(Clone)]
pub(crate) struct NixStore {
    store: nixb::store::Store,
}

#[derive(Debug)]
pub(crate) enum NixStoreError {
    InvalidContentAddress(nix_types::ContentAddressFromStrError),
    InvalidNarHash(NixHashFromStrError),
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

    async fn get_nar_hash(
        &mut self,
        _store_path: &NixStorePath<StoreDir>,
    ) -> Result<Nix32Digest<32>, Self::Error> {
        todo!()
    }

    async fn write_nar(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
        writer: impl futures::AsyncWrite + Send,
    ) -> Result<(), Either<io::Error, Self::Error>> {
        let store_path = store_path.to_string();
        let mut writer = pin!(Compat::new(writer));
        let root = nar_writer::open(&mut writer).await.map_err(Either::Left)?;
        write_nar(Path::new(&store_path), root).await.map_err(Either::Left)?;
        writer.shutdown().await.map_err(Either::Left)
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

        res.map_err(Into::into)
    }
}

async fn write_nar(
    node_path: &Path,
    nar_node: nar_writer::Node<'_, '_>,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(node_path).await?;
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        let mut directory = nar_node.directory().await?;
        for (name, entry_path) in read_dir_entries(node_path).await? {
            let child = directory.entry(&name).await?;
            Box::pin(write_nar(&entry_path, child)).await?;
        }
        directory.close().await?;
    } else if file_type.is_symlink() {
        let target = fs::read_link(node_path).await?;
        nar_node.symlink(target.as_os_str().as_bytes()).await?;
    } else if file_type.is_file() {
        // If it's executable by the user, it'll become executable. This matches
        // nix's dump() function behaviour.
        let executable = metadata.permissions().mode() & 0o100 != 0;
        let file = fs::File::open(node_path).await?;
        let mut reader = BufReader::new(file);
        nar_node.file(executable, metadata.len(), &mut reader).await?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported file type at {}", node_path.display()),
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
            Self::InvalidContentAddress(error) => {
                write!(f, "couldn't parse content address: {error}")
            },
            Self::InvalidNarHash(error) => {
                write!(f, "couldn't parse NAR hash: {error}")
            },
            Self::Io(error) => error.fmt(f),
            Self::Nix(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for NixStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidContentAddress(error) => Some(error),
            Self::InvalidNarHash(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Nix(error) => Some(error),
        }
    }
}

impl From<nix_types::ContentAddressFromStrError> for NixStoreError {
    fn from(value: nix_types::ContentAddressFromStrError) -> Self {
        Self::InvalidContentAddress(value)
    }
}

impl From<NixHashFromStrError> for NixStoreError {
    fn from(value: NixHashFromStrError) -> Self {
        Self::InvalidNarHash(value)
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
