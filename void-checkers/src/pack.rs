//! Serves the resource pack over HTTP, since clients can only download packs
//! from a URL. `pack/build_pack.py` generates the archive.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

const ARCHIVE: &[u8] = include_bytes!("../pack/checkers.zip");

/// Answers every request with the archive; the path does not matter.
pub fn serve(address: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(address)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if let Err(error) = respond(stream) {
                tracing::warn!(%error, "resource pack request failed");
            }
        }
    });
    Ok(())
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    let mut request = [0; 1024];
    let _ = stream.read(&mut request)?;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ARCHIVE.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(ARCHIVE)
}
