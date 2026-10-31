mod app;
mod auth;
mod debug_streams;
#[cfg(test)]
mod integration_tests;
mod pi_rpc;
mod routes;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "pi-star",
    version,
    about = "Rust/Axum web UI for the pi coding agent"
)]
struct Cli {
    /// Port to listen on (`PORT`, default 30151).
    #[arg(short = 'p', long, env = "PORT", default_value_t = 30151)]
    port: u16,
    /// Hostname to bind (`PI_WEB_HOSTNAME`, default 127.0.0.1).
    #[arg(
        short = 'H',
        long = "hostname",
        env = "PI_WEB_HOSTNAME",
        default_value = "127.0.0.1"
    )]
    hostname: String,
    /// Do not open a browser after startup.
    #[arg(long = "no-open", env = "PI_WEB_NO_OPEN")]
    no_open: bool,
    /// Directory containing index.html and assets (default ui/dist).
    #[arg(long = "ui-dir", default_value = "ui/dist")]
    ui_dir: PathBuf,
    /// pi agent directory (default $PI_CODING_AGENT_DIR or ~/.pi/agent).
    #[arg(long = "agent-dir")]
    agent_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pi_star=info,tower_http=warn".into()),
        )
        .init();

    let host = cli
        .hostname
        .parse::<std::net::IpAddr>()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let addr = SocketAddr::new(host, cli.port);
    let state = state::AppState::new(cli.agent_dir, cli.ui_dir, &cli.hostname).await?;
    let app = app::router(state.clone());
    let idle_timeout_ms = std::env::var("PI_WEB_IDLE_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(600_000);
    if idle_timeout_ms > 0 {
        let reaper = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let mut reaped = reaper
                    .agents
                    .reap_idle(std::time::Duration::from_millis(idle_timeout_ms));
                reaped.extend(
                    reaper
                        .pi_rpc
                        .reap_idle(std::time::Duration::from_millis(idle_timeout_ms)),
                );
                for id in reaped {
                    tracing::info!(session = %id, "closed an idle agent session");
                }
            }
        });
    }
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    let actual = listener.local_addr()?;
    state.set_server_port(actual.port());
    tracing::info!("pi-star listening on http://{actual}");
    if !cli.no_open {
        open_browser(&format!("http://{actual}"));
    }

    let shutdown = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c().await.ok();
        };
        #[cfg(unix)]
        let terminate = async {
            if let Ok(mut signal) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                signal.recv().await;
            }
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    state.shutdown();
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let command = ("open", vec![url]);
    #[cfg(target_os = "windows")]
    let command = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let command = ("xdg-open", vec![url]);
    let _ = std::process::Command::new(command.0)
        .args(command.1)
        .spawn();
}
