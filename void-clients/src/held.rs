use std::io::{BufRead, BufReader};
use std::ops::{Deref, DerefMut};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

/// A long-lived child of a test, and the only way to start one: it leads its own process group,
/// the whole group dies with this value, and the open stdin ends it if the tests are killed.
pub struct Held {
    child: Child,
    stopped: bool,
}

impl Held {
    pub fn launch(command: &mut Command) -> Self {
        let child = command
            .process_group(0)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            child,
            stopped: false,
        }
    }

    /// One process and no shell, which `ps` shows under `title`, until its stdin closes.
    pub fn waiting(title: &str) -> Self {
        Self::launch(
            Command::new("cat")
                .arg0(title)
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
    }

    /// Before the wait: a group outlives its leader, but only an unreaped leader keeps its id.
    pub fn stop(&mut self) {
        if std::mem::replace(&mut self.stopped, true) {
            return;
        }
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", self.child.id())])
            .stderr(Stdio::null())
            .status();
        let _ = self.child.wait();
    }

    pub fn first_line(&mut self) -> String {
        let out = self.child.stdout.take().expect("stdout is piped");
        let mut line = String::new();
        BufReader::new(out).read_line(&mut line).unwrap();
        line.trim_end().to_string()
    }
}

impl Deref for Held {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for Held {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
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
        child.stop();
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn what_waits_ends_with_its_stdin() {
        let mut child = Held::waiting("held-stdin");
        drop(child.stdin.take());
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
