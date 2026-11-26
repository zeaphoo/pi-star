//! Run a child process with a deadline while keeping its pipes drained.
//!
//! `Command::output()` has no timeout: a `git` process stuck on an iCloud or
//! network volume blocks its caller (and a blocking-pool thread) forever. This
//! helper polls for exit and kills the child at the deadline.
//!
//! stdout/stderr are read on their own threads. Waiting for `try_wait` without
//! draining would deadlock a command whose output exceeds the pipe buffer
//! (large `git diff`/`git show`), and it would then be killed by the deadline.

use std::io::{self, Read};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(20);

/// Spawn `command` (stdin is closed) and wait up to `timeout`.
///
/// Returns `ErrorKind::TimedOut` after killing the child when the deadline
/// passes. The child is always reaped.
pub fn output_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    let stdout = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = pipe.read_to_end(&mut buffer);
            buffer
        })
    });
    let stderr = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = pipe.read_to_end(&mut buffer);
            buffer
        })
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                // Do not join the reader threads here: the command's own
                // children (a hook, fsmonitor, a shell's `sleep`) may still
                // hold the pipe open, and joining would wait past the
                // deadline. The detached readers end when the pipe closes.
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("command timed out after {timeout:?}"),
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    };

    // Both pipes close when the (reaped) child is done; joining cannot hang
    // here because there is no deadline left to overshoot.
    let stdout = stdout
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    #[cfg(unix)]
    #[test]
    fn kills_a_command_that_exceeds_the_deadline() {
        let started = Instant::now();
        let error = output_with_timeout(
            Command::new("sh").args(["-c", "sleep 10"]),
            Duration::from_millis(200),
        )
        .expect_err("must time out");
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "killed promptly"
        );
    }

    #[cfg(unix)]
    #[test]
    fn drains_output_larger_than_the_pipe_buffer() {
        // 256 KiB; a non-draining wait would block until the timeout kills it.
        let output = output_with_timeout(
            Command::new("sh").args(["-c", "head -c 262144 /dev/zero"]),
            Duration::from_secs(10),
        )
        .expect("must complete");
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 262_144);
    }

    #[cfg(unix)]
    #[test]
    fn passes_through_status_and_streams() {
        let output = output_with_timeout(
            Command::new("sh").args(["-c", "printf out; printf err >&2; exit 3"]),
            Duration::from_secs(10),
        )
        .expect("must complete");
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
    }
}
