use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use void_codec::{Decode, Encode, VarI32};

pub struct Packet(Vec<u8>);

impl Packet {
    pub fn decode<T: Decode>(&self) -> std::io::Result<T> {
        let mut slice = self.0.as_slice();
        T::decode(&mut slice)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }
}

pub struct ClientSocket(TcpStream, pub SocketAddr);

impl ClientSocket {
    pub async fn receive(&mut self) -> std::io::Result<Packet> {
        // 1. Read a vari32 from the stream to determine packet length
        let mut len_buf = [0u8; 5]; // Max 5 bytes for VarI32
        let mut bytes_read = 0;

        // Read bytes until we have a complete VarI32
        loop {
            let n = self
                .0
                .read(&mut len_buf[bytes_read..bytes_read + 1])
                .await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Connection closed",
                ));
            }
            bytes_read += n;

            // Check if this byte ends the VarI32 (high bit not set)
            if len_buf[bytes_read - 1] & 0x80 == 0 {
                break;
            }
            if bytes_read >= 5 {
                break;
            }
        }

        // 2. Convert vari32 to usize
        let mut slice = &len_buf[..bytes_read];
        let len = VarI32::decode(&mut slice)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid VarI32"))?;
        let len = len.0 as usize;

        // 3. Read the whole packet into a buffer
        let mut packet_buf = vec![0u8; len];
        self.0.read_exact(&mut packet_buf).await?;

        Ok(Packet(packet_buf))
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
        Ok(ClientSocket(stream, addr))
    }
}
