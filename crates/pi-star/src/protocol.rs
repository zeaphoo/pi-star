//! 桌面壳可解析的启动事件。
//!
//! 标准输出只承载这些行；所有 tracing 日志都写标准错误。事件格式是对外契约，
//! 修改时必须同步 `app/src/supervisor.rs` 的解析与两侧单测。

use std::io::Write;
use std::net::SocketAddr;

use crate::web_assets::Descriptor;

/// stdout 事件前缀。
pub const PREFIX: &str = "pi-star: ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindFailure {
    AddressInUse,
    PermissionDenied,
    Other,
}

impl BindFailure {
    pub fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::AddrInUse => Self::AddressInUse,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AddressInUse => "address-in-use",
            Self::PermissionDenied => "permission-denied",
            Self::Other => "other",
        }
    }
}

pub fn assets_event(descriptor: &Descriptor) -> String {
    format!(
        "{PREFIX}assets source={} files={}",
        descriptor.source, descriptor.files
    )
}

pub fn listening_event(addr: SocketAddr) -> String {
    format!("{PREFIX}listening addr={addr}")
}

pub fn bind_failed_event(kind: BindFailure, addr: SocketAddr) -> String {
    format!("{PREFIX}bind-failed kind={} addr={addr}", kind.as_str())
}

/// 写一行事件并立即 flush。stdout 是行缓冲的，但显式 flush 让父进程能立刻拿到。
pub fn emit(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    #[test]
    fn events_have_stable_shape() {
        assert_eq!(
            listening_event(addr(30151)),
            "pi-star: listening addr=127.0.0.1:30151"
        );
        assert_eq!(
            bind_failed_event(BindFailure::AddressInUse, addr(30151)),
            "pi-star: bind-failed kind=address-in-use addr=127.0.0.1:30151"
        );
        assert_eq!(
            bind_failed_event(BindFailure::PermissionDenied, addr(80)),
            "pi-star: bind-failed kind=permission-denied addr=127.0.0.1:80"
        );
        assert_eq!(
            assets_event(&Descriptor {
                source: "embedded",
                path: None,
                files: 12,
            }),
            "pi-star: assets source=embedded files=12"
        );
    }

    #[test]
    fn bind_failure_classification() {
        let in_use = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let other = std::io::Error::from(std::io::ErrorKind::Other);
        assert_eq!(BindFailure::from_io(&in_use), BindFailure::AddressInUse);
        assert_eq!(BindFailure::from_io(&denied), BindFailure::PermissionDenied);
        assert_eq!(BindFailure::from_io(&other), BindFailure::Other);
    }
}
