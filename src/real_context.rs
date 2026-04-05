use crate::nix_cli::NixCli;
use crate::real_cache::RealCache;
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
    type Nix = NixCli;
    type Spawner = TokioSpawner;

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
