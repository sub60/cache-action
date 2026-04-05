use core::error::Error;
use core::fmt;

use bytes::Bytes;
use futures::Stream;
use nix_types::{NarFileName, NarInfo, NarInfoFileName, NixStorePath};
use smol_str::SmolStr;

use crate::protocol::StoreDir;

pub trait Context {
    /// TODO: docs.
    type Cache: Cache;

    /// TODO: docs.
    type Nix: Nix;

    /// TODO: docs.
    type Spawner: Spawner;

    /// TODO: docs.
    fn nix(&self) -> &Self::Nix;

    /// TODO: docs.
    fn spawner(&self) -> &Self::Spawner;
}

pub trait Cache {
    type Error: Error;

    fn has_nar(
        &self,
        nar_filename: &NarFileName,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    fn has_narinfo(
        &self,
        narinfo_filename: &NarInfoFileName,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    fn write_nar(
        &self,
        nar_bytes: Bytes,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn write_narinfo(
        &self,
        narinfo: NarInfo<impl fmt::Display + Send, StoreDir>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

pub trait Nix {
    type Error: Error;

    fn nar(
        &self,
        store_path: &NixStorePath<StoreDir>,
    ) -> impl Future<Output = Result<Bytes, Self::Error>>;

    fn narinfo(
        &self,
        store_path: &NixStorePath<StoreDir>,
    ) -> impl Future<Output = Result<NarInfo<SmolStr, StoreDir>, Self::Error>>;

    fn store_closure(
        &self,
        store_path: &NixStorePath<StoreDir>,
    ) -> impl Future<
        Output = Result<
            impl Stream<Item = NixStorePath<StoreDir>>,
            Self::Error,
        >,
    >;
}

pub trait Spawner {
    type JoinHandle<Fut: Future>: JoinHandle<Fut::Output>;

    fn spawn<Fut>(&self, future: Fut) -> Self::JoinHandle<Fut>
    where
        Fut: Future + Send + Sync + 'static,
        Fut::Output: Send + Sync + 'static;
}

pub trait JoinHandle<Output>: Future<Output = Output> {
    fn detach(self);
}
