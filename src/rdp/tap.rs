//! A look at the bytes on the wire, for when a server sends something the
//! engine can't read: with `MOTE_LOG=mote::wire=trace`, the start of every
//! read and write after TLS goes to the log. Credentials are inside it, so
//! it stays off unless asked for.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// How much of each read or write is shown.
const SHOWN: usize = 64;

pub(super) struct Tap<S> {
    inner: S,
}

impl<S> Tap<S> {
    pub(super) fn new(inner: S) -> Self {
        Self { inner }
    }
}

fn show(direction: &str, bytes: &[u8]) {
    if bytes.is_empty() || !tracing::enabled!(target: "mote::wire", tracing::Level::TRACE) {
        return;
    }
    let hex: String = bytes
        .iter()
        .take(SHOWN)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    tracing::trace!(target: "mote::wire", "{direction} {} bytes: {hex}", bytes.len());
}

impl<S: AsyncRead + Unpin> AsyncRead for Tap<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = poll {
            show("read", &buf.filled()[before..]);
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Tap<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, data);
        if let Poll::Ready(Ok(written)) = poll {
            show("wrote", &data[..written]);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
