use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tokio::sync::broadcast;

const BACKLOG_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalEvent {
    Output {
        data: String,
        offset: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        reset: Option<bool>,
    },
    Exit {
        #[serde(rename = "exitCode")]
        exit_code: i32,
    },
    Closed,
}

struct TerminalRecord {
    cwd: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send>>,
    backlog: Mutex<String>,
    offset: Mutex<usize>,
    exit_code: Mutex<Option<i32>>,
    sender: broadcast::Sender<TerminalEvent>,
    exited: Mutex<bool>,
}

pub struct TerminalSubscription {
    pub output: TerminalEvent,
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub receiver: broadcast::Receiver<TerminalEvent>,
}

pub struct TerminalManager {
    terminals: Mutex<HashMap<String, Arc<TerminalRecord>>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(&self, cwd: &Path, cols: u16, rows: u16, id: Option<String>) -> Result<String> {
        let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: rows.max(2),
                cols: cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| anyhow!("open pty: {error}"))?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(windows) {
                "powershell.exe".to_string()
            } else {
                "/bin/sh".to_string()
            }
        });
        let mut command = CommandBuilder::new(&shell);
        if !cfg!(windows) {
            command.arg("-l");
        }
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()),
        );
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| anyhow!("spawn shell: {error}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| anyhow!("pty reader: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| anyhow!("pty writer: {error}"))?;
        let (sender, _) = broadcast::channel(256);
        let record = Arc::new(TerminalRecord {
            cwd: cwd.to_string_lossy().into_owned(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            backlog: Mutex::new(String::new()),
            offset: Mutex::new(0),
            exit_code: Mutex::new(None),
            sender: sender.clone(),
            exited: Mutex::new(false),
        });
        {
            let record = record.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buffer[..n]).into_owned();
                            let end = {
                                let mut offset = record.offset.lock();
                                *offset += data.len();
                                *offset
                            };
                            {
                                let mut backlog = record.backlog.lock();
                                backlog.push_str(&data);
                                if backlog.len() > BACKLOG_LIMIT {
                                    let excess = backlog.len() - BACKLOG_LIMIT;
                                    let cut = backlog
                                        .char_indices()
                                        .find(|(index, _)| *index >= excess)
                                        .map(|(index, _)| index)
                                        .unwrap_or(backlog.len());
                                    backlog.drain(..cut);
                                }
                            }
                            let _ = sender.send(TerminalEvent::Output {
                                data,
                                offset: end,
                                reset: None,
                            });
                        }
                        Err(_) => break,
                    }
                }
                // The exit watcher owns the end of a shell: it sends Exit with
                // the code. Sending Closed here would race it, and a stream
                // that saw Closed first reported "Terminal closed" instead.
            });
        }
        {
            let record = record.clone();
            std::thread::spawn(move || {
                // Poll instead of holding the child lock through `wait()`:
                // `kill()` needs that lock, and a shell can outlive its
                // stream by hours.
                let code = loop {
                    let status = {
                        let mut child = record.child.lock();
                        child.try_wait()
                    };
                    match status {
                        Ok(Some(status)) => break status.exit_code() as i32,
                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                        Err(_) => break -1,
                    }
                };
                *record.exit_code.lock() = Some(code);
                *record.exited.lock() = true;
                let _ = record.sender.send(TerminalEvent::Exit { exit_code: code });
            });
        }
        self.terminals.lock().insert(id.clone(), record);
        Ok(id)
    }

    pub fn cwd(&self, id: &str) -> Option<String> {
        self.terminals
            .lock()
            .get(id)
            .map(|record| record.cwd.clone())
    }

    pub fn has(&self, id: &str) -> bool {
        self.terminals.lock().contains_key(id)
    }

    pub fn write(&self, id: &str, data: &str) -> bool {
        let Some(record) = self.terminals.lock().get(id).cloned() else {
            return false;
        };
        let mut writer = record.writer.lock();
        writer.write_all(data.as_bytes()).is_ok() && writer.flush().is_ok()
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> bool {
        let Some(record) = self.terminals.lock().get(id).cloned() else {
            return false;
        };
        record
            .master
            .lock()
            .resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .is_ok()
    }

    pub fn subscribe(&self, id: &str) -> Option<TerminalSubscription> {
        let record = self.terminals.lock().get(id).cloned()?;
        let backlog = record.backlog.lock().clone();
        let offset = *record.offset.lock();
        let receiver = record.sender.subscribe();
        let exited = *record.exited.lock();
        let exit_code = *record.exit_code.lock();
        Some(TerminalSubscription {
            output: TerminalEvent::Output {
                data: backlog,
                offset,
                reset: None,
            },
            exited,
            exit_code,
            receiver,
        })
    }

    pub fn kill(&self, id: &str) -> bool {
        let Some(record) = self.terminals.lock().remove(id) else {
            return false;
        };
        let _ = record.child.lock().kill();
        let _ = record.sender.send(TerminalEvent::Closed);
        true
    }

    pub fn kill_all(&self) {
        let ids: Vec<String> = self.terminals.lock().keys().cloned().collect();
        for id in ids {
            self.kill(&id);
        }
    }

    pub fn count(&self) -> usize {
        self.terminals.lock().len()
    }

    /// Shells whose process has exited but whose backlog can still be read.
    pub fn exited_count(&self) -> usize {
        self.terminals
            .lock()
            .values()
            .filter(|record| *record.exited.lock())
            .count()
    }
}
