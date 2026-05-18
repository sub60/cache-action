use core::pin::Pin;
use core::task::{Context, Poll, ready};
use std::io;

use futures::AsyncRead;

pub(crate) trait AsyncReadExt: AsyncRead {
    /// Creates a future which will try to fill the given buffer, returning the
    /// number of bytes that have been read into it.
    fn try_fill<'a>(
        self: Pin<&'a mut Self>,
        buf: &'a mut [u8],
    ) -> TryFill<'a, Self> {
        TryFill { reader: self, buf, num_read: 0 }
    }
}

impl<R: AsyncRead + ?Sized> AsyncReadExt for R {}

pub(crate) struct TryFill<'a, R: ?Sized> {
    reader: Pin<&'a mut R>,
    buf: &'a mut [u8],
    num_read: usize,
}

impl<R: AsyncRead + ?Sized> Future for TryFill<'_, R> {
    type Output = io::Result<usize>;

    #[inline]
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        while this.num_read < this.buf.len() {
            let n = ready!(
                this.reader
                    .as_mut()
                    .poll_read(ctx, &mut this.buf[this.num_read..])
            )?;

            if n == 0 {
                break;
            }

            this.num_read += n;
        }

        Poll::Ready(Ok(this.num_read))
    }
}
