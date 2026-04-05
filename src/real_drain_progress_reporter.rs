use futures::AsyncWrite;
use nix_types::NixStorePath;

use crate::protocol::StoreDir;
use crate::{context, event_loop};

pub(crate) struct RealDrainProgressReporter<W> {
    writer: W,
}

impl<W> RealDrainProgressReporter<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: AsyncWrite + Unpin> context::DrainProgressReporter
    for RealDrainProgressReporter<W>
{
    async fn report_paths_left_to_handle(
        &mut self,
        _num_paths_left_to_handle: u32,
    ) {
        todo!()
    }

    async fn report_path_handling_outcome(
        &mut self,
        _path: &NixStorePath<StoreDir>,
        _outcome: &event_loop::HandlePathOutcome,
    ) {
        todo!()
    }

    async fn report_path_handling_error<C: context::Cache, N: context::Nix>(
        &mut self,
        _path: &NixStorePath<StoreDir>,
        _error: &event_loop::HandlePathError<C, N>,
    ) {
        todo!()
    }

    async fn report_final_report<Ctx: context::Context>(
        self,
        _report: event_loop::ActionReport<Ctx>,
    ) {
        todo!()
    }
}
