use futures::AsyncWrite;

use crate::nix_store::NixStore;
use crate::real_stop_progress_reporter::RealStopProgressReporter;
use crate::tokio::TokioSpawner;
use crate::{context, protocol};

pub(crate) struct RealContext {
    http_client: reqwest::Client,
    nix_store: NixStore,
    spawner: TokioSpawner,
}

impl RealContext {
    pub(crate) fn new(nix_store: NixStore, spawner: TokioSpawner) -> Self {
        Self { http_client: reqwest::Client::new(), nix_store, spawner }
    }
}

impl context::Context for RealContext {
    #[cfg(feature = "noop-cache")]
    type Cache = crate::noop_cache::NoopCache;
    #[cfg(feature = "sub60-cache")]
    type Cache = crate::sub60_cache::Sub60Cache;
    type HttpClient = reqwest::Client;
    type StopProgressReporter<W: AsyncWrite + Unpin> =
        RealStopProgressReporter<W>;
    type Nix = NixStore;
    type Spawner = TokioSpawner;

    fn handle_rx_error(&mut self, rx_error: protocol::ReceiveError) {
        eprintln!("{rx_error}")
    }

    fn http_client(&self) -> &Self::HttpClient {
        &self.http_client
    }

    fn new_stop_progress_reporter<W: AsyncWrite + Unpin>(
        &mut self,
        writer: W,
    ) -> Self::StopProgressReporter<W> {
        RealStopProgressReporter::new(writer)
    }

    fn nix(&self) -> &Self::Nix {
        &self.nix_store
    }

    fn spawner(&self) -> &Self::Spawner {
        &self.spawner
    }
}

impl context::HttpClient for reqwest::Client {
    type Error = reqwest::Error;

    async fn get(
        &self,
        url: url::Url,
    ) -> Result<http::StatusCode, Self::Error> {
        self.get(url).send().await.map(|response| response.status())
    }
}
