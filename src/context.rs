use core::error::Error;
use core::fmt;

use bytes::Bytes;
use futures::Stream;
use nix_types::{
    CacheName,
    NarFileName,
    NarInfo,
    NarInfoFileName,
    NixStorePath,
    UserName,
};
use smol_str::SmolStr;

use crate::AuthToken;

pub trait Context {
    /// TODO: docs.
    type Cache: Cache;

    /// TODO: docs.
    type Nix: Nix;

    /// TODO: docs.
    type Runtime: Runtime;

    /// The type of error returned when [`cache`](Context::cache) fails.
    type CacheError: Error;

    /// TODO: docs.
    fn cache(
        &mut self,
        owner: UserName,
        name: CacheName,
        auth: AuthToken,
    ) -> impl Future<Output = Result<Self::Cache, Self::CacheError>>;

    /// TODO: docs.
    fn nix(&self) -> Self::Nix;

    /// TODO: docs.
    fn runtime(&self) -> Self::Runtime;
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
        narinfo: NarInfo<impl fmt::Display>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

pub trait Nix {
    type Error: Error;

    fn nar(
        &self,
        store_path: &NixStorePath,
    ) -> impl Future<Output = Result<Bytes, Self::Error>>;

    fn narinfo(
        &self,
        store_path: &NixStorePath,
    ) -> impl Future<Output = Result<NarInfo<SmolStr>, Self::Error>>;

    fn store_closure(
        &self,
        store_path: &NixStorePath,
    ) -> impl Future<Output = Result<impl Stream<Item = NixStorePath>, Self::Error>>;
}

pub trait Runtime {
    type Handle<Fut: Future>: RuntimeHandle<Fut::Output>;

    fn block_on<Fut: Future>(&self, future: Fut) -> Fut::Output;

    fn spawn<Fut>(&self, future: Fut) -> Self::Handle<Fut>
    where
        Fut: Future + Send + Sync + 'static,
        Fut::Output: Send + Sync + 'static;
}

pub trait RuntimeHandle<Output>: Future<Output = Output> {
    fn detach(self);
}
