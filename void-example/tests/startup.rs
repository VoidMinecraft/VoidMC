//! Launches the built `voidmc-example` binary, as `cargo run -p voidmc-example`
//! would, and checks that it starts: it listens on its fixed address
//! `127.0.0.1:25565`, generates its spawn area, keeps running and logs no
//! error or panic.
//!
//! The test runs the example with its default address (`VOID_LISTEN_ADDR`
//! unset), so it needs port 25565 free. Runs of this test on one machine take
//! turns: each holds an advisory lock on a file in the system temporary
//! directory while its server runs, so two overlapping `cargo test` runs queue
//! instead of colliding. The test then waits up to `PORT_WAIT_LIMIT` for the
//! lock and the port, and fails with a clear message if the port is still
//! taken, for instance by an example server left running in another terminal.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_protocol::{PROTOCOL_VERSION, State, clientbound, serverbound};

const LISTEN_HOST: &str = "127.0.0.1";
const LISTEN_PORT: u16 = 25565;

/// Upper bound on each wait. Startup takes about a second; the bound only
/// keeps a hung server from hanging the test run.
const WAIT_LIMIT: Duration = Duration::from_secs(120);

/// Upper bound on waiting for another run of this test to release the port
/// lock, then for the port to be free. Another run holds the lock for about a
/// second; the bound is that of any other wait in the test, so a slow machine
/// does not fail here, and a server left running fails the test in two
/// minutes rather than hanging it.
const PORT_WAIT_LIMIT: Duration = WAIT_LIMIT;

const PORT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[test]
fn example_server_starts_listens_and_keeps_running() {
    let listen_addr = format!("{LISTEN_HOST}:{LISTEN_PORT}");
    let deadline = Instant::now() + PORT_WAIT_LIMIT;
    // Declared before the server, so released after the server is stopped and
    // the port is free again, also when the test fails.
    let _port_lock = lock_listen_port(deadline);
    wait_for_free_port(&listen_addr, deadline);

    let mut server = ExampleServer::spawn();

    let listening = server.wait_for_line("the listening log line", |line| {
        line.contains("Server listening")
    });
    if !listening.contains(&format!("listen_addr: {listen_addr}")) {
        server.fail(&format!(
            "the server does not listen on {listen_addr}: {listening:?}"
        ));
    }

    let generated = server.wait_for_line("the spawn area log line", |line| {
        line.contains("spawn area chunks")
    });
    if !generated.contains("Generated 81 spawn area chunks") {
        server.fail(&format!(
            "a spawn chunk radius of 4 generates 9 x 9 = 81 chunks: {generated:?}"
        ));
    }

    // Startup is over. The server must keep running: its network thread must
    // accept a connection and its game loop must answer a login, which only
    // the game thread does.
    let probe = server.begin_login(&listen_addr);

    // Everything the server logged before answering the login is now in its
    // output pipe. Read up to a line logged after that answer, so the check
    // below sees all of it.
    let accepted = |line: &str| line.contains("Accepted new connection");
    server.wait_for_line("the login probe's connection log line", accepted);
    let marker = server.connect(&listen_addr);
    server.wait_for_line("the marker connection log line", accepted);

    server.assert_still_running();
    drop((probe, marker));

    if let Some(line) = server.lines.iter().find(|line| {
        line.contains("panicked") || line.contains(" ERROR ") || line.contains("FATAL")
    }) {
        let line = line.clone();
        server.fail(&format!(
            "the server reported a fatal error or panic: {line:?}"
        ));
    }
}

/// Takes the machine-wide lock that runs of this test hold while they use
/// `LISTEN_PORT`, waiting for another run to release it until `deadline`. The
/// lock lasts until the returned file is dropped, or the process ends.
fn lock_listen_port(deadline: Instant) -> File {
    let path = std::env::temp_dir().join(format!("voidmc-example-port-{LISTEN_PORT}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open the port lock {}: {error}", path.display()));
    loop {
        match file.try_lock() {
            Ok(()) => return file,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(PORT_POLL_INTERVAL)
            }
            Err(TryLockError::WouldBlock) => panic!(
                "another run of this test still holds the port lock {} after {PORT_WAIT_LIMIT:?}",
                path.display()
            ),
            Err(TryLockError::Error(error)) => {
                panic!("lock the port lock {}: {error}", path.display())
            }
        }
    }
}

/// Waits until `listen_addr` can be bound, failing at `deadline`. Binding then
/// closing at once leaves no connection behind to keep the port busy.
fn wait_for_free_port(listen_addr: &str, deadline: Instant) {
    loop {
        match TcpListener::bind(listen_addr) {
            Ok(_) => return,
            Err(_) if Instant::now() < deadline => thread::sleep(PORT_POLL_INTERVAL),
            Err(error) => panic!(
                "{listen_addr} is not available ({error}); stop whatever listens there \
                 (another voidmc-example?) and run the test again"
            ),
        }
    }
}

/// The example binary running in its own temporary directory, where it writes
/// `logs/` and `world/`. Dropping it kills the process and removes the
/// directory, also when the test fails.
struct ExampleServer {
    child: Child,
    runtime_dir: PathBuf,
    output: Receiver<String>,
    lines: Vec<String>,
}

impl ExampleServer {
    fn spawn() -> Self {
        let runtime_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("voidmc-example-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&runtime_dir);
        std::fs::create_dir_all(&runtime_dir).expect("create the runtime directory");

        let mut child = Command::new(env!("CARGO_BIN_EXE_voidmc-example"))
            .current_dir(&runtime_dir)
            // The default `info` filter is what prints the startup lines.
            .env_remove("RUST_LOG")
            // The default address, not an override from the caller's shell.
            .env_remove("VOID_LISTEN_ADDR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the voidmc-example binary");

        let (sender, output) = mpsc::channel();
        forward_lines(child.stdout.take().expect("piped stdout"), sender.clone());
        forward_lines(child.stderr.take().expect("piped stderr"), sender);

        Self {
            child,
            runtime_dir,
            output,
            lines: Vec::new(),
        }
    }

    /// Returns the first output line, ANSI colours removed, that `matches`,
    /// failing if the process exits or `WAIT_LIMIT` passes first.
    fn wait_for_line(&mut self, what: &str, matches: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + WAIT_LIMIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(line) => {
                    self.lines.push(line.clone());
                    if matches(&line) {
                        return line;
                    }
                }
                Err(RecvTimeoutError::Timeout) => self.fail(&format!(
                    "timed out after {WAIT_LIMIT:?} waiting for {what}"
                )),
                Err(RecvTimeoutError::Disconnected) => {
                    self.fail(&format!("the server closed its output before {what}"))
                }
            }
        }
    }

    fn connect(&mut self, listen_addr: &str) -> TcpStream {
        match TcpStream::connect(listen_addr) {
            Ok(stream) => stream,
            Err(error) => self.fail(&format!("connecting to {listen_addr} failed: {error}")),
        }
    }

    /// Connects like a player and sends a handshake and a login start. Returns
    /// the connection once the first login answer arrives: the compression
    /// setting or the login success, both sent when the game thread handles
    /// the login.
    fn begin_login(&mut self, listen_addr: &str) -> TcpStream {
        let mut stream = self.connect(listen_addr);
        stream
            .set_read_timeout(Some(WAIT_LIMIT))
            .expect("set a read timeout");

        let handshake = serverbound::HandshakePacket::Handshake(serverbound::Handshake {
            protocol_version: PROTOCOL_VERSION,
            server_address: LISTEN_HOST.to_string(),
            server_port: LISTEN_PORT,
            next_state: State::Login,
        });
        let login_start = serverbound::LoginPacket::LoginStart(serverbound::LoginStart {
            name: "StartupProbe".to_string(),
            uuid: uuid::Uuid::nil(),
        });
        let mut request = frame(&handshake);
        request.extend(frame(&login_start));
        if let Err(error) = stream.write_all(&request) {
            self.fail(&format!("sending the login failed: {error}"));
        }

        let answer = match read_frame(&mut stream) {
            Ok(answer) => answer,
            Err(error) => self.fail(&format!("the server did not answer the login: {error}")),
        };
        match clientbound::LoginPacket::decode(&mut answer.as_slice()) {
            Ok(
                clientbound::LoginPacket::SetCompression(_)
                | clientbound::LoginPacket::LoginSuccess(_),
            ) => stream,
            // Unreachable today; keeps the test failing, not silently passing,
            // if the login state gains a packet the server could send first.
            #[allow(unreachable_patterns)]
            Ok(other) => self.fail(&format!("unexpected login answer: {other:?}")),
            Err(error) => self.fail(&format!("undecodable login answer: {error:?}")),
        }
    }

    fn assert_still_running(&mut self) {
        match self.child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => self.fail(&format!("the server exited ({status}) after starting")),
            Err(error) => self.fail(&format!("querying the server process failed: {error}")),
        }
    }

    /// Stops the server, collects the rest of its output and fails the test
    /// with the whole transcript.
    fn fail(&mut self, message: &str) -> ! {
        let _ = self.child.kill();
        let status = self.child.wait().expect("wait for the server process");
        // The process is gone, so both pipes close and the channel drains.
        self.lines.extend(self.output.iter());
        panic!(
            "{message}\n--- voidmc-example output ({status}) ---\n{}",
            self.lines.join("\n")
        );
    }
}

impl Drop for ExampleServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

fn forward_lines(stream: impl Read + Send + 'static, sender: mpsc::Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if sender.send(strip_ansi(&line)).is_err() {
                break;
            }
        }
    });
}

/// Removes the colour escape sequences the console logger writes, so that
/// `listen_addr: 127.0.0.1:25565` reads as it does on a terminal.
fn strip_ansi(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
}

/// An uncompressed protocol frame: the packet length, then the packet.
fn frame<T: Encode>(packet: &T) -> Vec<u8> {
    let mut body = Vec::new();
    packet.encode(&mut body);
    let mut frame = Vec::new();
    VarI32(body.len() as i32).encode(&mut frame);
    frame.extend(body);
    frame
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut length = 0usize;
    for shift in (0..35).step_by(7) {
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        length |= usize::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            let mut body = vec![0; length];
            stream.read_exact(&mut body)?;
            return Ok(body);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "frame length is not a valid VarInt",
    ))
}
