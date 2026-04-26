use core::fmt;
use core::num::NonZeroU64;
use core::pin::{Pin, pin};
use core::task::{Context as TaskContext, Poll};
use std::collections::HashSet;
use std::io;

use async_compat::Compat;
use futures::stream::{FusedStream, FuturesUnordered};
use futures::{AsyncRead, AsyncWrite, StreamExt, future, select};
use nix_types::{
    CompressionAlgorithm,
    NarInfo,
    NarInfoFileName,
    Nix32Digest,
    StorePath,
};
use pin_project_lite::pin_project;
use sha2::{Digest, Sha256};

use crate::context::{
    Cache,
    Context,
    JoinHandle,
    Nix,
    Spawner,
    StopProgressReporter,
    StorePathInfos,
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
        Vec<(StorePath<StoreDir>, <Ctx::Nix as Nix>::StoreClosureError)>,

    /// The list of store paths for which it wasn't possible to compute their
    /// closure, together with the corresponding error.
    #[expect(clippy::type_complexity)]
    pub(crate) path_handling_errors:
        Vec<(StorePath<StoreDir>, HandlePathError<Ctx::Cache, Ctx::Nix>)>,
}

/// The type of error that can occur when [handling](handle_store_path) a store
/// path.
pub(crate) enum HandlePathError<C: Cache, N: Nix> {
    CheckNarInfoExistence(C::Error),
    CreateNarUploadId(C::Error),
    GetPathInfos(N::PathInfosError),
    UploadNar(C::Error),
    UploadNarInfo(C::Error),
    WriteNar(N::WriteNarError),
    WriteNarWroteZeroBytes,
}

enum Event<Writer> {
    PushStorePath(StorePath<StoreDir>),
    Stop(Writer),
}

pin_project! {
    /// An [`AsyncWrite`] adapter which maintains a running SHA-256 hash of the
    /// written bytes and counts how many bytes were successfully written.
    struct HashAndCountWriter<W> {
        #[pin]
        inner: W,
        sha256: Sha256,
        num_bytes_written: u64,
    }
}

impl<W> HashAndCountWriter<W> {
    fn new(writer: W) -> Self {
        Self { inner: writer, sha256: Sha256::new(), num_bytes_written: 0 }
    }
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

    let stop_progress_writer = loop {
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
                    Ok(Event::Stop(writer)) => break writer,
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
                    Ok(Some(num_bytes)) => {
                        report.num_bytes_pushed += u64::from(num_bytes);
                        report.num_paths_pushed += 1;
                    },
                    Ok(None) => report.num_paths_skipped += 1,
                    Err(err) => report.path_handling_errors.push((path, err)),
                }
            },
        };
    };

    let writer = pin!(stop_progress_writer);

    let mut reporter = ctx.new_stop_progress_reporter(writer);

    reporter.report_paths_left_to_handle(handle_store_paths.len() as u32).await;

    while let Some((result, path)) = handle_store_paths.next().await {
        match result {
            Ok(Some(num_bytes)) => {
                reporter.report_path_pushed(&path, num_bytes).await;
                report.num_bytes_pushed += u64::from(num_bytes);
                report.num_paths_pushed += 1;
            },
            Ok(None) => {
                reporter.report_path_skipped(&path).await;
                report.num_paths_skipped += 1;
            },
            Err(err) => {
                reporter.report_path_handling_error(&path, &err).await;
                report.path_handling_errors.push((path, err))
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
            Ok(protocol::Message::StopDaemon) => {
                let _ = message_tx.send(Ok(Event::Stop(writer)));
                return;
            },
            Err(err) => {
                let _ = message_tx.send(Err(err));
            },
        }
    }
}

/// Handles the given store path by first checking if it's already cached, then
/// uploading the path's NAR (compressed) and NARInfo if it isn't.
///
/// Returns the total number of bytes pushed to the cache, or `None` if the path
/// was cached.
async fn handle_store_path<C: Cache, N: Nix>(
    store_path: &StorePath<StoreDir>,
    cache: C,
    mut nix: N,
) -> Result<Option<NonZeroU64>, HandlePathError<C, N>> {
    let narinfo_filename = NarInfoFileName { store_hash: *store_path.hash() };

    // If the cache already has the NARInfo then it must also have the NAR, so
    // we can skip this path.
    if cache
        .has_narinfo(&narinfo_filename)
        .await
        .map_err(HandlePathError::CheckNarInfoExistence)?
    {
        return Ok(None);
    }

    let mut nar_upload_state = cache
        .initiate_nar_upload(store_path)
        .await
        .map_err(HandlePathError::CreateNarUploadId)?;

    // Just like `nix copy --to`, we write the NAR *before* the NARInfo to avoid
    // the cache server temporarily reporting false positives.

    let (reader, writer) = tokio::io::duplex(NAR_STREAM_BUFFER_SIZE);

    let mut writer = HashAndCountWriter::new(Compat::new(writer));

    let (nix_res, cache_res) = future::join(
        nix.write_nar(store_path, &mut writer),
        cache.upload_nar(&mut nar_upload_state, Compat::new(reader)),
    )
    .await;

    nix_res.map_err(HandlePathError::WriteNar)?;
    cache_res.map_err(HandlePathError::UploadNar)?;

    let nar_hash = Nix32Digest::new(&writer.sha256.finalize().into());

    let nar_size = NonZeroU64::new(writer.num_bytes_written)
        .ok_or(HandlePathError::WriteNarWroteZeroBytes)?;

    let mut infos = nix
        .get_path_infos(store_path)
        .await
        .map_err(HandlePathError::GetPathInfos)?;

    let references = infos
        .references()
        .map_err(HandlePathError::GetPathInfos)?
        .into_iter()
        .collect();

    let deriver = infos.deriver().map_err(HandlePathError::GetPathInfos)?;

    let signatures = infos
        .signatures()
        .map_err(HandlePathError::GetPathInfos)?
        .into_iter()
        .collect();

    let content_address =
        infos.content_address().map_err(HandlePathError::GetPathInfos)?;

    let narinfo = NarInfo {
        store_path: store_path.clone(),
        url: (),
        compression: CompressionAlgorithm::None,
        file_hash: nar_hash,
        file_size: nar_size,
        nar_hash,
        nar_size,
        references,
        deriver,
        signatures,
        content_address,
    };

    let narinfo_size = cache
        .upload_narinfo(narinfo_filename, narinfo, nar_upload_state)
        .await
        .map_err(HandlePathError::UploadNarInfo)?;

    Ok(Some(nar_size.saturating_add(narinfo_size)))
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
            Self::CheckNarInfoExistence(err) => {
                write!(
                    f,
                    "couldn't check NARInfo existence on remote cache: {err}"
                )
            },
            Self::CreateNarUploadId(err) => {
                write!(f, "couldn't create NAR upload ID: {err}")
            },
            Self::GetPathInfos(err) => {
                write!(f, "couldn't get store path infos: {err}")
            },
            Self::UploadNar(err) => {
                write!(f, "couldn't upload NAR to cache: {err}")
            },
            Self::UploadNarInfo(err) => {
                write!(f, "couldn't upload NARInfo to cache: {err}")
            },
            Self::WriteNar(err) => {
                write!(f, "couldn't get NAR from store: {err}")
            },
            Self::WriteNarWroteZeroBytes => {
                write!(f, "got empty NAR from store")
            },
        }
    }
}

impl<W: AsyncWrite> AsyncWrite for HashAndCountWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        match this.inner.poll_write(cx, buf) {
            Poll::Ready(Ok(num_bytes)) => {
                this.sha256.update(&buf[..num_bytes]);
                *this.num_bytes_written += num_bytes as u64;
                Poll::Ready(Ok(num_bytes))
            },
            other => other,
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_close(cx)
    }
}
