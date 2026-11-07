use std::io::{ErrorKind, Result};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::UnixStream;

/// A socket of a test. On macOS `accept` and `set_cloexec` are two steps, as `socket` and
/// `set_cloexec` are, so a child started by another test meanwhile can hold any socket: a drop
/// then sends no end to the peer, and only a shutdown does.
pub trait Socket: Sized {
    fn dup(&self) -> Result<Self>;
    fn end(&self) -> Result<()>;
}

impl Socket for TcpStream {
    fn dup(&self) -> Result<Self> {
        self.try_clone()
    }

    fn end(&self) -> Result<()> {
        self.shutdown(Shutdown::Both)
    }
}

impl Socket for UnixStream {
    fn dup(&self) -> Result<Self> {
        self.try_clone()
    }

    fn end(&self) -> Result<()> {
        self.shutdown(Shutdown::Both)
    }
}

/// The one way a test ends a connection whose peer waits for that end.
pub fn hang_up(socket: impl Socket) {
    match socket.end() {
        Err(error) if error.kind() != ErrorKind::NotConnected => panic!("{error}"),
        _ => {}
    }
}

/// The same socket as a child started at the wrong moment holds it, for as long as this lives.
pub fn held_by_a_child<S: Socket>(socket: &S) -> S {
    socket.dup().unwrap()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;

    const SILENCE: Duration = Duration::from_millis(50);

    fn ended(reader: &mut impl Read) -> bool {
        matches!(reader.read(&mut [0; 1]), Ok(0))
    }

    #[test]
    fn under_a_child_a_drop_ends_nothing_and_a_hang_up_ends_a_unix_socket() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        theirs.set_read_timeout(Some(SILENCE)).unwrap();
        let child = held_by_a_child(&ours);
        let second = ours.dup().unwrap();
        drop(ours);
        assert!(!ended(&mut theirs));
        hang_up(second);
        assert!(ended(&mut theirs));
        drop(child);
    }

    #[test]
    fn under_a_child_a_drop_ends_nothing_and_a_hang_up_ends_a_tcp_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut theirs, _) = listener.accept().unwrap();
        theirs.set_read_timeout(Some(SILENCE)).unwrap();
        let child = held_by_a_child(&ours);
        let second = ours.dup().unwrap();
        drop(ours);
        assert!(!ended(&mut theirs));
        hang_up(second);
        assert!(ended(&mut theirs));
        drop(child);
    }

    #[test]
    fn a_peer_already_gone_is_no_error() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        drop(theirs);
        hang_up(ours);
    }

    #[test]
    fn what_was_written_before_a_hang_up_still_arrives() {
        let (mut ours, mut theirs) = UnixStream::pair().unwrap();
        ours.write_all(b"last").unwrap();
        hang_up(ours);
        let mut all = String::new();
        theirs.read_to_string(&mut all).unwrap();
        assert_eq!(all, "last");
    }
}
