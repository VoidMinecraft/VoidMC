use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::TcpSocket;
use tokio::sync::mpsc;

use super::*;

async fn narrow_pair() -> (TcpStream, TcpStream) {
    let listening = TcpSocket::new_v6().unwrap();
    listening.set_recv_buffer_size(64 * 1024).unwrap();
    listening.bind("[::1]:0".parse().unwrap()).unwrap();
    let listener = listening.listen(1).unwrap();
    let connecting = TcpSocket::new_v6().unwrap();
    connecting.set_send_buffer_size(64 * 1024).unwrap();
    let near = connecting
        .connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (far, _) = listener.accept().await.unwrap();
    (near, far)
}

#[tokio::test]
async fn a_peer_that_stops_reading_fails_the_flush() {
    let (near, _far) = pair().await;
    let mut peer = Peer::new(near);
    peer.outbound = vec![0; 64 * 1024 * 1024];
    let error = peer
        .flush_within(Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[tokio::test]
async fn a_peer_that_swallows_a_little_now_and_then_is_lost_once_its_waits_add_up() {
    let (near, mut far) = narrow_pair().await;
    let swallowing = tokio::spawn(async move {
        let mut buffer = vec![0; 64 * 1024];
        loop {
            sleep(Duration::from_millis(20)).await;
            if far.read(&mut buffer).await.map_or(true, |read| read == 0) {
                break;
            }
        }
    });
    let mut peer = Peer::new(near);
    let limit = Duration::from_secs(1);
    let started = Instant::now();
    let error = loop {
        peer.outbound.resize(1024 * 1024, 0);
        if let Err(error) = peer.flush_within(limit).await {
            break error;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "every flush made a little progress, and the peer was never lost"
        );
    };
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(peer.stalled_for >= limit && peer.stalled_for < limit * 3);
    swallowing.abort();
}

fn reader_with_pauses(mut far: TcpStream) -> (mpsc::UnboundedSender<Duration>, Arc<AtomicU64>) {
    let (pause, mut pauses) = mpsc::unbounded_channel::<Duration>();
    let read = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&read);
    tokio::spawn(async move {
        let mut buffer = vec![0; 1 << 20];
        loop {
            while let Ok(pause) = pauses.try_recv() {
                sleep(pause).await;
            }
            match timeout(Duration::from_millis(5), far.read(&mut buffer)).await {
                Ok(Ok(0) | Err(_)) => break,
                Ok(Ok(count)) => {
                    counted.fetch_add(count as u64, Ordering::Relaxed);
                }
                Err(_) => {}
            }
        }
    });
    (pause, read)
}

const PATIENT: Duration = Duration::from_secs(3);

async fn burst(peer: &mut Peer, reader: &mpsc::UnboundedSender<Duration>, pause: Duration) {
    reader.send(pause).unwrap();
    peer.outbound.resize(1 << 20, 0);
    peer.flush_within(PATIENT).await.unwrap();
}

#[tokio::test]
async fn a_stall_long_past_does_not_count_against_the_next_burst() {
    let (near, far) = narrow_pair().await;
    let (reader, _) = reader_with_pauses(far);
    let mut peer = Peer::new(near);
    burst(&mut peer, &reader, Duration::from_millis(400)).await;
    assert!(!peer.stalled_for.is_zero(), "the first burst never waited");
    sleep(PATIENT + Duration::from_secs(1)).await;
    burst(&mut peer, &reader, Duration::ZERO).await;
}

#[tokio::test]
async fn a_queue_written_without_waiting_starts_the_count_again() {
    let (near, far) = narrow_pair().await;
    let (reader, read) = reader_with_pauses(far);
    let mut peer = Peer::new(near);
    let mut sent = 0;
    for _ in 0..4 {
        burst(&mut peer, &reader, Duration::from_secs(1)).await;
        sent += 1 << 20;
        timeout(Duration::from_secs(10), async {
            while read.load(Ordering::Relaxed) < sent {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the reader never caught up");
        sent += 1;
        peer.outbound.push(0);
        peer.flush_within(PATIENT).await.unwrap();
        assert!(peer.stalled_for.is_zero());
    }
}

/// A socket the kernel refuses buffers to, `refusals` times in a row.
struct Starved {
    refusals: Cell<u32>,
    taken: Cell<usize>,
}

impl Starved {
    fn refusing(refusals: u32) -> Self {
        Self {
            refusals: Cell::new(refusals),
            taken: Cell::new(0),
        }
    }
}

impl TryWrite for Starved {
    fn try_write(&self, bytes: &[u8]) -> io::Result<usize> {
        if self.refusals.get() > 0 {
            self.refusals.set(self.refusals.get() - 1);
            return Err(io::Error::from_raw_os_error(libc::ENOBUFS));
        }
        self.taken.set(self.taken.get() + bytes.len());
        Ok(bytes.len())
    }

    async fn writable(&self) -> io::Result<()> {
        tokio::task::yield_now().await;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn a_kernel_out_of_buffers_is_waited_for_not_a_lost_peer() {
    let mut peer = Peer::new(Starved::refusing(3));
    peer.outbound = vec![7; 100];
    let started = Instant::now();
    peer.flush_within(Duration::from_secs(5)).await.unwrap();
    assert_eq!(peer.stream.taken.get(), 100);
    assert!(peer.outbound.is_empty());
    assert_eq!(peer.stalled_for, NO_BUFFER_PAUSE * 3);
    assert_eq!(started.elapsed(), NO_BUFFER_PAUSE * 3);
}

#[tokio::test(start_paused = true)]
async fn a_kernel_out_of_buffers_for_too_long_counts_as_a_stuck_peer() {
    let mut peer = Peer::new(Starved::refusing(1000));
    peer.outbound = vec![7; 100];
    let limit = NO_BUFFER_PAUSE * 50;
    let error = peer.flush_within(limit).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(peer.stalled_for, limit);
    assert_eq!(peer.stream.taken.get(), 0);
}
