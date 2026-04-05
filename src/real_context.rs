use futures::AsyncWrite;

use crate::nix_cli::NixCli;
use crate::real_cache::RealCache;
use crate::real_drain_progress_reporter::RealDrainProgressReporter;
use crate::tokio::TokioSpawner;
use crate::{context, protocol};

pub(crate) struct RealContext {
    spawner: TokioSpawner,
}

impl RealContext {
    pub(crate) fn new(spawner: TokioSpawner) -> Self {
        Self { spawner }
    }
}

impl context::Context for RealContext {
    type Cache = RealCache;
    type DrainProgressReporter<W: AsyncWrite + Unpin> =
        RealDrainProgressReporter<W>;
    type Nix = NixCli;
    type Spawner = TokioSpawner;

    fn create_drain_progress_reporter<W: AsyncWrite + Unpin>(
        &mut self,
        writer: W,
    ) -> Self::DrainProgressReporter<W> {
        RealDrainProgressReporter::new(writer)
    }

    fn handle_rx_error(&mut self, rx_error: protocol::ReceiveError) {
        eprintln!("{rx_error}")
    }

    fn nix(&self) -> &Self::Nix {
        &NixCli
    }

    fn spawner(&self) -> &Self::Spawner {
        &self.spawner
    }
}
