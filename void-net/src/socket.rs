use std::usize;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::{AsyncPacketDecode, AsyncPacketEncode, StatePacket};

pub struct ClientSocket(TcpStream);

impl ClientSocket {
    pub async fn receive<T: StatePacket>(&mut self) -> std::io::Result<T> {
        let length: usize = self.0.decode_vari32().await?.try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid packet length")
        })?;

        let mut buffer = vec![0; length];
        self.0.read_exact(&mut buffer).await?;

        T::decode(&mut buffer.as_slice())
    }

    pub async fn send<T: StatePacket>(&mut self, packet: &T) -> std::io::Result<()> {
        let mut buffer = Vec::new();
        packet.encode(&mut buffer)?;

        let length: i32 = buffer.len().try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid packet length")
        })?;

        self.0.encode_vari32(length).await?;
        self.0.write_all(&buffer).await?;

        Ok(())
    }
}

pub struct ServerSocket(pub TcpListener);

impl ServerSocket {
    pub async fn accept(&self) -> std::io::Result<ClientSocket> {
        let (stream, _) = self.0.accept().await?;
        Ok(ClientSocket(stream))
    }
}
