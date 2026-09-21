use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
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
    TruncatedLengthPrefix { bytes_read: usize },
    NegativeLength(i32),
    EmptyFrame,
    FrameTooLarge { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
    OutboundLengthOverflow { requested: usize },
    TruncatedFrame { expected: usize, received: usize },
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLengthVarInt => write!(f, "invalid frame length VarInt"),
            Self::TruncatedLengthPrefix { bytes_read } => {
                write!(f, "connection closed after {bytes_read} frame length bytes")
            }
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
            Self::TruncatedFrame { expected, received } => {
                write!(
                    f,
                    "connection closed after {received} of {expected} frame bytes"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug)]
pub enum SocketError {
    PeerClosed,
    Io(std::io::Error),
    Frame(FrameError),
    Decode(DecodeError),
}

impl std::fmt::Display for SocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PeerClosed => write!(f, "peer closed the connection"),
            Self::Io(error) => error.fmt(f),
            Self::Frame(error) => error.fmt(f),
            Self::Decode(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SocketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PeerClosed => None,
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

pub struct ClientSocket {
    stream: TcpStream,
    peer_addr: SocketAddr,
    limits: FrameLimits,
}

impl ClientSocket {
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub fn into_split(self) -> (ClientReader, ClientWriter) {
        let (reader, writer) = self.stream.into_split();
        (
            ClientReader {
                stream: reader,
                limits: self.limits,
            },
            ClientWriter {
                stream: BufWriter::new(writer),
                limits: self.limits,
                frame: Vec::new(),
                header: Vec::with_capacity(FRAME_HEADER_BYTES),
            },
        )
    }
}

pub struct ClientReader {
    stream: OwnedReadHalf,
    limits: FrameLimits,
}

impl ClientReader {
    pub async fn receive(&mut self) -> Result<Packet, SocketError> {
        let mut len_buf = [0u8; 5];
        let mut bytes_read = 0usize;
        loop {
            if self
                .stream
                .read(&mut len_buf[bytes_read..bytes_read + 1])
                .await?
                == 0
            {
                return if bytes_read == 0 {
                    Err(SocketError::PeerClosed)
                } else {
                    Err(FrameError::TruncatedLengthPrefix { bytes_read }.into())
                };
            }
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
        if len > self.limits.max_inbound_frame_bytes {
            return Err(FrameError::FrameTooLarge {
                requested: len,
                limit: self.limits.max_inbound_frame_bytes,
            }
            .into());
        }

        let mut packet_buf = Vec::new();
        packet_buf
            .try_reserve_exact(len)
            .map_err(|_| FrameError::AllocationFailed { requested: len })?;
        packet_buf.resize(len, 0);
        let mut received = 0;
        while received < len {
            let bytes_read = self.stream.read(&mut packet_buf[received..]).await?;
            if bytes_read == 0 {
                return Err(FrameError::TruncatedFrame {
                    expected: len,
                    received,
                }
                .into());
            }
            received += bytes_read;
        }
        Ok(Packet {
            bytes: packet_buf,
            decode_limits: self.limits.decode,
        })
    }
}

const FRAME_HEADER_BYTES: usize = 5;
const FRAME_BUFFER_RETAINED_BYTES: usize = 64 * 1024;

pub struct ClientWriter {
    stream: BufWriter<OwnedWriteHalf>,
    limits: FrameLimits,
    frame: Vec<u8>,
    header: Vec<u8>,
}

impl ClientWriter {
    pub async fn send<T: Encode>(&mut self, packet: &T) -> Result<(), SocketError> {
        self.frame.clear();
        self.frame.resize(FRAME_HEADER_BYTES, 0);
        packet.encode(&mut self.frame);
        let body_len = self.frame.len() - FRAME_HEADER_BYTES;
        let result = self.write_frame(body_len).await;
        if self.frame.capacity() > FRAME_BUFFER_RETAINED_BYTES {
            self.frame.clear();
            self.frame.shrink_to(FRAME_BUFFER_RETAINED_BYTES);
        }
        result
    }

    async fn write_frame(&mut self, body_len: usize) -> Result<(), SocketError> {
        if body_len > self.limits.max_outbound_frame_bytes {
            return Err(FrameError::FrameTooLarge {
                requested: body_len,
                limit: self.limits.max_outbound_frame_bytes,
            }
            .into());
        }
        let len = i32::try_from(body_len).map_err(|_| FrameError::OutboundLengthOverflow {
            requested: body_len,
        })?;
        self.header.clear();
        VarI32(len).encode(&mut self.header);
        let start = FRAME_HEADER_BYTES - self.header.len();
        self.frame[start..FRAME_HEADER_BYTES].copy_from_slice(&self.header);
        self.stream.write_all(&self.frame[start..]).await?;
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<(), SocketError> {
        self.stream.flush().await?;
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
        stream.set_nodelay(true)?;
        Ok(ClientSocket {
            stream,
            peer_addr: addr,
            limits: self.limits,
        })
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
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        let mut prefix = Vec::new();
        VarI32(-1).encode(&mut prefix);
        peer.write_all(&prefix).await.unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::NegativeLength(-1)))
        ));
    }

    #[tokio::test]
    async fn rejects_zero_and_overlong_frame_lengths() {
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        peer.write_all(&[0]).await.unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::EmptyFrame))
        ));

        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        peer.write_all(&[0x80; 5]).await.unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::InvalidLengthVarInt))
        ));
    }

    #[tokio::test]
    async fn rejects_frame_above_configured_limit() {
        let limits = FrameLimits {
            max_inbound_frame_bytes: 8,
            ..FrameLimits::default()
        };
        let (socket, mut peer) = connected(limits).await;
        let (mut reader, _) = socket.into_split();
        let mut prefix = Vec::new();
        VarI32(9).encode(&mut prefix);
        peer.write_all(&prefix).await.unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                requested: 9,
                limit: 8
            }))
        ));

        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        let mut prefix = Vec::new();
        VarI32(i32::MAX).encode(&mut prefix);
        peer.write_all(&prefix).await.unwrap();
        assert!(matches!(
            reader.receive().await,
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
        let (socket, mut peer) = connected(limits).await;
        let (mut reader, _) = socket.into_split();
        peer.write_all(&[1, 42]).await.unwrap();
        let packet = reader.receive().await.unwrap();
        assert_eq!(packet.decode::<u8>(), Ok(42));
    }

    #[tokio::test]
    async fn distinguishes_eof_while_reading_frames() {
        let (socket, peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        drop(peer);
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::PeerClosed)
        ));

        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        peer.write_all(&[0x80]).await.unwrap();
        peer.shutdown().await.unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::TruncatedLengthPrefix {
                bytes_read: 1
            }))
        ));

        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (mut reader, _) = socket.into_split();
        peer.write_all(&[2, 0]).await.unwrap();
        peer.shutdown().await.unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::TruncatedFrame {
                expected: 2,
                received: 1
            }))
        ));
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
        let (socket, _peer) = connected(limits).await;
        let (_, mut writer) = socket.into_split();
        assert!(matches!(
            writer.send(&Bytes(vec![0; 5])).await,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                requested: 5,
                limit: 4
            }))
        ));
    }

    fn frame_bytes(payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        VarI32(payload.len() as i32).encode(&mut frame);
        frame.extend_from_slice(payload);
        frame
    }

    #[tokio::test]
    async fn batched_sends_match_individual_frames_on_the_wire() {
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (_, mut writer) = socket.into_split();

        let payloads: [Vec<u8>; 9] = [
            vec![1, 2, 3],
            vec![],
            vec![7; 200],
            vec![42],
            vec![9; 8 * 1024],
            vec![5; 8 * 1024 - 100],
            vec![8; 200],
            vec![3; 100_000],
            vec![6],
        ];

        let mut expected = Vec::new();
        for payload in &payloads {
            expected.extend_from_slice(&frame_bytes(payload));
        }

        for payload in &payloads {
            writer.send(&Bytes(payload.clone())).await.unwrap();
        }
        writer.flush().await.unwrap();
        drop(writer);

        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, expected);
    }

    #[tokio::test]
    async fn frame_buffer_is_released_after_an_oversized_frame() {
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (_, mut writer) = socket.into_split();

        let large = vec![1; 4 * FRAME_BUFFER_RETAINED_BYTES];
        writer.send(&Bytes(large.clone())).await.unwrap();
        assert!(writer.frame.capacity() <= FRAME_BUFFER_RETAINED_BYTES);

        writer.send(&Bytes(vec![2, 3])).await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);

        let mut expected = frame_bytes(&large);
        expected.extend_from_slice(&frame_bytes(&[2, 3]));
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, expected);
    }

    #[tokio::test]
    async fn rejected_frame_leaves_later_frames_intact() {
        let limits = FrameLimits {
            max_outbound_frame_bytes: 4,
            ..FrameLimits::default()
        };
        let (socket, mut peer) = connected(limits).await;
        let (_, mut writer) = socket.into_split();

        writer.send(&Bytes(vec![1, 2])).await.unwrap();
        writer.send(&Bytes(vec![0; 5])).await.unwrap_err();
        writer.send(&Bytes(vec![3, 4, 5, 6])).await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);

        let mut expected = frame_bytes(&[1, 2]);
        expected.extend_from_slice(&frame_bytes(&[3, 4, 5, 6]));
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, expected);
    }
}
