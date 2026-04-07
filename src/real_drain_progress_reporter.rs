use futures::{AsyncWrite, AsyncWriteExt};
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
    }

    async fn report_path_handling_outcome(
        &mut self,
        _path: &NixStorePath<StoreDir>,
        _outcome: &event_loop::HandlePathOutcome,
    ) {
    }

    async fn report_path_handling_error<C: context::Cache, N: context::Nix>(
        &mut self,
        _path: &NixStorePath<StoreDir>,
        _error: &event_loop::HandlePathError<C, N>,
    ) {
    }

    async fn report_final_report<Ctx: context::Context>(
        mut self,
        report: event_loop::ActionReport<Ctx>,
    ) {
        let msg = format!(
            "Pushed {} paths, {} bytes",
            report.num_paths_pushed, report.num_bytes_pushed
        );
        let _ = self.writer.write_all(msg.as_bytes()).await;
        let _ = self.writer.flush().await;
    }
}
