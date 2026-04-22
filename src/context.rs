use core::error::Error;
use core::fmt;
use std::io;

use either::Either;
use futures::{AsyncRead, AsyncWrite};
use nix_types::{
    NarFileName,
    NarInfo,
    NarInfoFileName,
    Nix32Digest,
    NixStorePath,
};

use crate::event_loop;
use crate::protocol::{self, StoreDir};

pub trait Context {
    /// TODO: docs.
    type Cache: Cache;

    /// TODO: docs.
    type DrainProgressReporter<W: AsyncWrite + Unpin>: DrainProgressReporter;

    /// TODO: docs.
    type Nix: Nix;

    /// TODO: docs.
    type Spawner: Spawner;

    /// TODO: docs.
    fn handle_rx_error(&mut self, rx_error: protocol::ReceiveError);

    /// TODO: docs.
    fn new_drain_progress_reporter<W: AsyncWrite + Unpin>(
        &mut self,
        writer: W,
    ) -> Self::DrainProgressReporter<W>;

    /// TODO: docs.
    fn nix(&self) -> &Self::Nix;

    /// TODO: docs.
    fn spawner(&self) -> &Self::Spawner;
}

pub trait Cache: Clone + Send + 'static {
    type Error: Error + Send;

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
        nar_filename: NarFileName,
        nar_bytes: impl AsyncRead + Send + 'static,
    ) -> impl Future<Output = Result<(), Either<io::Error, Self::Error>>> + Send;

    fn write_narinfo(
        &self,
        narinfo_filename: NarInfoFileName,
        narinfo: NarInfo<impl fmt::Display + Send, StoreDir>,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send;
}

pub trait DrainProgressReporter {
    fn report_paths_left_to_handle(
        &mut self,
        num_paths_left_to_handle: u32,
    ) -> impl Future<Output = ()>;

    fn report_path_handling_outcome(
        &mut self,
        path: &NixStorePath<StoreDir>,
        outcome: &event_loop::HandlePathOutcome,
    ) -> impl Future<Output = ()>;

    fn report_path_handling_error<C: Cache, N: Nix>(
        &mut self,
        path: &NixStorePath<StoreDir>,
        error: &event_loop::HandlePathError<C, N>,
    ) -> impl Future<Output = ()>;

    fn report_final_report<Ctx: Context>(
        self,
        report: event_loop::ActionReport<Ctx>,
    ) -> impl Future<Output = ()>;
}

pub trait Nix: Clone + Send + 'static {
    type Error: Error + Send;

    fn get_nar_hash(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> impl Future<Output = Result<Nix32Digest<32>, Self::Error>> + Send;

    fn store_closure(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
    ) -> impl Future<Output = Result<Vec<NixStorePath<StoreDir>>, Self::Error>> + Send;

    fn write_nar(
        &mut self,
        store_path: &NixStorePath<StoreDir>,
        writer: impl AsyncWrite + Send,
    ) -> impl Future<Output = Result<(), Either<io::Error, Self::Error>>> + Send;
}

pub trait Spawner {
    type JoinHandle<Fut: Future>: JoinHandle<Fut::Output>;

    fn spawn<Fut>(&self, future: Fut) -> Self::JoinHandle<Fut>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static;
}

pub trait JoinHandle<Output>: Future<Output = Output> {
    fn detach(self);
}
