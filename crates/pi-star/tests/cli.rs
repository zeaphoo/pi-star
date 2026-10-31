//! 二进制级测试：启动事件、绑定失败、stdin watchdog。
//!
//! 这些行为只在真实子进程中成立，单元测试覆盖不到，因此放在 `tests/` 下使用
//! `CARGO_BIN_EXE_pi-star`。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("child")
    }

    fn wait_timeout(&mut self, timeout: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child().try_wait().expect("try_wait") {
                Some(status) => return Some(status),
                None if Instant::now() >= deadline => return None,
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn(args: &[&str]) -> (ChildGuard, mpsc::Receiver<String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pi-star"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pi-star");

    let stdout = child.stdout.take().expect("stdout");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    (ChildGuard::new(child), receiver)
}

fn next_event(receiver: &mpsc::Receiver<String>, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok(line) => {
                assert!(
                    line.starts_with("pi-star: "),
                    "stdout must only contain lifecycle events: {line}"
                );
                return line;
            }
            Err(error) => panic!("timed out waiting for pi-star event: {error}"),
        }
    }
}

fn event_until(receiver: &mpsc::Receiver<String>, prefix: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = next_event(receiver, left);
        if line.starts_with(prefix) {
            return line;
        }
    }
}

fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    response
}

fn fixture_args(agent: &std::path::Path, ui: &std::path::Path, port: &str) -> Vec<String> {
    vec![
        "--hostname".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--agent-dir".to_string(),
        agent.display().to_string(),
        "--ui-dir".to_string(),
        ui.display().to_string(),
        "--exit-on-stdin-eof".to_string(),
    ]
}

#[test]
fn port_zero_reports_actual_address_and_stdin_eof_shuts_down() {
    let temp = tempfile::tempdir().expect("temp");
    let agent = temp.path().join("agent");
    let ui = temp.path().join("ui");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("index.html"), "<title>fixture</title>").unwrap();

    let args = fixture_args(&agent, &ui, "0");
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (mut child, receiver) = spawn(&args);

    let assets = event_until(&receiver, "pi-star: assets ", Duration::from_secs(20));
    assert!(assets.contains("source=disk"), "{assets}");

    let listening = event_until(&receiver, "pi-star: listening ", Duration::from_secs(20));
    let addr_text = listening
        .strip_prefix("pi-star: listening addr=")
        .expect("addr");
    let addr: SocketAddr = addr_text.parse().expect("parse addr");
    assert_ne!(addr.port(), 0);

    let response = http_get(addr, "/api/health");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"name\":\"pi-star\""), "{response}");
    assert!(response.contains("\"source\":\"disk\""), "{response}");
    assert!(response.contains("\"files\":"), "{response}");

    // Dropping the pipe is the desktop shell's liveness signal.
    drop(child.child().stdin.take());
    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("pi-star should exit after stdin EOF");
    assert!(status.success(), "{status}");

    // The port must be free again.
    TcpListener::bind(addr).expect("port released");
}

#[test]
fn occupied_port_emits_bind_failed_and_exits_two() {
    let temp = tempfile::tempdir().expect("temp");
    let agent = temp.path().join("agent");
    let ui = temp.path().join("ui");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("index.html"), "<title>fixture</title>").unwrap();

    let occupied = TcpListener::bind("127.0.0.1:0").expect("occupy");
    let port = occupied.local_addr().unwrap().port().to_string();

    let args = fixture_args(&agent, &ui, &port);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (mut child, receiver) = spawn(&args);

    let event = event_until(&receiver, "pi-star: bind-failed ", Duration::from_secs(20));
    assert!(event.contains("kind=address-in-use"), "{event}");
    assert!(event.contains(&format!("addr=127.0.0.1:{port}")), "{event}");

    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("pi-star should exit after bind failure");
    assert_eq!(status.code(), Some(2));
}
