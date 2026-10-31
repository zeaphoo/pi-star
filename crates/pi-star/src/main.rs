mod app;
mod auth;
mod debug_streams;
#[cfg(test)]
mod integration_tests;
mod pi_rpc;
mod pi_runtime;
mod protocol;
mod routes;
mod state;
mod web_assets;

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tokio_util::sync::CancellationToken;

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
    /// Open the URL in the default browser after startup.
    #[arg(long = "open", env = "PI_STAR_OPEN_BROWSER")]
    open: bool,
    /// Deprecated: the browser is no longer opened by default.
    #[arg(long = "no-open", hide = true)]
    _no_open: bool,
    /// Directory containing index.html and assets.
    ///
    /// Omit it to use embedded assets in release builds and `ui/dist` in debug
    /// builds. Supplying it forces disk assets in every profile.
    #[arg(long = "ui-dir", env = "PI_STAR_UI_DIR")]
    ui_dir: Option<PathBuf>,
    /// Exit gracefully when stdin reaches EOF (used by the desktop shell).
    #[arg(long = "exit-on-stdin-eof")]
    exit_on_stdin_eof: bool,
    /// pi agent directory (default $PI_CODING_AGENT_DIR or ~/.pi/agent).
    #[arg(long = "agent-dir")]
    agent_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        port,
        hostname,
        open,
        ui_dir,
        exit_on_stdin_eof,
        agent_dir,
        ..
    } = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pi_star=info,tower_http=warn".into()),
        )
        // stdout is reserved for machine-readable lifecycle events.
        .with_writer(std::io::stderr)
        .init();

    // Prefer the bundled Node + pi package when the desktop installer ships it.
    pi_runtime::apply_env();

    let host = hostname
        .parse::<std::net::IpAddr>()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let addr = SocketAddr::new(host, port);
    let state = state::AppState::new(agent_dir, ui_dir, &hostname).await?;
    let assets = state.web_assets.describe();
    protocol::emit(&protocol::assets_event(&assets));
    if assets.files == 0 {
        tracing::warn!(source = assets.source, "frontend assets are empty");
    }
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
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            let kind = protocol::BindFailure::from_io(&error);
            protocol::emit(&protocol::bind_failed_event(kind, addr));
            tracing::error!(%addr, "failed to bind: {error}");
            std::process::exit(2);
        }
    };
    let actual = listener.local_addr()?;
    state.set_server_port(actual.port());
    protocol::emit(&protocol::listening_event(actual));
    tracing::info!("Pi Star is running at http://{actual}");
    if open {
        open_browser(&format!("http://{actual}"));
    }

    // Graceful shutdown must not wait forever for long-lived SSE streams
    // (agent events, terminal, file watch): axum waits for every connection to
    // finish, and those streams stay open until the browser closes them.
    let shutdown_token = CancellationToken::new();
    {
        let token = shutdown_token.clone();
        tokio::spawn(async move {
            wait_for_shutdown_signal().await;
            tracing::info!("shutdown requested; closing connections");
            token.cancel();
        });
    }
    if exit_on_stdin_eof {
        let token = shutdown_token.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut stdin = tokio::io::stdin();
            let mut buffer = [0u8; 256];
            loop {
                match stdin.read(&mut buffer).await {
                    Ok(0) => {
                        tracing::info!("stdin closed; shutting down");
                        token.cancel();
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "stdin read failed; shutting down");
                        token.cancel();
                        break;
                    }
                }
            }
        });
    }
    let graceful = {
        let token = shutdown_token.clone();
        async move { token.cancelled().await }
    };
    let serve = axum::serve(listener, app)
        .with_graceful_shutdown(graceful)
        .into_future();
    tokio::pin!(serve);
    let result = tokio::select! {
        result = &mut serve => result,
        _ = async {
            shutdown_token.cancelled().await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        } => {
            tracing::warn!("graceful shutdown timed out; terminating open streams");
            Ok(())
        }
    };
    state.shutdown();
    result?;
    Ok(())
}

async fn wait_for_shutdown_signal() {
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
