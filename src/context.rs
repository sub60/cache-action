use core::error::Error;
use core::num::NonZeroU64;

use futures::{AsyncRead, AsyncWrite};
use nix_types::{
    ContentAddress,
    NarInfo,
    NarInfoFileName,
    Signature,
    StoreBasename,
    StorePath,
};

use crate::event_loop;
use crate::protocol::{self, StoreDir};

pub trait Context {
    type Cache: Cache;
    type DrainProgressReporter<W: AsyncWrite + Unpin>: DrainProgressReporter;
    type Nix: Nix;
    type Spawner: Spawner;

    fn handle_rx_error(&mut self, rx_error: protocol::ReceiveError);

    fn new_drain_progress_reporter<W: AsyncWrite + Unpin>(
        &mut self,
        writer: W,
    ) -> Self::DrainProgressReporter<W>;

    fn nix(&self) -> &Self::Nix;

    fn spawner(&self) -> &Self::Spawner;
}

pub trait Cache: Clone + Send + 'static {
    type NarUploadState: Send;
    type Error: Error + Send;

    fn has_narinfo(
        &self,
        narinfo_filename: &NarInfoFileName,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    fn initiate_nar_upload(
        &self,
        store_path: &StorePath<StoreDir>,
    ) -> impl Future<Output = Result<Self::NarUploadState, Self::Error>> + Send;

    fn upload_nar(
        &self,
        upload_state: &mut Self::NarUploadState,
        nar_bytes: impl AsyncRead + Send + 'static,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn upload_narinfo(
        &self,
        narinfo_filename: NarInfoFileName,
        narinfo: NarInfo<(), StoreDir>,
        nar_upload_state: Self::NarUploadState,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send;
}

pub trait DrainProgressReporter {
    fn report_paths_left_to_handle(
        &mut self,
        num_paths_left_to_handle: u32,
    ) -> impl Future<Output = ()>;

    fn report_path_pushed(
        &mut self,
        path: &StorePath<StoreDir>,
        num_bytes: NonZeroU64,
    ) -> impl Future<Output = ()>;

    fn report_path_skipped(
        &mut self,
        path: &StorePath<StoreDir>,
    ) -> impl Future<Output = ()>;

    fn report_path_handling_error<C: Cache, N: Nix>(
        &mut self,
        path: &StorePath<StoreDir>,
        error: &event_loop::HandlePathError<C, N>,
    ) -> impl Future<Output = ()>;

    fn report_final_report<Ctx: Context>(
        self,
        report: event_loop::ActionReport<Ctx>,
    ) -> impl Future<Output = ()>;
}

pub trait Nix: Clone + Send + 'static {
    type PathInfos<'path>: StorePathInfos<Error = Self::PathInfosError> + Send;

    type PathInfosError: Error + Send;
    type StoreClosureError: Error + Send;
    type WriteNarError: Error + Send;

    fn get_path_infos<'path>(
        &mut self,
        store_path: &'path StorePath<StoreDir>,
    ) -> impl Future<
        Output = Result<Self::PathInfos<'path>, Self::PathInfosError>,
    > + Send;

    fn store_closure(
        &mut self,
        store_path: &StorePath<StoreDir>,
    ) -> impl Future<
        Output = Result<Vec<StorePath<StoreDir>>, Self::StoreClosureError>,
    > + Send;

    fn write_nar(
        &mut self,
        store_path: &StorePath<StoreDir>,
        writer: impl AsyncWrite + Send,
    ) -> impl Future<Output = Result<(), Self::WriteNarError>> + Send;
}

pub trait StorePathInfos {
    type Error: Error + Send;

    fn content_address(
        &mut self,
    ) -> Result<Option<ContentAddress>, Self::Error>;

    fn deriver(&mut self) -> Result<Option<StoreBasename>, Self::Error>;

    fn references(
        &mut self,
    ) -> Result<impl IntoIterator<Item = StoreBasename>, Self::Error>;

    fn signatures(
        &mut self,
    ) -> Result<impl IntoIterator<Item = Signature>, Self::Error>;
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
