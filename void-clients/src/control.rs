use std::fmt;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::condition::{self, DEFAULT_WAIT_MS};
use crate::{perf, registry, version};

/// Above the 10 s the probe gives the game thread: a stuck game answers with a failure, in time.
const STATE_LIMIT: Duration = Duration::from_secs(12);

/// What goes to a probe of any version, unchecked: a stale client stays visible and stoppable, and
/// gets its clock back if it knows the verb.
#[derive(Clone, Copy)]
pub enum Chore {
    State,
    Thumb,
    ShowFocus,
    Hide,
    Quit,
    ClockReal,
}

impl Chore {
    fn verb(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Thumb => "thumb",
            Self::ShowFocus => "show focus",
            Self::Hide => "hide",
            Self::Quit => "quit",
            Self::ClockReal => "clock real",
        }
    }
}

/// One line out, one JSON line back: the probe of a client, the step socket of a server, or a
/// test that stands for either.
pub trait Ask {
    fn ask(&mut self, line: &str) -> Result<Value, Failure>;
}

impl<T: Ask + ?Sized> Ask for &mut T {
    fn ask(&mut self, line: &str) -> Result<Value, Failure> {
        (**self).ask(line)
    }
}

/// Why a line brought back nothing to go on with.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// The peer answered, without `"ok":true`. The connection holds.
    Refused(Value),
    /// No answer to trust: the connection is dropped, so that a late one never reads as the next.
    Lost(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(reply) => match reply["error"].as_str() {
                Some(error) => f.write_str(error),
                None => write!(f, "{reply}"),
            },
            Self::Lost(error) => f.write_str(error),
        }
    }
}

impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        failure.to_string()
    }
}

/// A reply with `"ok":true` is an answer, any other a refusal.
pub fn judged(reply: Value) -> Result<Value, Failure> {
    if reply["ok"] == true {
        Ok(reply)
    } else {
        Err(Failure::Refused(reply))
    }
}

/// The reply, refused or not: for what reads the fields of any answer.
pub fn answered(result: Result<Value, Failure>) -> Result<Value, String> {
    match result {
        Ok(reply) | Err(Failure::Refused(reply)) => Ok(reply),
        Err(Failure::Lost(error)) => Err(error),
    }
}

fn one_verb(verb: &str) -> Result<(), String> {
    if verb.contains(['\n', '\r']) {
        return Err(format!("one verb at a time: {verb:?} holds a line break"));
    }
    Ok(())
}

struct Connection {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Connection {
    fn over(stream: UnixStream) -> Result<Self, String> {
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        Ok(Self { stream, reader })
    }

    fn to_socket(who: &str, path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("{who}: {e}"))?;
        Self::over(stream)
    }

    fn to_client(name: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(registry::socket(name)?)
            .map_err(|e| format!("client {name} does not answer: {e}"))?;
        Self::over(stream)
    }

    /// The line as the peer wrote it, for what shows it as is.
    fn line(&mut self, who: &str, verb: &str, timeout: Duration) -> Result<String, Failure> {
        let closed = |e| Failure::Lost(format!("{who} closed the connection: {e}"));
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(closed)?;
        writeln!(self.stream, "{verb}").map_err(closed)?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| match e.kind() {
                ErrorKind::WouldBlock | ErrorKind::TimedOut => Failure::Lost(format!(
                    "{who} does not answer: no reply to {verb} within {timeout:?}"
                )),
                _ => Failure::Lost(format!("{who}, on {verb}: {e}")),
            })?;
        if line.is_empty() {
            return Err(Failure::Lost(format!("{who} closed the connection")));
        }
        Ok(line.trim_end().to_string())
    }

    fn exchange(&mut self, who: &str, verb: &str, timeout: Duration) -> Result<Value, Failure> {
        let line = self.line(who, verb, timeout)?;
        let reply = serde_json::from_str(&line)
            .map_err(|e| Failure::Lost(format!("{who} answered {verb} with no JSON: {e}")))?;
        judged(reply)
    }
}

/// Where a connection that lost a line comes back from.
enum Reopen {
    Never,
    Client(String),
    Socket(PathBuf),
}

/// The one client of the line protocol. A connection that loses a line is dropped, and the next
/// line opens another, checked again when it goes to a client.
pub struct Probe {
    who: String,
    reopen: Reopen,
    connection: Option<Connection>,
    limits: Limits,
}

impl Probe {
    /// A peer that is not a client, on a stream that is never reopened.
    #[cfg(test)]
    pub fn over(who: &str, stream: UnixStream, limits: Limits) -> Result<Self, String> {
        Ok(Self {
            who: who.to_string(),
            reopen: Reopen::Never,
            connection: Some(Connection::over(stream)?),
            limits,
        })
    }

    /// The step socket of a server: a lost line leaves the next one a new connection, so that a
    /// `stop` still reaches it.
    pub fn to_socket(who: &str, path: &Path, limits: Limits) -> Result<Self, String> {
        Ok(Self {
            who: who.to_string(),
            reopen: Reopen::Socket(path.to_path_buf()),
            connection: Some(Connection::to_socket(who, path)?),
            limits,
        })
    }

    /// The only way to drive a client: a probe of another version than this voidc is refused.
    pub fn open(name: &str, limits: Limits) -> Result<Self, String> {
        Ok(Self {
            who: format!("client {name}"),
            reopen: Reopen::Client(name.to_string()),
            connection: Some(Self::checked(name, limits)?),
            limits,
        })
    }

    fn checked(name: &str, limits: Limits) -> Result<Connection, String> {
        let mut connection = Connection::to_client(name)?;
        let who = format!("client {name}");
        let state = answered(connection.exchange(&who, "state", limits.open))?;
        version::check(name, &state)?;
        Ok(connection)
    }

    fn unchecked(name: &str) -> Result<Self, String> {
        Ok(Self {
            who: format!("client {name}"),
            reopen: Reopen::Never,
            connection: Some(Connection::to_client(name)?),
            limits: Limits::REAL,
        })
    }

    pub fn ask_within(&mut self, verb: &str, timeout: Duration) -> Result<Value, Failure> {
        self.through(verb, |connection, who| {
            connection.exchange(who, verb, timeout)
        })
    }

    /// The line itself, read by no one on the way: for what shows it, or needs no more than an
    /// answer.
    fn line_within(&mut self, verb: &str, timeout: Duration) -> Result<String, Failure> {
        self.through(verb, |connection, who| connection.line(who, verb, timeout))
    }

    fn through<T>(
        &mut self,
        verb: &str,
        exchange: impl FnOnce(&mut Connection, &str) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        one_verb(verb).map_err(Failure::Lost)?;
        let mut connection = match (self.connection.take(), &self.reopen) {
            (Some(connection), _) => connection,
            (None, Reopen::Client(name)) => {
                Self::checked(name, self.limits).map_err(Failure::Lost)?
            }
            (None, Reopen::Socket(path)) => {
                Connection::to_socket(&self.who, path).map_err(Failure::Lost)?
            }
            (None, Reopen::Never) => {
                return Err(Failure::Lost(format!(
                    "{} dropped the connection",
                    self.who
                )));
            }
        };
        let result = exchange(&mut connection, &self.who);
        if !matches!(result, Err(Failure::Lost(_))) {
            self.connection = Some(connection);
        }
        result
    }

    #[cfg(test)]
    pub fn read_timeout(&self) -> Option<Duration> {
        let connection = self.connection.as_ref()?;
        connection.stream.read_timeout().ok().flatten()
    }
}

impl Ask for Probe {
    fn ask(&mut self, line: &str) -> Result<Value, Failure> {
        let timeout = self.limits.of(line);
        self.ask_within(line, timeout)
    }
}

/// Sends one verb to a client whose probe is of this version, and brings back its reply as the
/// probe wrote it.
pub fn ask_line(name: &str, verb: &str) -> Result<String, String> {
    one_verb(verb)?;
    let line = Probe::open(name, Limits::REAL)?.line_within(verb, Limits::REAL.of(verb))?;
    Ok(line)
}

/// A chore needs no check of the version.
pub fn chore(name: &str, chore: Chore, timeout: Duration) -> Result<Value, Failure> {
    Probe::unchecked(name)
        .map_err(Failure::Lost)?
        .ask_within(chore.verb(), timeout)
}

/// The line of a chore as the probe wrote it.
pub fn chore_line(name: &str, chore: Chore, timeout: Duration) -> Result<String, String> {
    let line = Probe::unchecked(name)?.line_within(chore.verb(), timeout)?;
    Ok(line)
}

/// How long a client gets. A test takes a smaller set, so that it does not wait for the real one.
#[derive(Clone, Copy)]
pub struct Limits {
    pub open: Duration,
    verb: Duration,
    past_a_wait: Duration,
}

impl Limits {
    pub const REAL: Self = Self {
        open: STATE_LIMIT,
        verb: Duration::from_secs(30),
        past_a_wait: Duration::from_secs(5),
    };

    /// The real ones divided by 20.
    #[cfg(test)]
    pub const TEST: Self = Self {
        open: Duration::from_millis(600),
        verb: Duration::from_millis(1500),
        past_a_wait: Duration::from_millis(250),
    };

    /// How long a verb may take: a `wait_until` carries its own limit.
    pub fn of(self, verb: &str) -> Duration {
        let words: Vec<&str> = verb.split_whitespace().collect();
        if words.first() == Some(&"perf") {
            return perf::timeout_of(&words);
        }
        let limit = match condition::split(verb) {
            ("wait_until", rest) => condition::wait(rest).map_or(DEFAULT_WAIT_MS, |(_, ms)| ms),
            _ => return self.verb,
        };
        Duration::from_millis(limit) + self.past_a_wait
    }
}

#[cfg(test)]
pub mod fake;

#[cfg(test)]
mod tests;
