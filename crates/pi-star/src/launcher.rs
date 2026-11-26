//! Spawning short-lived system launchers without leaking zombies.
//!
//! `open` / `xdg-open` / `explorer` exit as soon as the OS has handed the
//! request off. Dropping their `std::process::Child` without `wait()` leaves a
//! zombie (`<defunct>`) entry in the process table for the lifetime of this
//! server. On macOS those entries show up in Activity Monitor and cannot be
//! killed from there — only reaping the parent clears them, and the desktop
//! shell keeps the server alive while it sits in the tray.

use std::process::Command;

/// Spawn `command` and reap it on a background thread.
pub fn spawn_and_reap(mut command: Command) -> std::io::Result<()> {
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
