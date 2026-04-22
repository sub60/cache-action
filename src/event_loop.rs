use core::fmt;
use core::num::NonZero;
use core::pin::pin;
use std::collections::HashSet;

use async_compat::Compat;
use either::Either;
use futures::stream::{FusedStream, FuturesUnordered};
use futures::{AsyncRead, AsyncWrite, StreamExt, future, select};
use nix_types::{
    CompressionAlgorithm,
    NarFileName,
    NarInfo,
    NarInfoFileName,
    NixStorePath,
};

use crate::context::{
    Cache,
    Context,
    DrainProgressReporter,
    JoinHandle,
    Nix,
    Spawner,
};
use crate::protocol::{self, StoreDir};

const NAR_STREAM_BUFFER_SIZE: usize = 64 * 1024;

pub(crate) trait Io: Send + 'static {
    type Reader: AsyncRead + Send;
    type Writer: AsyncWrite + Send;
    fn split(self) -> (Self::Reader, Self::Writer);
}

pub(crate) struct ActionReport<Ctx: Context> {
    /// The number of bytes pushed to the cache.
    pub(crate) num_bytes_pushed: u64,

    /// The number of store paths pushed to the cache.
    pub(crate) num_paths_pushed: u32,

    /// The number of store paths already stored in the cache
    pub(crate) num_paths_skipped: u32,

    /// The list of store paths for which it wasn't possible to compute their
    /// closure, together with the corresponding error.
    pub(crate) path_closure_errors:
        Vec<(NixStorePath<StoreDir>, <Ctx::Nix as Nix>::Error)>,

    /// The list of store paths for which it wasn't possible to compute their
    /// closure, together with the corresponding error.
    #[expect(clippy::type_complexity)]
    pub(crate) path_handling_errors:
        Vec<(NixStorePath<StoreDir>, HandlePathError<Ctx::Cache, Ctx::Nix>)>,
}

/// The successful outcome of [handling](handle_store_path) a store path.
pub(crate) enum HandlePathOutcome {
    /// The store path's NAR and NARInfo were both pushed to the cache.
    PushedNarAndNarInfo { nar_size: u64, narinfo_size: u64 },

    /// The store path's NARInfo was pushed to the cache, but the NAR was
    /// already cached.
    PushedNarInfo { narinfo_size: u64 },

    /// The path was already cached.
    Skipped,
}

/// The type of error that can occur when [handling](handle_store_path) a store
/// path.
pub(crate) enum HandlePathError<C: Cache, N: Nix> {
    CheckHasNar(C::Error),
    CheckHasNarInfo(C::Error),
    GetNarHash(N::Error),
    WriteNarFromStore(N::Error),
    WriteNarInfo(C::Error),
    WriteNarToCache(C::Error),
}

enum Event<Writer> {
    PushStorePath(NixStorePath<StoreDir>),
    Drain(Writer),
}

pub(crate) async fn run<Ctx: Context>(
    cache: Ctx::Cache,
    mut io_stream: impl FusedStream<Item = impl Io> + Unpin,
    ctx: &mut Ctx,
) {
    let (message_tx, message_rx) = flume::unbounded();

    let mut store_closures = FuturesUnordered::new();
    let mut handle_store_paths = FuturesUnordered::new();
    let mut handled_store_hashes = HashSet::new();

    let mut report = ActionReport::<Ctx>::default();

    let drain_progress_writer = loop {
        select! {
            io = io_stream.select_next_some() => {
                ctx.spawner().spawn(handle_io(io, message_tx.clone())).detach();
            },
            message_res = message_rx.recv_async() => {
                match message_res.expect("message_tx is still alive") {
                    Ok(Event::PushStorePath(store_path)) => {
                        let mut nix = ctx.nix().clone();
                        let fut = async move {
                            nix.store_closure(&store_path)
                                .await
                                .map_err(|err| (store_path, err))
                        };
                        store_closures.push(ctx.spawner().spawn(fut));
                    },
                    Ok(Event::Drain(writer)) => break writer,
                    Err(rx_err) => ctx.handle_rx_error(rx_err),
                }
            },
            closure_res = store_closures.select_next_some() => {
                match closure_res {
                    Ok(store_paths) => {
                        for path in store_paths {
                            // Skip this path if it's already been handled.
                            if handled_store_hashes.contains(path.hash()) {
                                continue;
                            }
                            let cache = cache.clone();
                            let nix = ctx.nix().clone();
                            let fut = async move {
                                (
                                    handle_store_path(&path, cache, nix).await,
                                    path,
                                )
                            };
                            handle_store_paths.push(ctx.spawner().spawn(fut));
                        }
                    },
                    Err(tuple) => report.path_closure_errors.push(tuple),
                }
            },
            (result, path) = handle_store_paths.select_next_some() => {
                handled_store_hashes.insert(*path.hash());
                match result {
                    Ok(outcome) => outcome.update_report(&mut report),
                    Err(err) => report.path_handling_errors.push((path, err)),
                }
            },
        };
    };

    let writer = pin!(drain_progress_writer);

    let mut reporter = ctx.new_drain_progress_reporter(writer);

    reporter.report_paths_left_to_handle(handle_store_paths.len() as u32).await;

    while let Some((result, path)) = handle_store_paths.next().await {
        match result {
            Ok(outcome) => {
                reporter.report_path_handling_outcome(&path, &outcome).await;
                outcome.update_report(&mut report);
            },
            Err(err) => {
                reporter.report_path_handling_error(&path, &err).await;
                report.path_handling_errors.push((path, err));
            },
        }
    }

    reporter.report_final_report(report).await;
}

async fn handle_io<I: Io>(
    io: I,
    message_tx: flume::Sender<Result<Event<I::Writer>, protocol::ReceiveError>>,
) {
    let (reader, writer) = io.split();
    let reader = pin!(reader);
    let mut message_rx = protocol::Receiver::new(reader);
    loop {
        let Some(message_res) = message_rx.next().await else { return };
        match message_res {
            Ok(protocol::Message::PushStorePath(store_path)) => {
                let _ = message_tx.send(Ok(Event::PushStorePath(store_path)));
            },
            Ok(protocol::Message::DrainDaemon) => {
                let _ = message_tx.send(Ok(Event::Drain(writer)));
                return;
            },
            Err(err) => {
                let _ = message_tx.send(Err(err));
            },
        }
    }
}

async fn handle_store_path<C: Cache, N: Nix>(
    store_path: &NixStorePath<StoreDir>,
    cache: C,
    mut nix: N,
) -> Result<HandlePathOutcome, HandlePathError<C, N>> {
    let narinfo_filename = NarInfoFileName { store_hash: *store_path.hash() };

    // If the cache already has the NARInfo then it must also have the NAR, so
    // we can skip this path.
    if cache
        .has_narinfo(&narinfo_filename)
        .await
        .map_err(HandlePathError::CheckHasNarInfo)?
    {
        return Ok(HandlePathOutcome::Skipped);
    }

    let nar_hash = nix
        .get_nar_hash(store_path)
        .await
        .map_err(HandlePathError::GetNarHash)?;

    let nar_filename = NarFileName { file_hash: nar_hash, extension: None };

    let has_nar = cache
        .has_nar(&nar_filename)
        .await
        .map_err(HandlePathError::CheckHasNar)?;

    let mut nar_size = 0;

    // Just like `nix copy --to`, we write the NAR *before* the NARInfo to avoid
    // the cache server temporarily reporting false positives.
    if !has_nar {
        let (reader, writer) = tokio::io::duplex(NAR_STREAM_BUFFER_SIZE);

        let (nix_res, cache_res) = future::join(
            nix.write_nar(store_path, Compat::new(writer)),
            cache.write_nar(nar_filename, Compat::new(reader)),
        )
        .await;

        nix_res.map_err(|err| match err {
            Either::Left(_io_err) => unreachable!("in memory duplex"),
            Either::Right(nix_err) => {
                HandlePathError::WriteNarFromStore(nix_err)
            },
        })?;

        cache_res.map_err(|err| match err {
            Either::Left(_io_err) => unreachable!("in memory duplex"),
            Either::Right(cache_err) => {
                HandlePathError::WriteNarToCache(cache_err)
            },
        })?;
    }

    let nar_size = NonZero::new(nar_size).expect("NARs are never empty");

    let narinfo = NarInfo {
        store_path: store_path.clone(),
        url: "",
        compression: CompressionAlgorithm::None,
        file_hash: nar_hash,
        file_size: nar_size,
        nar_hash,
        nar_size,
        references: Default::default(),
        deriver: Default::default(),
        signatures: Default::default(),
        content_address: Default::default(),
    };

    let narinfo_size = cache
        .write_narinfo(narinfo_filename, narinfo)
        .await
        .map_err(HandlePathError::WriteNarInfo)?;

    Ok(if has_nar {
        HandlePathOutcome::PushedNarInfo { narinfo_size }
    } else {
        HandlePathOutcome::PushedNarAndNarInfo {
            nar_size: nar_size.into(),
            narinfo_size,
        }
    })
}

impl HandlePathOutcome {
    fn update_report<Ctx: Context>(&self, report: &mut ActionReport<Ctx>) {
        match self {
            Self::PushedNarAndNarInfo { nar_size, narinfo_size } => {
                report.num_paths_pushed += 1;
                report.num_bytes_pushed += nar_size;
                report.num_bytes_pushed += narinfo_size;
            },
            Self::PushedNarInfo { narinfo_size } => {
                report.num_paths_pushed += 1;
                report.num_bytes_pushed += narinfo_size;
            },
            Self::Skipped => {
                report.num_paths_skipped += 1;
            },
        }
    }
}

impl<Ctx: Context> Default for ActionReport<Ctx> {
    fn default() -> Self {
        Self {
            num_bytes_pushed: 0,
            num_paths_pushed: 0,
            num_paths_skipped: 0,
            path_closure_errors: Default::default(),
            path_handling_errors: Default::default(),
        }
    }
}

impl<C: Cache, N: Nix> fmt::Display for HandlePathError<C, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CheckHasNar(err) => {
                write!(f, "couldn't check NAR existence on remote cache: {err}")
            },
            Self::CheckHasNarInfo(err) => {
                write!(
                    f,
                    "couldn't check NARInfo existence on remote cache: {err}"
                )
            },
            Self::GetNarHash(err) => {
                write!(f, "couldn't get NAR hash: {err}")
            },
            Self::WriteNarFromStore(err) => {
                write!(f, "couldn't get NAR bytes: {err}")
            },
            Self::WriteNarInfo(err) => {
                write!(f, "couldn't write NARInfo to remote cache: {err}")
            },
            Self::WriteNarToCache(err) => {
                write!(f, "couldn't write NAR to cache: {err}")
            },
        }
    }
}
