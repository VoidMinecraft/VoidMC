//! One end of a relay: the frames read from a socket, and the bytes queued for it.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Instant, sleep, timeout};

use crate::frame::{FrameCodec, FrameCodecError};

const READ_CHUNK: usize = 64 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const NO_BUFFER_PAUSE: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub(super) enum PeerError {
    Io(io::Error),
    Frame(FrameCodecError),
}

impl std::fmt::Display for PeerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Frame(error) => error.fmt(f),
        }
    }
}

/// A socket written without blocking, and told when it can take more.
pub(super) trait TryWrite {
    fn try_write(&self, bytes: &[u8]) -> io::Result<usize>;
    fn writable(&self) -> impl Future<Output = io::Result<()>>;
}

impl TryWrite for TcpStream {
    fn try_write(&self, bytes: &[u8]) -> io::Result<usize> {
        TcpStream::try_write(self, bytes)
    }

    fn writable(&self) -> impl Future<Output = io::Result<()>> {
        TcpStream::writable(self)
    }
}

/// Why a write did not go through now and may later.
enum Wait {
    Writable,
    /// `ENOBUFS`: the kernel has no buffer for the write yet.
    Buffers,
}

impl Wait {
    fn after(error: &io::Error) -> Option<Self> {
        if error.kind() == io::ErrorKind::WouldBlock {
            Some(Self::Writable)
        } else if error.raw_os_error() == Some(libc::ENOBUFS) {
            Some(Self::Buffers)
        } else {
            None
        }
    }
}

pub(super) struct Peer<S = TcpStream> {
    pub(super) stream: S,
    pub(super) codec: FrameCodec,
    pub(super) inbound: BytesMut,
    pub(super) outbound: Vec<u8>,
    stalled_for: Duration,
}

impl<S> Peer<S> {
    pub(super) fn new(stream: S) -> Self {
        Self {
            stream,
            codec: FrameCodec::default(),
            inbound: BytesMut::with_capacity(READ_CHUNK),
            outbound: Vec::new(),
            stalled_for: Duration::ZERO,
        }
    }

    pub(super) fn buffered(&mut self) -> Result<Option<Bytes>, FrameCodecError> {
        self.codec.decode(&mut self.inbound)
    }

    pub(super) fn queue(&mut self, frame: &[u8]) {
        self.codec.encode(frame, &mut self.outbound);
    }
}

impl Peer {
    /// `None` when the peer closed. Safe to cancel: bytes only ever land in `inbound`.
    pub(super) async fn next(&mut self) -> Result<Option<Bytes>, PeerError> {
        loop {
            if let Some(frame) = self.buffered().map_err(PeerError::Frame)? {
                return Ok(Some(frame));
            }
            self.inbound.reserve(READ_CHUNK);
            let read = self
                .stream
                .read_buf(&mut self.inbound)
                .await
                .map_err(PeerError::Io)?;
            if read == 0 {
                return Ok(None);
            }
        }
    }
}

impl<S: TryWrite> Peer<S> {
    pub(super) async fn flush(&mut self) -> io::Result<()> {
        self.flush_within(WRITE_TIMEOUT).await
    }

    /// Fails once the writes to the peer have waited `limit` in all: a peer that takes a few
    /// bytes now and then is still stuck. Only a queue written whole without waiting starts
    /// the count again.
    async fn flush_within(&mut self, limit: Duration) -> io::Result<()> {
        let mut written = 0;
        let mut waited = false;
        while written < self.outbound.len() {
            match self.stream.try_write(&self.outbound[written..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => written += count,
                Err(error) => {
                    let Some(wait) = Wait::after(&error) else {
                        return Err(error);
                    };
                    self.wait(wait, limit).await?;
                    waited = true;
                }
            }
        }
        if written > 0 {
            self.outbound.clear();
            if !waited {
                self.stalled_for = Duration::ZERO;
            }
        }
        Ok(())
    }

    async fn wait(&mut self, wait: Wait, limit: Duration) -> io::Result<()> {
        let started = Instant::now();
        let stream = &self.stream;
        let ready = timeout(limit.saturating_sub(self.stalled_for), async {
            match wait {
                Wait::Writable => stream.writable().await,
                Wait::Buffers => {
                    sleep(NO_BUFFER_PAUSE).await;
                    Ok(())
                }
            }
        })
        .await;
        self.stalled_for += started.elapsed();
        ready.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
}

pub(super) async fn next_of(peer: &mut Option<Peer>) -> Result<Option<Bytes>, PeerError> {
    match peer {
        Some(peer) => peer.next().await,
        None => std::future::pending().await,
    }
}

pub(super) async fn connect(address: SocketAddr) -> Option<TcpStream> {
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .ok()?
        .ok()?;
    stream.set_nodelay(true).ok()?;
    Some(stream)
}

#[cfg(test)]
pub(super) async fn pair() -> (TcpStream, TcpStream) {
    let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    let near = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (far, _) = listener.accept().await.unwrap();
    (near, far)
}

#[cfg(test)]
mod tests;
