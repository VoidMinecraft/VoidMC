use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use voidmc_codec::{Decode, DecodeError, DecodeLimits, Decoder, Encode, VarI32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimits {
    pub max_inbound_frame_bytes: usize,
    pub max_outbound_frame_bytes: usize,
    pub decode: DecodeLimits,
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_inbound_frame_bytes: 2 * 1024 * 1024,
            max_outbound_frame_bytes: 8 * 1024 * 1024,
            decode: DecodeLimits::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    InvalidLengthVarInt,
    NegativeLength(i32),
    EmptyFrame,
    FrameTooLarge { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
    OutboundLengthOverflow { requested: usize },
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLengthVarInt => write!(f, "invalid frame length VarInt"),
            Self::NegativeLength(value) => write!(f, "negative frame length {value}"),
            Self::EmptyFrame => write!(f, "empty packet frame"),
            Self::FrameTooLarge { requested, limit } => {
                write!(f, "frame length {requested} exceeds limit {limit}")
            }
            Self::AllocationFailed { requested } => {
                write!(f, "failed to allocate {requested} bytes for packet frame")
            }
            Self::OutboundLengthOverflow { requested } => {
                write!(
                    f,
                    "outbound frame length {requested} does not fit in a VarInt"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug)]
pub enum SocketError {
    Io(std::io::Error),
    Frame(FrameError),
    Decode(DecodeError),
}

impl std::fmt::Display for SocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Frame(error) => error.fmt(f),
            Self::Decode(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SocketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::Decode(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for SocketError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<FrameError> for SocketError {
    fn from(value: FrameError) -> Self {
        Self::Frame(value)
    }
}

impl From<DecodeError> for SocketError {
    fn from(value: DecodeError) -> Self {
        Self::Decode(value)
    }
}

pub struct Packet {
    bytes: Vec<u8>,
    decode_limits: DecodeLimits,
}

impl Packet {
    pub fn decode<T: Decode>(&self) -> Result<T, DecodeError> {
        let mut decoder = Decoder::new(&self.bytes, self.decode_limits);
        decoder.decode_exact::<T>()
    }
}

pub struct ClientSocket(TcpStream, pub SocketAddr, FrameLimits);

impl ClientSocket {
    pub async fn receive(&mut self) -> Result<Packet, SocketError> {
        let mut len_buf = [0u8; 5];
        let mut bytes_read = 0usize;
        loop {
            self.0
                .read_exact(&mut len_buf[bytes_read..bytes_read + 1])
                .await?;
            let byte = len_buf[bytes_read];
            bytes_read += 1;
            if byte & 0x80 == 0 {
                break;
            }
            if bytes_read == len_buf.len() {
                return Err(FrameError::InvalidLengthVarInt.into());
            }
        }

        let mut prefix = &len_buf[..bytes_read];
        let signed_len = VarI32::decode(&mut prefix)
            .map_err(|_| FrameError::InvalidLengthVarInt)?
            .0;
        if signed_len < 0 {
            return Err(FrameError::NegativeLength(signed_len).into());
        }
        if signed_len == 0 {
            return Err(FrameError::EmptyFrame.into());
        }
        let len =
            usize::try_from(signed_len).map_err(|_| FrameError::NegativeLength(signed_len))?;
        if len > self.2.max_inbound_frame_bytes {
            return Err(FrameError::FrameTooLarge {
                requested: len,
                limit: self.2.max_inbound_frame_bytes,
            }
            .into());
        }

        let mut packet_buf = Vec::new();
        packet_buf
            .try_reserve_exact(len)
            .map_err(|_| FrameError::AllocationFailed { requested: len })?;
        packet_buf.resize(len, 0);
        self.0.read_exact(&mut packet_buf).await?;
        Ok(Packet {
            bytes: packet_buf,
            decode_limits: self.2.decode,
        })
    }

    pub async fn send<T: Encode>(&mut self, packet: &T) -> Result<(), SocketError> {
        let mut packet_buf = Vec::new();
        packet.encode(&mut packet_buf);
        if packet_buf.len() > self.2.max_outbound_frame_bytes {
            return Err(FrameError::FrameTooLarge {
                requested: packet_buf.len(),
                limit: self.2.max_outbound_frame_bytes,
            }
            .into());
        }
        let len =
            i32::try_from(packet_buf.len()).map_err(|_| FrameError::OutboundLengthOverflow {
                requested: packet_buf.len(),
            })?;
        let mut len_buf = Vec::with_capacity(5);
        VarI32(len).encode(&mut len_buf);
        self.0.write_all(&len_buf).await?;
        self.0.write_all(&packet_buf).await?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct ServerSocket {
    pub listener: TcpListener,
    limits: FrameLimits,
}

impl ServerSocket {
    pub fn new(listener: TcpListener, limits: FrameLimits) -> Self {
        Self { listener, limits }
    }

    pub async fn accept(&self) -> std::io::Result<ClientSocket> {
        let (stream, addr) = self.listener.accept().await?;
        Ok(ClientSocket(stream, addr, self.limits))
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn connected(limits: FrameLimits) -> (ClientSocket, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = TcpStream::connect(address).await.unwrap();
        let socket = ServerSocket::new(listener, limits).accept().await.unwrap();
        (socket, peer)
    }

    #[tokio::test]
    async fn rejects_negative_frame_length_before_allocation() {
        let (mut socket, mut peer) = connected(FrameLimits::default()).await;
        let mut prefix = Vec::new();
        VarI32(-1).encode(&mut prefix);
        peer.write_all(&prefix).await.unwrap();
        assert!(matches!(
            socket.receive().await,
            Err(SocketError::Frame(FrameError::NegativeLength(-1)))
        ));
    }

    #[tokio::test]
    async fn rejects_zero_and_overlong_frame_lengths() {
        let (mut socket, mut peer) = connected(FrameLimits::default()).await;
        peer.write_all(&[0]).await.unwrap();
        assert!(matches!(
            socket.receive().await,
            Err(SocketError::Frame(FrameError::EmptyFrame))
        ));

        let (mut socket, mut peer) = connected(FrameLimits::default()).await;
        peer.write_all(&[0x80; 5]).await.unwrap();
        assert!(matches!(
            socket.receive().await,
            Err(SocketError::Frame(FrameError::InvalidLengthVarInt))
        ));
    }

    #[tokio::test]
    async fn rejects_frame_above_configured_limit() {
        let limits = FrameLimits {
            max_inbound_frame_bytes: 8,
            ..FrameLimits::default()
        };
        let (mut socket, mut peer) = connected(limits).await;
        let mut prefix = Vec::new();
        VarI32(9).encode(&mut prefix);
        peer.write_all(&prefix).await.unwrap();
        assert!(matches!(
            socket.receive().await,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                requested: 9,
                limit: 8
            }))
        ));

        let (mut socket, mut peer) = connected(FrameLimits::default()).await;
        let mut prefix = Vec::new();
        VarI32(i32::MAX).encode(&mut prefix);
        peer.write_all(&prefix).await.unwrap();
        assert!(matches!(
            socket.receive().await,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                requested,
                ..
            })) if requested == i32::MAX as usize
        ));
    }

    #[tokio::test]
    async fn accepts_frame_at_configured_limit() {
        let limits = FrameLimits {
            max_inbound_frame_bytes: 1,
            ..FrameLimits::default()
        };
        let (mut socket, mut peer) = connected(limits).await;
        peer.write_all(&[1, 42]).await.unwrap();
        let packet = socket.receive().await.unwrap();
        assert_eq!(packet.decode::<u8>(), Ok(42));
    }

    #[tokio::test]
    async fn rejects_truncated_frame() {
        let (mut socket, mut peer) = connected(FrameLimits::default()).await;
        peer.write_all(&[2, 0]).await.unwrap();
        peer.shutdown().await.unwrap();
        assert!(matches!(socket.receive().await, Err(SocketError::Io(_))));
    }

    #[test]
    fn exact_packet_decode_rejects_trailing_data() {
        let packet = Packet {
            bytes: vec![42, 99],
            decode_limits: DecodeLimits::default(),
        };
        assert_eq!(
            packet.decode::<u8>(),
            Err(DecodeError::TrailingData { remaining: 1 })
        );
    }

    struct Bytes(Vec<u8>);

    impl Encode for Bytes {
        fn encode(&self, buf: &mut Vec<u8>) {
            buf.extend_from_slice(&self.0);
        }
    }

    #[tokio::test]
    async fn rejects_oversized_outbound_frame() {
        let limits = FrameLimits {
            max_outbound_frame_bytes: 4,
            ..FrameLimits::default()
        };
        let (mut socket, _peer) = connected(limits).await;
        assert!(matches!(
            socket.send(&Bytes(vec![0; 5])).await,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                requested: 5,
                limit: 4
            }))
        ));
    }
}
