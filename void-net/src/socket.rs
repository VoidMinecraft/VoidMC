use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

pub struct Packet(Vec<u8>);

impl Packet {
    pub fn decode<T: Decode>(&self) -> std::io::Result<T> {
        let mut slice = self.0.as_slice();
        T::decode(&mut slice).map_err(|e| {
            // An unrecognized packet id is an expected, non-fatal condition (e.g. a
            // packet we don't handle yet), so flag it distinctly from genuine decode
            // failures. Callers can match on `ErrorKind::Unsupported` to log it as a
            // warning rather than an error.
            let kind = match e {
                DecodeError::InvalidPacketId(_) => std::io::ErrorKind::Unsupported,
                _ => std::io::ErrorKind::InvalidData,
            };
            std::io::Error::new(kind, e.to_string())
        })
    }
}

pub struct ClientSocket(TcpStream, pub SocketAddr, Vec<u8>);

impl ClientSocket {
    pub async fn receive(&mut self) -> std::io::Result<Packet> {
        // Partial frames live in `self.2`, not in this future: `Client::run` races
        // `receive()` against outgoing packets in `tokio::select!`, and any bytes held
        // in a cancelled future would be lost, corrupting the stream.
        loop {
            // 1. Look for a complete length prefix (VarI32, at most 5 bytes).
            if let Some(last) = self.2.iter().take(5).position(|b| b & 0x80 == 0) {
                let prefix = last + 1;
                let len = VarI32::decode(&mut &self.2[..prefix])
                    .map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid VarI32")
                    })?
                    .0;
                let len = usize::try_from(len).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid frame length")
                })?;

                // 2. Return the frame once its body is fully buffered.
                let end = prefix + len;
                if self.2.len() >= end {
                    let packet = Packet(self.2[prefix..end].to_vec());
                    self.2.drain(..end);
                    return Ok(packet);
                }
            } else if self.2.len() >= 5 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid VarI32",
                ));
            }

            // 3. Read more. `read_buf` is cancellation-safe: bytes already received
            //    stay in `self.2` even if this future is dropped mid-await.
            let n = self.0.read_buf(&mut self.2).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Connection closed",
                ));
            }
        }
    }

    pub async fn send<T: Encode>(&mut self, packet: &T) -> std::io::Result<()> {
        // 1. Encode the packet into a buffer
        let mut packet_buf = Vec::new();
        packet.encode(&mut packet_buf);

        // 2. Determine the length of the buffer and encode it as a vari32
        let len = packet_buf.len() as i32;
        let mut len_buf = Vec::new();
        VarI32(len).encode(&mut len_buf);

        // 3. Send the length prefix followed by the packet buffer to the stream
        self.0.write_all(&len_buf).await?;
        self.0.write_all(&packet_buf).await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct ServerSocket(pub TcpListener);

impl ServerSocket {
    pub async fn accept(&self) -> std::io::Result<ClientSocket> {
        let (stream, addr) = self.0.accept().await?;
        Ok(ClientSocket(stream, addr, Vec::with_capacity(8192)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn fragmented_frames_survive_cancelled_reads_and_interleaved_writes() {
        let listener = ServerSocket(TcpListener::bind("127.0.0.1:0").await.unwrap());
        let mut peer = TcpStream::connect(listener.0.local_addr().unwrap())
            .await
            .unwrap();
        let mut socket = listener.accept().await.unwrap();

        let payload = vec![42u8; 300];
        let mut frame = Vec::new();
        VarI32(300).encode(&mut frame);
        frame.extend_from_slice(&payload);

        // Cancel `receive()` midway through the VarInt, then midway through the body,
        // sending an outgoing packet in between, exactly as `Client::run` does.
        for fragment in [&frame[..1], &frame[1..37]] {
            peer.write_all(fragment).await.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(10), socket.receive())
                    .await
                    .is_err()
            );
            socket.send(&7i32).await.unwrap();
            let mut response = [0u8; 5];
            peer.read_exact(&mut response).await.unwrap();
            assert_eq!(response, [4, 0, 0, 0, 7]);
        }

        // The rest of the frame arrives coalesced with a second, complete frame.
        let mut remainder = frame[37..].to_vec();
        remainder.extend_from_slice(&[1, 99]);
        peer.write_all(&remainder).await.unwrap();
        assert_eq!(socket.receive().await.unwrap().0, payload);
        assert_eq!(socket.receive().await.unwrap().0, [99]);
    }
}
