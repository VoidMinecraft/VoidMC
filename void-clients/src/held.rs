use std::io::{BufRead, BufReader, Result};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, PoisonError};

/// On macOS `pipe` and `set_cloexec` are two steps: two children started at once could each hold
/// the stdin of the other, and neither would ever see it close.
static SPAWN: Mutex<()> = Mutex::new(());

/// A long-lived child of a test, and the only way to start one: it leads its own process group,
/// the whole group dies with this value, and the open stdin ends it if the tests are killed.
///
/// A group outlives its leader, but only an unreaped leader keeps its id: every wait goes through
/// here, so that nothing is signalled once the id may belong to someone else.
pub struct Held {
    child: Child,
    reaped: bool,
}

fn kill_group(leader: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{leader}")])
        .stderr(Stdio::null())
        .status();
}

impl Held {
    pub fn launch(command: &mut Command) -> Self {
        let one_at_a_time = SPAWN.lock().unwrap_or_else(PoisonError::into_inner);
        let child = command.process_group(0).stdin(Stdio::piped()).spawn();
        drop(one_at_a_time);
        let child = child.unwrap();
        Self {
            child,
            reaped: false,
        }
    }

    /// One process, which `ps` shows with `title` among its words, until its stdin closes. A
    /// builtin of the shell, and no `arg0`: a multi-call `cat` would take it for its own name.
    pub fn waiting(title: &str) -> Self {
        Self::launch(
            Command::new("sh")
                .args(["-c", "read line; exit 0", title])
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Closes its stdin, then waits. What it has started stays: only `stop` takes the group.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        let status = self.child.wait();
        self.reaped = true;
        status
    }

    /// Lends the child to what ends it on its own, and notes whether it did.
    pub fn ended_by(&mut self, end: impl FnOnce(&mut Child)) -> Option<ExitStatus> {
        end(&mut self.child);
        let status = self.child.try_wait().ok().flatten();
        self.reaped = status.is_some();
        status
    }

    pub fn stop(&mut self) {
        self.stop_with(kill_group);
    }

    fn stop_with(&mut self, kill: impl FnOnce(u32)) {
        if self.reaped {
            return;
        }
        kill(self.id());
        let _ = self.wait();
    }

    pub fn first_line(&mut self) -> String {
        let out = self.child.stdout.take().expect("stdout is piped");
        let mut line = String::new();
        BufReader::new(out).read_line(&mut line).unwrap();
        line.trim_end().to_string()
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use super::*;

    fn gone(pid: &str) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let stat = Command::new("ps")
                .args(["-o", "stat=", "-p", pid])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&stat.stdout);
            if stat.trim().is_empty() || stat.trim().starts_with('Z') {
                return true;
            }
            sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_stop_takes_the_grandchildren_with_it() {
        let mut shell = Held::launch(
            Command::new("sh")
                .args(["-c", "sleep 30 & echo $!; wait"])
                .stdout(Stdio::piped()),
        );
        let grandchild = shell.first_line();
        shell.stop();
        assert!(gone(&grandchild), "{grandchild} outlived its group");
    }

    #[test]
    fn a_second_stop_does_nothing() {
        let mut child = Held::waiting("held-twice");
        child.stop();
        child.stop_with(|_| panic!("the leader is already reaped"));
        assert_eq!(child.wait().unwrap().signal(), Some(9));
    }

    #[test]
    fn what_waits_ends_with_its_stdin() {
        let mut child = Held::waiting("held-stdin");
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn nothing_is_signalled_once_a_wait_has_reaped_the_leader() {
        let mut child = Held::waiting("held-reaped");
        child.wait().unwrap();
        child.stop_with(|_| panic!("the id may be someone else's"));
    }

    #[test]
    fn nothing_is_signalled_once_what_ended_the_child_has_reaped_it() {
        let mut child = Held::waiting("held-lent");
        let status = child.ended_by(|child| {
            child.kill().unwrap();
            child.wait().unwrap();
        });
        assert_eq!(status.and_then(|status| status.signal()), Some(9));
        child.stop_with(|_| panic!("the id may be someone else's"));
    }

    #[test]
    fn a_child_still_running_after_it_was_lent_is_still_stopped() {
        let mut child = Held::waiting("held-still-lent");
        assert!(child.ended_by(|_| {}).is_none());
        let mut signalled = false;
        child.stop_with(|_| signalled = true);
        assert!(signalled);
    }

    #[test]
    fn a_refused_kill_does_not_leave_the_stop_waiting() {
        let mut child = Held::waiting("held-unkilled");
        child.stop_with(|_| {});
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn what_waits_goes_by_its_title() {
        let child = Held::waiting("held-title -Dvoid.client=x y");
        let shown = Command::new("ps")
            .args(["-o", "command=", "-p", &child.id().to_string()])
            .output()
            .unwrap();
        let shown = String::from_utf8_lossy(&shown.stdout);
        assert!(shown.contains("-Dvoid.client=x "), "{shown}");
    }
}
