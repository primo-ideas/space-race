//! Lifecycle of a player connection: handshake (version, nickname, authentication), then browsing
//! lobbies and playing in them until the player leaves.

use std::fmt;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use space_race_protocol::auth::{self, PublicKey};
use space_race_protocol::{
    self as protocol, ClientMessage, PROTOCOL_VERSION, RejectReason, ServerMessage,
    is_valid_nickname,
};
use tokio::time::timeout;
use tracing::info;

use crate::directory::{Directory, OnlineGuard};
use crate::lobby::Membership;

/// Time given to the client to complete the handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Persistent player identity: their public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(pub PublicKey);

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The first 8 bytes are enough to tell players apart in logs.
        self.0[..8]
            .iter()
            .try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    pub id: PlayerId,
    pub nickname: String,
}

pub async fn run(mut socket: WebSocket, directory: Directory) -> Result<()> {
    let handshake = timeout(HANDSHAKE_TIMEOUT, handshake(&mut socket))
        .await
        .context("handshake timed out")??;
    let Some(player) = handshake else {
        return Ok(());
    };
    // Held until the connection ends, however it ends. It also carries where the player is, for
    // the list of connected players.
    let Some(online) = directory.connect(&player) else {
        return reject(&mut socket, RejectReason::AlreadyConnected).await;
    };

    info!(player = %player.id, nickname = %player.nickname, "player connected");
    // From here on, a dropped connection (crash, lost network) is an ordinary way to leave, not a
    // server failure: it is logged as the reason.
    let reason = connected(&mut socket, &directory, &player, &online).await;
    info!(player = %player.id, nickname = %player.nickname, %reason, "player disconnected");
    Ok(())
}

/// Returns `None` if the player was rejected.
async fn handshake(socket: &mut WebSocket) -> Result<Option<Player>> {
    let Some(ClientMessage::Hello {
        protocol_version,
        nickname,
        public_key,
    }) = receive(socket).await?
    else {
        bail!("expected Hello");
    };

    if protocol_version != PROTOCOL_VERSION {
        let reason = RejectReason::ProtocolMismatch {
            server_version: PROTOCOL_VERSION,
        };
        return reject(socket, reason).await.map(|()| None);
    }
    if !is_valid_nickname(&nickname) {
        return reject(socket, RejectReason::InvalidNickname)
            .await
            .map(|()| None);
    }

    let nonce = auth::new_nonce();
    send(socket, &ServerMessage::Challenge { nonce }).await?;
    let Some(ClientMessage::ChallengeResponse { signature }) = receive(socket).await? else {
        bail!("expected ChallengeResponse");
    };
    if !auth::verify_challenge(&public_key, &nonce, &signature) {
        return reject(socket, RejectReason::AuthenticationFailed)
            .await
            .map(|()| None);
    }

    Ok(Some(Player {
        id: PlayerId(public_key),
        nickname,
    }))
}

/// Sends the welcome, then lets the player browse lobbies and play in them until the connection
/// ends. Returns why it ended.
async fn connected(
    socket: &mut WebSocket,
    directory: &Directory,
    player: &Player,
    online: &OnlineGuard,
) -> String {
    let welcome = ServerMessage::Welcome {
        tracks: directory.tracks(),
    };
    if let Err(error) = send(socket, &welcome).await {
        return error.to_string();
    }

    let mut lobbies = directory.lobbies();
    let mut players = directory.online_players();
    lobbies.mark_changed();
    players.mark_changed();
    loop {
        let entered = tokio::select! {
            changed = lobbies.changed() => {
                if changed.is_err() {
                    return "the server is stopping".into();
                }
                let now = Instant::now();
                let list = lobbies
                    .borrow_and_update()
                    .iter()
                    .map(|info| info.summary(now))
                    .collect();
                if let Err(error) = send(socket, &ServerMessage::Lobbies(list)).await {
                    return error.to_string();
                }
                continue;
            }
            // Only browsing players are told who is connected: it is the lobby browser that
            // shows them.
            changed = players.changed() => {
                if changed.is_err() {
                    return "the server is stopping".into();
                }
                let list = players.borrow_and_update().as_ref().clone();
                if let Err(error) = send(socket, &ServerMessage::Players(list)).await {
                    return error.to_string();
                }
                continue;
            }
            message = receive(socket) => match message {
                Ok(Some(ClientMessage::CreateLobby(settings))) => {
                    directory.create(settings, player.clone()).await
                }
                Ok(Some(ClientMessage::JoinLobby(lobby))) => {
                    directory.join(lobby, player.clone()).await
                }
                // The round trip is the client's to time: answered before anything else.
                Ok(Some(ClientMessage::Ping(number))) => {
                    if let Err(error) = send(socket, &ServerMessage::Pong(number)).await {
                        return error.to_string();
                    }
                    continue;
                }
                // Sent before the client knew it was out of its lobby.
                Ok(Some(ClientMessage::Input { .. } | ClientMessage::LeaveLobby)) => continue,
                Ok(Some(_)) => return "unexpected message".into(),
                Ok(None) => return "connection closed".into(),
                Err(error) => return error.to_string(),
            },
        };

        match entered {
            Ok(membership) => {
                online.entered_lobby(membership.lobby.id());
                if let Some(reason) = play(socket, membership).await {
                    // The guard takes the player off the list of connected players.
                    return reason;
                }
                online.browsing();
                // Back to browsing: both lists may have changed meanwhile, send them again.
                lobbies.mark_changed();
                players.mark_changed();
            }
            Err(error) => {
                info!(player = %player.id, ?error, "entering a lobby failed");
                if let Err(error) = send(socket, &ServerMessage::JoinFailed(error)).await {
                    return error.to_string();
                }
            }
        }
    }
}

/// Relays between the player and their lobby. Returns `None` when the player leaves the lobby,
/// or why the connection ended.
async fn play(socket: &mut WebSocket, membership: Membership) -> Option<String> {
    let Membership {
        lobby,
        car,
        settings,
        mut snapshots,
        mut state,
    } = membership;
    let joined = ServerMessage::JoinedLobby {
        lobby: lobby.id(),
        car,
        settings,
    };
    if let Err(error) = send(socket, &joined).await {
        lobby.leave(car).await;
        return Some(error.to_string());
    }

    let ending = loop {
        tokio::select! {
            // The state first, so a player hears of a phase before the snapshots that follow it.
            biased;
            changed = state.changed() => {
                if changed.is_err() {
                    break Some("the lobby closed".into());
                }
                let bytes = state.borrow_and_update().clone();
                if let Err(error) = socket.send(Message::Binary(bytes)).await {
                    break Some(error.to_string());
                }
            }
            message = receive(socket) => match message {
                Ok(Some(ClientMessage::Input { input, tick, seq })) => {
                    lobby.input(car, input, tick, seq).await
                }
                Ok(Some(ClientMessage::Ping(number))) => {
                    if let Err(error) = send(socket, &ServerMessage::Pong(number)).await {
                        break Some(error.to_string());
                    }
                }
                Ok(Some(ClientMessage::LeaveLobby)) => break None,
                Ok(Some(_)) => break Some("unexpected message".into()),
                Ok(None) => break Some("connection closed".into()),
                Err(error) => break Some(error.to_string()),
            },
            snapshot = snapshots.recv() => {
                let Some(bytes) = snapshot else {
                    break Some("the lobby closed".into());
                };
                if let Err(error) = socket.send(Message::Binary(bytes)).await {
                    break Some(error.to_string());
                }
            }
        }
    };
    // Every way out of the loop leaves the lobby, including a lost connection.
    lobby.leave(car).await;
    ending
}

async fn reject(socket: &mut WebSocket, reason: RejectReason) -> Result<()> {
    info!(?reason, "player rejected");
    send(socket, &ServerMessage::Rejected(reason)).await?;
    socket.send(Message::Close(None)).await?;
    Ok(())
}

async fn send(socket: &mut WebSocket, message: &ServerMessage) -> Result<()> {
    socket
        .send(Message::Binary(protocol::encode(message).into()))
        .await?;
    Ok(())
}

/// Next message from the client, or `None` if it closed the connection.
async fn receive(socket: &mut WebSocket) -> Result<Option<ClientMessage>> {
    while let Some(message) = socket.recv().await {
        match message? {
            Message::Binary(bytes) => return Ok(Some(protocol::decode(&bytes)?)),
            Message::Close(_) => break,
            // Ping and pong are handled by axum; the protocol doesn't use text frames.
            _ => {}
        }
    }
    Ok(None)
}
