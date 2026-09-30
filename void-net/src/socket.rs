use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use voidmc_codec::{Decode, DecodeError, DecodeLimits, Decoder, Encode, VarI32};

/// Largest uncompressed packet a compressed frame may declare, as in vanilla.
pub const MAX_DECOMPRESSED_BYTES: usize = 1 << 23;

/// Vanilla's default `network-compression-threshold`.
pub const DEFAULT_COMPRESSION_THRESHOLD: u32 = 256;

const COMPRESSION_LEVEL: u32 = 4;

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
    InvalidDataLength,
    CompressedBelowThreshold { data_length: usize, threshold: u32 },
    DecompressedTooLarge { requested: usize, limit: usize },
    DecompressedLengthMismatch { declared: usize },
    CorruptCompressedData,
    CompressionFailed,
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
            Self::InvalidDataLength => write!(f, "invalid compressed frame data length"),
            Self::CompressedBelowThreshold {
                data_length,
                threshold,
            } => write!(
                f,
                "compressed packet of {data_length} bytes is below threshold {threshold}"
            ),
            Self::DecompressedTooLarge { requested, limit } => {
                write!(
                    f,
                    "decompressed packet length {requested} exceeds limit {limit}"
                )
            }
            Self::DecompressedLengthMismatch { declared } => {
                write!(
                    f,
                    "decompressed packet does not match declared length {declared}"
                )
            }
            Self::CorruptCompressedData => write!(f, "corrupt compressed packet data"),
            Self::CompressionFailed => write!(f, "failed to compress outbound packet"),
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
    compression_threshold: Option<u32>,
}

impl ClientSocket {
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub fn into_split(self) -> (ClientReader, ClientWriter) {
        let (reader, writer) = self.stream.into_split();
        let enabled = Arc::new(AtomicBool::new(false));
        (
            ClientReader {
                stream: BufReader::with_capacity(READ_BUFFER_BYTES, reader),
                limits: self.limits,
                compression: self
                    .compression_threshold
                    .map(|threshold| InboundCompression {
                        threshold,
                        enabled: Arc::clone(&enabled),
                        frame: Vec::new(),
                        retained_bytes: self.limits.max_inbound_frame_bytes
                            / FRAME_BUFFER_RETAINED_DIVISOR,
                        inflater: Decompress::new(true),
                    }),
            },
            ClientWriter {
                stream: BufWriter::new(writer),
                limits: self.limits,
                frame: Vec::new(),
                retained_bytes: self.limits.max_outbound_frame_bytes
                    / FRAME_BUFFER_RETAINED_DIVISOR,
                compression: self
                    .compression_threshold
                    .map(|threshold| OutboundCompression {
                        threshold,
                        enabled,
                        active: false,
                        deflater: Compress::new(Compression::new(COMPRESSION_LEVEL), true),
                        frame: Vec::new(),
                    }),
            },
        )
    }
}

const READ_BUFFER_BYTES: usize = 8 * 1024;

struct InboundCompression {
    threshold: u32,
    enabled: Arc<AtomicBool>,
    frame: Vec<u8>,
    retained_bytes: usize,
    inflater: Decompress,
}

pub struct ClientReader {
    stream: BufReader<OwnedReadHalf>,
    limits: FrameLimits,
    compression: Option<InboundCompression>,
}

impl ClientReader {
    pub async fn receive(&mut self) -> Result<Packet, SocketError> {
        let len = self.read_frame_length().await?;
        let compression = self
            .compression
            .as_mut()
            .filter(|compression| compression.enabled.load(Ordering::Acquire));
        let bytes = match compression {
            None => {
                let mut packet_buf = Vec::new();
                read_frame_body(&mut self.stream, &mut packet_buf, len).await?;
                packet_buf
            }
            Some(compression) => {
                let result = read_frame_body(&mut self.stream, &mut compression.frame, len)
                    .await
                    .and_then(|()| {
                        Ok(decompress_frame(
                            &compression.frame,
                            compression.threshold,
                            self.limits
                                .max_inbound_frame_bytes
                                .min(MAX_DECOMPRESSED_BYTES),
                            &mut compression.inflater,
                        )?)
                    });
                if compression.frame.capacity() > compression.retained_bytes {
                    compression.frame.clear();
                    compression.frame.shrink_to(compression.retained_bytes);
                }
                result?
            }
        };
        Ok(Packet {
            bytes,
            decode_limits: self.limits.decode,
        })
    }

    async fn read_frame_length(&mut self) -> Result<usize, SocketError> {
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
        Ok(len)
    }
}

async fn read_frame_body(
    stream: &mut BufReader<OwnedReadHalf>,
    buf: &mut Vec<u8>,
    len: usize,
) -> Result<(), SocketError> {
    buf.clear();
    buf.try_reserve_exact(len)
        .map_err(|_| FrameError::AllocationFailed { requested: len })?;
    buf.resize(len, 0);
    let mut received = 0;
    while received < len {
        let bytes_read = stream.read(&mut buf[received..]).await?;
        if bytes_read == 0 {
            return Err(FrameError::TruncatedFrame {
                expected: len,
                received,
            }
            .into());
        }
        received += bytes_read;
    }
    Ok(())
}

fn decompress_frame(
    frame: &[u8],
    threshold: u32,
    limit: usize,
    inflater: &mut Decompress,
) -> Result<Vec<u8>, FrameError> {
    let mut input = frame;
    let data_length = VarI32::decode(&mut input)
        .map_err(|_| FrameError::InvalidDataLength)?
        .0;
    let data_length = usize::try_from(data_length).map_err(|_| FrameError::InvalidDataLength)?;

    if data_length == 0 {
        if input.is_empty() {
            return Err(FrameError::EmptyFrame);
        }
        let mut packet = Vec::new();
        packet
            .try_reserve_exact(input.len())
            .map_err(|_| FrameError::AllocationFailed {
                requested: input.len(),
            })?;
        packet.extend_from_slice(input);
        return Ok(packet);
    }

    if u32::try_from(data_length).is_ok_and(|length| length < threshold) {
        return Err(FrameError::CompressedBelowThreshold {
            data_length,
            threshold,
        });
    }
    if data_length > limit {
        return Err(FrameError::DecompressedTooLarge {
            requested: data_length,
            limit,
        });
    }

    let mut packet = Vec::new();
    packet
        .try_reserve_exact(data_length)
        .map_err(|_| FrameError::AllocationFailed {
            requested: data_length,
        })?;
    inflater.reset(true);
    loop {
        let consumed_before = inflater.total_in();
        let produced_before = inflater.total_out();
        let rest = &input[consumed_before as usize..];
        let status = if packet.len() < data_length {
            inflater.decompress_vec(rest, &mut packet, FlushDecompress::None)
        } else {
            inflater.decompress(rest, &mut [0u8; 1], FlushDecompress::None)
        }
        .map_err(|_| FrameError::CorruptCompressedData)?;
        if inflater.total_out() > data_length as u64 {
            return Err(FrameError::DecompressedLengthMismatch {
                declared: data_length,
            });
        }
        if status == Status::StreamEnd {
            break;
        }
        if inflater.total_in() == consumed_before && inflater.total_out() == produced_before {
            return Err(FrameError::CorruptCompressedData);
        }
    }
    if packet.len() != data_length {
        return Err(FrameError::DecompressedLengthMismatch {
            declared: data_length,
        });
    }
    if inflater.total_in() != input.len() as u64 {
        return Err(FrameError::CorruptCompressedData);
    }
    Ok(packet)
}

const VARINT_MAX_BYTES: usize = 5;
const FRAME_HEADER_BYTES: usize = 2 * VARINT_MAX_BYTES;
const FRAME_BUFFER_RETAINED_DIVISOR: usize = 8;

struct OutboundCompression {
    threshold: u32,
    enabled: Arc<AtomicBool>,
    active: bool,
    deflater: Compress,
    frame: Vec<u8>,
}

pub struct ClientWriter {
    stream: BufWriter<OwnedWriteHalf>,
    limits: FrameLimits,
    frame: Vec<u8>,
    retained_bytes: usize,
    compression: Option<OutboundCompression>,
}

impl ClientWriter {
    /// The threshold the next Set Compression packet must announce, while
    /// compression is configured but not yet switched on.
    pub fn pending_compression_threshold(&self) -> Option<u32> {
        self.compression
            .as_ref()
            .filter(|compression| !compression.active)
            .map(|compression| compression.threshold)
    }

    pub fn compression_enabled(&self) -> bool {
        self.compression
            .as_ref()
            .is_some_and(|compression| compression.active)
    }

    /// Switches both halves to compressed framing. Call right after the Set
    /// Compression packet has been passed to [`ClientWriter::send`]: every
    /// later frame is compressed, and every inbound frame whose length prefix
    /// is read from now on is parsed as compressed. The packet is still in
    /// this writer's buffer at that point, so the peer cannot have answered it.
    pub fn enable_compression(&mut self) {
        if let Some(compression) = &mut self.compression {
            compression.active = true;
            compression.enabled.store(true, Ordering::Release);
        }
    }

    pub async fn send<T: Encode>(&mut self, packet: &T) -> Result<(), SocketError> {
        self.frame.clear();
        self.frame.resize(FRAME_HEADER_BYTES, 0);
        packet.encode(&mut self.frame);
        let result = self.write_frame().await;
        if self.frame.capacity() > self.retained_bytes {
            self.frame.clear();
            self.frame.shrink_to(self.retained_bytes);
        }
        if let Some(compression) = &mut self.compression
            && compression.frame.capacity() > self.retained_bytes
        {
            compression.frame.clear();
            compression.frame.shrink_to(self.retained_bytes);
        }
        result
    }

    async fn write_frame(&mut self) -> Result<(), SocketError> {
        let body_len = self.frame.len() - FRAME_HEADER_BYTES;
        if body_len > self.limits.max_outbound_frame_bytes {
            return Err(FrameError::FrameTooLarge {
                requested: body_len,
                limit: self.limits.max_outbound_frame_bytes,
            }
            .into());
        }
        let threshold = self
            .compression
            .as_ref()
            .filter(|compression| compression.active)
            .map(|compression| compression.threshold);
        let (buf, data_length) = match threshold {
            None => (&mut self.frame, None),
            Some(threshold) if u32::try_from(body_len).is_ok_and(|len| len < threshold) => {
                (&mut self.frame, Some(0))
            }
            Some(_) => {
                let compression = self
                    .compression
                    .as_mut()
                    .expect("active threshold implies compression state");
                deflate_into(
                    &mut compression.deflater,
                    &self.frame[FRAME_HEADER_BYTES..],
                    &mut compression.frame,
                )?;
                (&mut compression.frame, Some(body_len))
            }
        };
        let start = write_frame_header(buf, data_length)?;
        self.stream.write_all(&buf[start..]).await?;
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<(), SocketError> {
        self.stream.flush().await?;
        Ok(())
    }
}

fn deflate_into(
    deflater: &mut Compress,
    input: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), FrameError> {
    out.clear();
    out.resize(FRAME_HEADER_BYTES, 0);
    out.reserve(input.len() / 4 + 64);
    deflater.reset();
    loop {
        if out.len() == out.capacity() {
            out.reserve(out.capacity());
        }
        let consumed = deflater.total_in() as usize;
        let status = deflater
            .compress_vec(&input[consumed..], out, FlushCompress::Finish)
            .map_err(|_| FrameError::CompressionFailed)?;
        if status == Status::StreamEnd {
            return Ok(());
        }
    }
}

fn encode_varint(value: u32) -> ([u8; VARINT_MAX_BYTES], usize) {
    let mut bytes = [0u8; VARINT_MAX_BYTES];
    let mut value = value;
    let mut written = 0;
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes[written] = byte;
        written += 1;
        if value == 0 {
            return (bytes, written);
        }
    }
}

fn outbound_varint(value: usize) -> Result<([u8; VARINT_MAX_BYTES], usize), FrameError> {
    let value = i32::try_from(value)
        .map_err(|_| FrameError::OutboundLengthOverflow { requested: value })?;
    Ok(encode_varint(value as u32))
}

fn write_frame_header(buf: &mut [u8], data_length: Option<usize>) -> Result<usize, FrameError> {
    let mut start = FRAME_HEADER_BYTES;
    if let Some(data_length) = data_length {
        let (bytes, written) = outbound_varint(data_length)?;
        start -= written;
        buf[start..start + written].copy_from_slice(&bytes[..written]);
    }
    let (bytes, written) = outbound_varint(buf.len() - start)?;
    start -= written;
    buf[start..start + written].copy_from_slice(&bytes[..written]);
    Ok(start)
}

#[derive(Debug)]
pub struct ServerSocket {
    pub listener: TcpListener,
    limits: FrameLimits,
    compression_threshold: Option<u32>,
}

impl ServerSocket {
    pub fn new(listener: TcpListener, limits: FrameLimits) -> Self {
        Self {
            listener,
            limits,
            compression_threshold: None,
        }
    }

    /// Packets of at least `threshold` bytes are zlib-compressed once a
    /// connection enables compression; `None` never compresses.
    pub fn with_compression_threshold(mut self, threshold: Option<u32>) -> Self {
        self.compression_threshold = threshold;
        self
    }

    pub async fn accept(&self) -> std::io::Result<ClientSocket> {
        let (stream, addr) = self.listener.accept().await?;
        stream.set_nodelay(true)?;
        Ok(ClientSocket {
            stream,
            peer_addr: addr,
            limits: self.limits,
            compression_threshold: self.compression_threshold,
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
    async fn retained_capacity_is_derived_from_the_outbound_limit() {
        let (socket, _peer) = connected(FrameLimits::default()).await;
        let (_, writer) = socket.into_split();
        assert_eq!(writer.retained_bytes, 1024 * 1024);
    }

    #[tokio::test]
    async fn chunk_sized_frames_do_not_reallocate_after_warm_up() {
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (_, mut writer) = socket.into_split();

        let chunk = vec![7; 80_409];
        writer.send(&Bytes(chunk.clone())).await.unwrap();
        let warm = writer.frame.capacity();
        assert!(warm > chunk.len());
        assert!(warm <= writer.retained_bytes);

        for _ in 0..2 {
            writer.send(&Bytes(chunk.clone())).await.unwrap();
            assert_eq!(writer.frame.capacity(), warm);
        }
        writer.flush().await.unwrap();
        drop(writer);

        let mut expected = Vec::new();
        for _ in 0..3 {
            expected.extend_from_slice(&frame_bytes(&chunk));
        }
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, expected);
    }

    #[tokio::test]
    async fn frame_buffer_is_released_after_an_oversized_frame() {
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (_, mut writer) = socket.into_split();
        let receiving = tokio::spawn(async move {
            let mut received = Vec::new();
            peer.read_to_end(&mut received).await.unwrap();
            received
        });

        let large = vec![1; 4 * writer.retained_bytes];
        writer.send(&Bytes(large.clone())).await.unwrap();
        assert!(writer.frame.capacity() <= writer.retained_bytes);

        writer.send(&Bytes(vec![2, 3])).await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);

        let mut expected = frame_bytes(&large);
        expected.extend_from_slice(&frame_bytes(&[2, 3]));
        assert_eq!(receiving.await.unwrap(), expected);
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

    fn zlib(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn inflate(data: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    fn compressed_frame(data_length: i32, body: &[u8]) -> Vec<u8> {
        let mut inner = Vec::new();
        VarI32(data_length).encode(&mut inner);
        inner.extend_from_slice(body);
        frame_bytes(&inner)
    }

    async fn compressed(
        threshold: u32,
        limits: FrameLimits,
    ) -> (ClientReader, ClientWriter, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = TcpStream::connect(address).await.unwrap();
        let socket = ServerSocket::new(listener, limits)
            .with_compression_threshold(Some(threshold))
            .accept()
            .await
            .unwrap();
        let (reader, mut writer) = socket.into_split();
        assert_eq!(writer.pending_compression_threshold(), Some(threshold));
        writer.enable_compression();
        assert_eq!(writer.pending_compression_threshold(), None);
        assert!(writer.compression_enabled());
        (reader, writer, peer)
    }

    async fn read_raw_frame(peer: &mut TcpStream) -> Vec<u8> {
        let mut prefix = Vec::new();
        loop {
            let byte = peer.read_u8().await.unwrap();
            prefix.push(byte);
            if byte & 0x80 == 0 {
                break;
            }
        }
        let len = VarI32::decode(&mut prefix.as_slice()).unwrap().0 as usize;
        let mut body = vec![0; len];
        peer.read_exact(&mut body).await.unwrap();
        body
    }

    #[tokio::test]
    async fn outbound_frames_follow_the_compressed_format_around_the_threshold() {
        let (_reader, mut writer, mut peer) = compressed(256, FrameLimits::default()).await;
        let below = vec![7; 255];
        let at = vec![8; 256];
        let above: Vec<u8> = (0..60_000u32).map(|i| (i % 7) as u8).collect();
        for payload in [&below, &at, &above] {
            writer.send(&Bytes(payload.clone())).await.unwrap();
        }
        writer.flush().await.unwrap();

        let frame = read_raw_frame(&mut peer).await;
        assert_eq!(frame[0], 0);
        assert_eq!(&frame[1..], below.as_slice());

        for payload in [&at, &above] {
            let frame = read_raw_frame(&mut peer).await;
            let mut rest = frame.as_slice();
            let data_length = VarI32::decode(&mut rest).unwrap().0;
            assert_eq!(data_length as usize, payload.len());
            assert_eq!(&inflate(rest), payload);
        }
    }

    #[tokio::test]
    async fn uncompressed_writer_keeps_plain_frames_until_enabled() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut peer = TcpStream::connect(address).await.unwrap();
        let socket = ServerSocket::new(listener, FrameLimits::default())
            .with_compression_threshold(Some(0))
            .accept()
            .await
            .unwrap();
        let (_reader, mut writer) = socket.into_split();
        writer.send(&Bytes(vec![1, 2, 3])).await.unwrap();
        writer.enable_compression();
        writer.send(&Bytes(vec![4])).await.unwrap();
        writer.flush().await.unwrap();

        assert_eq!(read_raw_frame(&mut peer).await, [1, 2, 3]);
        let frame = read_raw_frame(&mut peer).await;
        assert_eq!(frame[0], 1);
        assert_eq!(inflate(&frame[1..]), [4]);
    }

    #[tokio::test]
    async fn enabling_without_a_threshold_is_a_no_op() {
        let (socket, mut peer) = connected(FrameLimits::default()).await;
        let (_reader, mut writer) = socket.into_split();
        assert_eq!(writer.pending_compression_threshold(), None);
        writer.enable_compression();
        assert!(!writer.compression_enabled());
        writer.send(&Bytes(vec![0; 300])).await.unwrap();
        writer.flush().await.unwrap();
        assert_eq!(read_raw_frame(&mut peer).await, vec![0; 300]);
    }

    #[tokio::test]
    async fn outbound_limit_applies_to_the_uncompressed_packet() {
        let limits = FrameLimits {
            max_outbound_frame_bytes: 1000,
            ..FrameLimits::default()
        };
        let (_reader, mut writer, _peer) = compressed(0, limits).await;
        assert!(matches!(
            writer.send(&Bytes(vec![0; 1001])).await,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                requested: 1001,
                limit: 1000
            }))
        ));
    }

    #[tokio::test]
    async fn compression_buffer_is_reused_and_released() {
        let (_reader, mut writer, mut peer) = compressed(256, FrameLimits::default()).await;
        let receiving = tokio::spawn(async move {
            let mut received = Vec::new();
            peer.read_to_end(&mut received).await.unwrap();
            received
        });
        let chunk: Vec<u8> = (0..80_000u32).map(|i| (i * 31 % 251) as u8).collect();
        writer.send(&Bytes(chunk.clone())).await.unwrap();
        let warm = writer.compression.as_ref().unwrap().frame.capacity();
        for _ in 0..3 {
            writer.send(&Bytes(chunk.clone())).await.unwrap();
            assert_eq!(writer.compression.as_ref().unwrap().frame.capacity(), warm);
        }
        let noise: Vec<u8> = (0..4 * writer.retained_bytes as u64)
            .map(|i| (i.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 56) as u8)
            .collect();
        writer.send(&Bytes(noise)).await.unwrap();
        assert!(writer.compression.as_ref().unwrap().frame.capacity() <= writer.retained_bytes);
        writer.flush().await.unwrap();
        drop(writer);
        receiving.await.unwrap();
    }

    #[tokio::test]
    async fn inbound_compressed_frames_decode_around_the_threshold() {
        let (mut reader, _writer, mut peer) = compressed(256, FrameLimits::default()).await;
        let small = vec![5; 10];
        let large: Vec<u8> = (0..5000u32).map(|i| (i % 13) as u8).collect();
        let mut wire = compressed_frame(0, &small);
        wire.extend(compressed_frame(256, &zlib(&[9; 256])));
        wire.extend(compressed_frame(large.len() as i32, &zlib(&large)));
        peer.write_all(&wire).await.unwrap();

        assert_eq!(reader.receive().await.unwrap().bytes, small);
        assert_eq!(reader.receive().await.unwrap().bytes, vec![9; 256]);
        assert_eq!(reader.receive().await.unwrap().bytes, large);
    }

    #[tokio::test]
    async fn reader_stays_plain_until_the_writer_enables_compression() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut peer = TcpStream::connect(address).await.unwrap();
        let socket = ServerSocket::new(listener, FrameLimits::default())
            .with_compression_threshold(Some(64))
            .accept()
            .await
            .unwrap();
        let (mut reader, mut writer) = socket.into_split();
        peer.write_all(&frame_bytes(&[0, 42])).await.unwrap();
        assert_eq!(reader.receive().await.unwrap().bytes, [0, 42]);
        writer.enable_compression();
        peer.write_all(&frame_bytes(&[0, 42])).await.unwrap();
        assert_eq!(reader.receive().await.unwrap().bytes, [42]);
    }

    fn decompress(frame: &[u8], threshold: u32) -> Result<Vec<u8>, FrameError> {
        decompress_frame(
            frame,
            threshold,
            MAX_DECOMPRESSED_BYTES,
            &mut Decompress::new(true),
        )
    }

    fn inner(data_length: i32, body: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        VarI32(data_length).encode(&mut frame);
        frame.extend_from_slice(body);
        frame
    }

    #[test]
    fn rejects_compressed_packets_below_the_threshold() {
        assert_eq!(
            decompress(&inner(10, &zlib(&[1; 10])), 256),
            Err(FrameError::CompressedBelowThreshold {
                data_length: 10,
                threshold: 256
            })
        );
    }

    #[test]
    fn rejects_declared_lengths_above_the_limit_before_allocating() {
        assert_eq!(
            decompress(&inner(MAX_DECOMPRESSED_BYTES as i32 + 1, &[0x78]), 0),
            Err(FrameError::DecompressedTooLarge {
                requested: MAX_DECOMPRESSED_BYTES + 1,
                limit: MAX_DECOMPRESSED_BYTES
            })
        );
        assert_eq!(
            decompress_frame(
                &inner(i32::MAX, &[0x78]),
                0,
                2 * 1024 * 1024,
                &mut Decompress::new(true)
            ),
            Err(FrameError::DecompressedTooLarge {
                requested: i32::MAX as usize,
                limit: 2 * 1024 * 1024
            })
        );
        assert_eq!(
            decompress(&inner(-1, &[0x78]), 0),
            Err(FrameError::InvalidDataLength)
        );
        assert_eq!(
            decompress(&[0x80, 0x80, 0x80, 0x80, 0x80], 0),
            Err(FrameError::InvalidDataLength)
        );
    }

    #[test]
    fn rejects_payloads_that_do_not_match_the_declared_length() {
        let data = vec![3; 1000];
        assert_eq!(
            decompress(&inner(999, &zlib(&data)), 0),
            Err(FrameError::DecompressedLengthMismatch { declared: 999 })
        );
        assert_eq!(
            decompress(&inner(1001, &zlib(&data)), 0),
            Err(FrameError::DecompressedLengthMismatch { declared: 1001 })
        );
        assert_eq!(decompress(&inner(1000, &zlib(&data)), 0), Ok(data));
    }

    #[test]
    fn zip_bombs_stop_at_the_declared_length() {
        let bomb = zlib(&vec![0; 64 * 1024 * 1024]);
        assert!(bomb.len() < 128 * 1024);
        assert_eq!(
            decompress(&inner(1024, &bomb), 0),
            Err(FrameError::DecompressedLengthMismatch { declared: 1024 })
        );
    }

    #[test]
    fn rejects_corrupt_truncated_and_trailing_zlib_data() {
        let data: Vec<u8> = (0..2000u32).map(|i| (i % 17) as u8).collect();
        let good = zlib(&data);

        let mut corrupt = good.clone();
        corrupt[0] = 0x00;
        assert_eq!(
            decompress(&inner(2000, &corrupt), 0),
            Err(FrameError::CorruptCompressedData)
        );

        let mut flipped = good.clone();
        let middle = flipped.len() / 2;
        flipped[middle] ^= 0xFF;
        assert!(decompress(&inner(2000, &flipped), 0).is_err());

        let truncated = &good[..good.len() - 6];
        assert!(decompress(&inner(2000, truncated), 0).is_err());

        let mut trailing = good.clone();
        trailing.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            decompress(&inner(2000, &trailing), 0),
            Err(FrameError::CorruptCompressedData)
        );

        assert_eq!(
            decompress(&inner(2000, &[]), 0),
            Err(FrameError::CorruptCompressedData)
        );
        assert_eq!(decompress(&[0], 0), Err(FrameError::EmptyFrame));
    }

    #[test]
    fn inflater_is_reusable_after_an_error() {
        let mut inflater = Decompress::new(true);
        let data = vec![4; 500];
        assert!(
            decompress_frame(
                &inner(500, &[0xFF; 8]),
                0,
                MAX_DECOMPRESSED_BYTES,
                &mut inflater
            )
            .is_err()
        );
        assert_eq!(
            decompress_frame(
                &inner(500, &zlib(&data)),
                0,
                MAX_DECOMPRESSED_BYTES,
                &mut inflater
            ),
            Ok(data)
        );
    }

    #[tokio::test]
    async fn malformed_compressed_frame_is_a_socket_error() {
        let (mut reader, _writer, mut peer) = compressed(0, FrameLimits::default()).await;
        peer.write_all(&compressed_frame(100, &[0xDE, 0xAD, 0xBE, 0xEF]))
            .await
            .unwrap();
        assert!(matches!(
            reader.receive().await,
            Err(SocketError::Frame(FrameError::CorruptCompressedData))
        ));
    }
}
