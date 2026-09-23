mod content;
mod directory;
mod lobby;
mod session;
#[cfg(test)]
mod tests;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use anyhow::Result;
use axum::Router;
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use axum::serve::ListenerExt;
use clap::Parser;
use space_race_protocol::{DEFAULT_PORT, MAX_MESSAGE_SIZE, WS_PATH};
use tokio::net::TcpListener;
use tracing::{Instrument, info, info_span, warn};

use crate::content::CarTuningFile;
use crate::directory::Directory;
use crate::lobby::Timings;

#[derive(Parser)]
struct Args {
    /// Game data directory, holding `car.ron` and `tracks/`.
    #[arg(long, default_value = "server/data")]
    data_dir: PathBuf,
    /// Port to listen on, on the loopback address. The deployment machine already runs another
    /// game on the default one (see docs/deploy.md).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
}

/// Plain text and loopback only: in production, the reverse proxy terminates TLS and forwards here.
fn listen_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs at `info` unless `RUST_LOG` says otherwise, e.g. `RUST_LOG=debug`.
    space_race_logger::init();
    let args = Args::parse();

    let tracks = content::load_tracks(&args.data_dir)?;
    for (key, loaded) in &tracks {
        info!(
            key,
            name = loaded.track.name(),
            length = %format_args!("{:.1} m", loaded.track.length()),
            "track loaded"
        );
    }
    let (tuning_file, tuning) =
        CarTuningFile::load(args.data_dir.join(content::CAR_TUNING_FILE), &tracks)?;
    let directory = Directory::spawn(tracks, tuning_file.watch(tuning), Timings::default());

    let listener = TcpListener::bind(listen_addr(args.port)).await?;
    info!(addr = %listener.local_addr()?, path = WS_PATH, "server listening");
    // Send small messages at once instead of batching them: a snapshot held back by Nagle's
    // algorithm is a snapshot the player sees late.
    let listener = listener.tap_io(|stream| {
        if let Err(error) = stream.set_nodelay(true) {
            warn!("cannot disable Nagle's algorithm: {error}");
        }
    });

    axum::serve(listener, router(directory))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown requested");
        })
        .await?;

    info!("server stopped");
    Ok(())
}

fn router(directory: Directory) -> Router {
    Router::new()
        .route(WS_PATH, get(upgrade))
        .with_state(directory)
}

async fn upgrade(ws: WebSocketUpgrade, State(directory): State<Directory>) -> Response {
    ws.max_message_size(MAX_MESSAGE_SIZE).on_upgrade(|socket| {
        async move {
            if let Err(error) = session::run(socket, directory).await {
                warn!("session failed: {error:#}");
            }
        }
        .instrument(info_span!("session"))
    })
}
