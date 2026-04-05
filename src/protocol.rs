//! TODO: docs.

use core::convert::Infallible;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::{fmt, str};
use std::io;

use futures::{AsyncRead, AsyncWrite};
use nix_types::{NixStoreLiteral, NixStorePath};
use smol_str::SmolStr;

pub(crate) struct Sender<Writer> {
    writer: Writer,
}

pub(crate) struct Receiver<Reader> {
    reader: Reader,
}

pub(crate) enum Message {
    PushStorePath(NixStorePath<StoreDir>),
    DrainDaemon,
}

#[derive(Clone)]
pub enum StoreDir {
    NixStore,
    Other(SmolStr),
}

pub(crate) enum ReceiveError {
    Io(io::Error),
    InvalidSeparator(u8),
    InvalidStorePath(<NixStorePath<StoreDir> as str::FromStr>::Err),
}

impl<Writer> Sender<Writer> {
    pub(crate) fn new(writer: Writer) -> Self {
        Self { writer }
    }
}

impl<Reader> Receiver<Reader> {
    pub(crate) fn new(reader: Reader) -> Self {
        Self { reader }
    }
}

impl<Writer: AsyncWrite> futures::Sink<Message> for Sender<Writer> {
    type Error = io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        _ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn start_send(
        self: Pin<&mut Self>,
        _message: Message,
    ) -> Result<(), Self::Error> {
        todo!()
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}

impl<Reader: AsyncRead> futures::Stream for Receiver<Reader> {
    type Item = Result<Message, ReceiveError>;

    fn poll_next(
        self: Pin<&mut Self>,
        _ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

impl fmt::Display for StoreDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreDir::NixStore => NixStoreLiteral.fmt(f),
            StoreDir::Other(smol_str) => smol_str.fmt(f),
        }
    }
}

impl str::FromStr for StoreDir {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            NixStoreLiteral::STR => Self::NixStore,
            other => Self::Other(other.into()),
        })
    }
}
