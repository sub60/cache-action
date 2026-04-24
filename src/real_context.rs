use futures::AsyncWrite;

use crate::nix_store::NixStore;
use crate::real_drain_progress_reporter::RealDrainProgressReporter;
use crate::tokio::TokioSpawner;
use crate::{context, protocol};

pub(crate) struct RealContext {
    nix_store: NixStore,
    spawner: TokioSpawner,
}

impl RealContext {
    pub(crate) fn new(nix_store: NixStore, spawner: TokioSpawner) -> Self {
        Self { nix_store, spawner }
    }
}

impl context::Context for RealContext {
    #[cfg(feature = "noop-cache")]
    type Cache = crate::noop_cache::NoopCache;
    #[cfg(feature = "sub60-cache")]
    type Cache = crate::sub60_cache::Sub60Cache;
    type DrainProgressReporter<W: AsyncWrite + Unpin> =
        RealDrainProgressReporter<W>;
    type Nix = NixStore;
    type Spawner = TokioSpawner;

    fn handle_rx_error(&mut self, rx_error: protocol::ReceiveError) {
        eprintln!("{rx_error}")
    }

    fn new_drain_progress_reporter<W: AsyncWrite + Unpin>(
        &mut self,
        writer: W,
    ) -> Self::DrainProgressReporter<W> {
        RealDrainProgressReporter::new(writer)
    }

    fn nix(&self) -> &Self::Nix {
        &self.nix_store
    }

    fn spawner(&self) -> &Self::Spawner {
        &self.spawner
    }
}
