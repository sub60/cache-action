use core::ffi::CStr;
use core::num::NonZeroU64;
use core::pin::pin;
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use async_compat::Compat;
use either::Either;
use nix_compat::nar::writer::r#async as nar_writer;
use nix_types::{Nix32Digest, NixStoreBaseName, NixStorePath};
use nixb::store::GetFsClosureOpts;
use tokio::fs;
use tokio::io::{AsyncWriteExt as _, BufReader};

use crate::context;
use crate::protocol::StoreDir;

#[derive(Clone)]
pub(crate) struct NixStore {
    store: nixb::store::Store,
}

impl NixStore {
    pub(crate) fn open() -> nixb::Result<Self> {
        let mut ctx = nixb::contexts::c_context::CContext::create();
        let init = nixb::store::init::<true>(&mut ctx)?;
        let store = nixb::store::Store::open(init, c"", [], ctx)?;
        Ok(Self { store })
    }

    fn parse_path(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> nixb::Result<nixb::store::StorePath> {
        let mut bytes = store_path.to_string().into_bytes();
        bytes.push(0);
        // SAFETY: we just pushed a trailing NUL byte, and both the StoreDir and
        // the store basename are guaranteed to not contain any interior NULs.
        let cstr = unsafe { CStr::from_bytes_with_nul_unchecked(&bytes) };
        self.store.parse_path(cstr)
    }
}

impl context::Nix for NixStore {
    type Error = nixb::Error;

    async fn get_nar_hash(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<Nix32Digest<32>, Self::Error> {
        let path = self.parse_path(store_path)?;
        self.store.query_path_info(&path)?.with_nar_hash(|hash| {
            hash.strip_prefix("sha256:")
                .expect("Nix nar hashes are always sha256")
                .parse()
                .expect("Nix nar hashes must use valid nix base32")
        })
    }

    async fn get_nar_size(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<NonZeroU64, Self::Error> {
        let path = self.parse_path(store_path)?;
        self.store.query_path_info(&path)?.get_nar_size()?.ok_or_else(|| {
            nixb::Error::from_message(format_args!(
                "unknown NAR size for {store_path}"
            ))
        })
    }

    async fn write_nar(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
        writer: impl futures::AsyncWrite + Send,
    ) -> Result<(), Either<io::Error, Self::Error>> {
        let store_path = store_path.to_string();
        let mut writer = pin!(Compat::new(writer));
        let root = nar_writer::open(&mut writer).await.map_err(Either::Left)?;
        write_nar(root, Path::new(&store_path)).await.map_err(Either::Left)?;
        writer.shutdown().await.map_err(Either::Left)
    }

    async fn store_closure(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> Result<Vec<NixStorePath<StoreDir>>, Self::Error> {
        let path = self.parse_path(store_path)?;

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
            let store_dir = store_path.store_dir().clone();
            paths.push(NixStorePath::new(basename).with_store_dir(store_dir));
        };

        self.store.get_fs_closure(
            &path,
            push_path,
            GetFsClosureOpts::default(),
        )?;

        res
    }
}

async fn write_nar(
    node: nar_writer::Node<'_, '_>,
    node_path: &Path,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(node_path).await?;
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        let mut directory = node.directory().await?;
        let mut child_path = node_path.to_owned();
        for child_name in read_dir_entries(node_path).await? {
            let child = directory.entry(child_name.as_bytes()).await?;
            child_path.push(&child_name);
            Box::pin(write_nar(child, &child_path)).await?;
            child_path.pop();
        }
        return directory.close().await;
    }

    if file_type.is_symlink() {
        let target = fs::read_link(node_path).await?;
        return node.symlink(target.as_os_str().as_bytes()).await;
    }

    if file_type.is_file() {
        let executable = metadata.permissions().mode() & 0o100 != 0;
        let file = fs::File::open(node_path).await?;
        let mut reader = BufReader::new(file);
        return node.file(executable, metadata.len(), &mut reader).await;
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("unsupported file type at {}", node_path.display()),
    ))
}

/// Returns the file names of the children of the directory at the given path,
/// sorted lexicographically.
async fn read_dir_entries(dir_path: &Path) -> io::Result<Vec<OsString>> {
    let mut read_dir = fs::read_dir(dir_path).await?;
    let mut file_names = vec![];
    while let Some(entry) = read_dir.next_entry().await? {
        file_names.push(entry.file_name());
    }
    file_names.sort();
    Ok(file_names)
}
