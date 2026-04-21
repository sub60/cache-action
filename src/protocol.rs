//! The wire protocol spoken on the Unix domain socket between the daemon and
//! the `push`/`drain` subcommands.
//!
//! Store paths are separated by newlines, and a null byte signals the daemon to
//! drain and shut down.
//!
//! ```text
//! push:  /nix/store/abc-foo\n/nix/store/def-bar\n
//! drain: \0
//! ```

use core::ops::Range;
use core::pin::Pin;
use core::task::{Context, Poll, ready};
use core::{fmt, str};
use std::io;

use futures::{AsyncRead, AsyncWrite};
use nix_types::{NixStoreLiteral, NixStorePath};
use smol_str::SmolStr;

/// The store paths separator in the wire format.
const PATH_SEPARATOR: u8 = b'\n';

/// The sentinal byte which signals the daemon to drain pending uploads and shut
/// down.
const DRAIN_SENTINEL: u8 = b'\0';

pin_project_lite::pin_project! {
    pub(crate) struct Sender<Writer> {
        buf: Vec<u8>,
        // The number of bytes in [`buf`](Sender::buf) we've already written to
        // the underlying writer.
        num_written: usize,
        #[pin]
        writer: Writer,
    }
}

pin_project_lite::pin_project! {
    pub(crate) struct Receiver<Reader> {
        is_done: bool,
        // The buffer bytes are read into.
        buf: Box<[u8; 1024]>,
        // The range of bytes in [`buf`](Receiver::buf) we still have to handle.
        unhandled_range: Range<usize>,
        #[pin]
        reader: Reader,
    }
}

#[derive(Debug)]
pub(crate) enum Message {
    PushStorePath(NixStorePath<StoreDir>),
    DrainDaemon,
}

#[derive(Clone)]
pub enum StoreDir {
    /// The `/nix/store` directory.
    NixStore,
    /// Any other store directory, guaranteed to not contain the `NUL` byte.
    Other(SmolStr),
}

#[derive(Debug)]
pub(crate) enum ReceiveError {
    Io(io::Error),
    InvalidStorePath(<NixStorePath<StoreDir> as str::FromStr>::Err),
}

impl<Writer> Sender<Writer> {
    pub(crate) fn new(writer: Writer) -> Self {
        Self { writer, buf: Vec::new(), num_written: 0 }
    }
}

impl<Writer: AsyncWrite> Sender<Writer> {
    /// Writes all buffered bytes to the underlying writer and clears the
    /// buffer.
    fn drain_buf(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let mut this = self.project();

        while *this.num_written < this.buf.len() {
            let num_written = ready!(
                this.writer
                    .as_mut()
                    .poll_write(ctx, &this.buf[*this.num_written..])
            )?;

            if num_written == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "write returned 0",
                )));
            }

            *this.num_written += num_written;
        }

        this.buf.clear();
        *this.num_written = 0;

        Poll::Ready(Ok(()))
    }
}

impl<Writer: AsyncWrite> futures::Sink<Message> for Sender<Writer> {
    type Error = io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        _ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        // Messages are buffered in memory and written to the underlying writer
        // on flush/close, so we're always ready
        Poll::Ready(Ok(()))
    }

    fn start_send(
        self: Pin<&mut Self>,
        message: Message,
    ) -> Result<(), Self::Error> {
        use std::io::Write;

        let buf = self.project().buf;

        match message {
            Message::PushStorePath(store_path) => {
                write!(buf, "{store_path}")
                    .expect("writing to a Vec never fails");
                buf.push(PATH_SEPARATOR);
            },
            Message::DrainDaemon => {
                buf.push(DRAIN_SENTINEL);
            },
        }

        Ok(())
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().drain_buf(ctx))?;
        self.project().writer.poll_flush(ctx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().drain_buf(ctx))?;
        self.project().writer.poll_close(ctx)
    }
}

impl<Reader> Receiver<Reader> {
    pub(crate) fn new(reader: Reader) -> Self {
        Self {
            is_done: false,
            reader,
            buf: Box::new([0u8; _]),
            unhandled_range: 0..0,
        }
    }
}

impl<Reader: AsyncRead> futures::Stream for Receiver<Reader> {
    type Item = Result<Message, ReceiveError>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.is_done {
            return Poll::Ready(None);
        }

        let mut this = self.project();

        let res = loop {
            let bytes = &this.buf[this.unhandled_range.clone()];

            if let Some(&DRAIN_SENTINEL) = bytes.first() {
                break Ok(Message::DrainDaemon);
            }

            let Some(separator_offset) = memchr::memchr(PATH_SEPARATOR, bytes)
            else {
                let bytes_len = bytes.len();
                this.buf.copy_within(this.unhandled_range.clone(), 0);
                *this.unhandled_range = 0..bytes_len;
                let read_buf = &mut this.buf[this.unhandled_range.end..];
                match ready!(this.reader.as_mut().poll_read(ctx, read_buf)) {
                    Ok(0) => return Poll::Ready(None),
                    Ok(num_read) => this.unhandled_range.end += num_read,
                    Err(err) => break Err(ReceiveError::Io(err)),
                }
                continue;
            };

            let store_path_bytes = &bytes[..separator_offset];

            this.unhandled_range.start += separator_offset + 1;

            let store_path_str = match str::from_utf8(store_path_bytes) {
                Ok(s) => s,
                Err(err) => {
                    break Err(ReceiveError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        err,
                    )));
                },
            };

            break match store_path_str.parse() {
                Ok(store_path) => Ok(Message::PushStorePath(store_path)),
                Err(err) => Err(ReceiveError::InvalidStorePath(err)),
            };
        };

        *this.is_done = matches!(res, Ok(Message::DrainDaemon) | Err(_));

        Poll::Ready(Some(res))
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
    type Err = core::ffi::FromBytesWithNulError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == NixStoreLiteral::STR {
            Ok(Self::NixStore)
        } else if let Some(position) = memchr::memchr(0, s.as_bytes()) {
            Err(core::ffi::FromBytesWithNulError::InteriorNul { position })
        } else {
            Ok(Self::Other(s.into()))
        }
    }
}

impl fmt::Display for ReceiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::InvalidStorePath(error) => {
                write!(f, "invalid store path: {error}")
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use core::pin::pin;

    use futures::io::Cursor;
    use futures::{SinkExt, StreamExt};

    use super::*;

    #[test]
    fn send_single_store_path() {
        let bytes = send_messages(vec![Message::PushStorePath(path(
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-hello",
        ))]);

        assert_eq!(
            bytes,
            b"/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-hello\n",
        );
    }

    #[test]
    fn send_drain() {
        assert_eq!(send_messages(vec![Message::DrainDaemon]), b"\0");
    }

    #[test]
    fn send_paths_then_drain() {
        let bytes = send_messages(vec![
            Message::PushStorePath(path(
                "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-foo",
            )),
            Message::PushStorePath(path(
                "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-bar",
            )),
            Message::DrainDaemon,
        ]);

        assert_eq!(
            bytes,
            b"/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-foo\n\
              /nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-bar\n\
              \0",
        );
    }

    #[test]
    fn receive_single_store_path() {
        let msgs = parse_messages(
            b"/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-hello\n",
        );
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            Ok(Message::PushStorePath(p)) => assert_eq!(
                p.to_string(),
                "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-hello",
            ),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn receive_multiple_store_paths() {
        let msgs = parse_messages(
            b"/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-foo\n\
              /nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-bar\n",
        );
        assert_eq!(msgs.len(), 2);
        assert!(matches!(&msgs[0], Ok(Message::PushStorePath(_))));
        assert!(matches!(&msgs[1], Ok(Message::PushStorePath(_))));
    }

    #[test]
    fn receive_drain() {
        let msgs = parse_messages(b"\0");
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], Ok(Message::DrainDaemon)));
    }

    #[test]
    fn receive_paths_then_drain() {
        let msgs = parse_messages(
            b"/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-foo\n\0",
        );
        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[0], Ok(Message::PushStorePath(_))));
        assert!(matches!(msgs[1], Ok(Message::DrainDaemon)));
    }

    #[test]
    fn receive_empty_input() {
        assert!(parse_messages(b"").is_empty());
    }

    #[test]
    fn receive_invalid_store_path() {
        let msgs = parse_messages(b"not-a-store-path\n");
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], Err(ReceiveError::InvalidStorePath(_))));
    }

    fn parse_messages(input: &[u8]) -> Vec<Result<Message, ReceiveError>> {
        futures::executor::block_on(async {
            let receiver = Receiver::new(Cursor::new(input.to_vec()));
            futures::pin_mut!(receiver);
            let mut results = Vec::new();
            while let Some(item) = receiver.next().await {
                results.push(item);
            }
            results
        })
    }

    fn send_messages(messages: Vec<Message>) -> Vec<u8> {
        futures::executor::block_on(async {
            let buf = Cursor::new(Vec::new());
            let mut sender = pin!(Sender::new(buf));
            for msg in messages {
                sender.as_mut().feed(msg).await.unwrap();
            }
            sender.as_mut().close().await.unwrap();
            sender.project().writer.get_mut().clone().into_inner()
        })
    }

    fn path(s: &str) -> NixStorePath<StoreDir> {
        s.parse().unwrap()
    }
}
