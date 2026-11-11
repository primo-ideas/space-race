//! Lifecycle of a player connection: handshake (version, nickname, authentication), then browsing
//! lobbies and playing in them until the player leaves.

use std::fmt;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use star_racer_protocol::auth::{self, PublicKey};
use star_racer_protocol::ranked::QueueStatus;
use star_racer_protocol::{
    self as protocol, ClientMessage, PROTOCOL_VERSION, RejectReason, ServerMessage,
    is_valid_nickname,
};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::info;

use crate::directory::{Directory, OnlineGuard, Replaced};
use crate::lobby::Membership;

/// Time given to the client to complete the handshake. Generous, because a web client answers
/// from the page's main thread, which its first frames can hold for seconds at a time while the
/// browser compiles shaders: 5 seconds was not enough for a page opened with `?nickname=` to
/// connect on its first load, even on a desktop GPU, and neither were 20 against the deployed
/// site, where a first visit held the thread for 29 seconds. A connection that never answers only
/// costs a task until then.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(120);

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
    // the list of connected players. A connection of the same player still open is replaced, and
    // this one will be in turn if the player connects again (see `Directory::connect`).
    let (online, mut replaced) = directory.connect(&player);

    info!(player = %player.id, nickname = %player.nickname, "player connected");
    // From here on, a dropped connection (crash, lost network) is an ordinary way to leave, not a
    // server failure: it is logged as the reason.
    let reason = connected(&mut socket, &directory, &player, &online, &mut replaced).await;
    info!(player = %player.id, nickname = %player.nickname, %reason, "player disconnected");
    Ok(())
}

/// Why a connection replaced by a newer one of the same player ends.
const REPLACED: &str = "replaced by a new connection";

/// Tells a replaced connection's client why it is being closed, if it is still there to hear it:
/// the same words a second window used to get, now said to the first.
async fn close_replaced(socket: &mut WebSocket) -> String {
    let _ = reject(socket, RejectReason::AlreadyConnected).await;
    REPLACED.into()
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
    replaced: &mut Replaced,
) -> String {
    let welcome = ServerMessage::Welcome {
        tracks: directory.tracks(),
    };
    if let Err(error) = send(socket, &welcome).await {
        return error.to_string();
    }
    if let Err(error) = send_profile(socket, directory, player).await {
        return error.to_string();
    }

    let mut lobbies = directory.lobbies();
    let mut players = directory.online_players();
    let mut leaderboard = directory.leaderboard();
    let mut queue_size = directory.queue_size();
    lobbies.mark_changed();
    players.mark_changed();
    leaderboard.mark_changed();
    // Holds the player's place in the ranked queue: dropping it gives the place up.
    let mut queued: Option<oneshot::Receiver<Membership>> = None;
    loop {
        let entered = tokio::select! {
            // The sender lives in the list of connected players, so it only goes, unsent, with
            // this connection: either way there is nothing left to do.
            _ = &mut *replaced => return close_replaced(socket).await,
            matched = async { queued.as_mut().expect("guarded by is_some").await },
                if queued.is_some() =>
            {
                queued = None;
                if let Err(error) = send(socket, &ServerMessage::Queue(None)).await {
                    return error.to_string();
                }
                match matched {
                    Ok(membership) => Ok(membership),
                    // The directory dropped the place: the server is stopping.
                    Err(_) => continue,
                }
            }
            changed = queue_size.changed(), if queued.is_some() => {
                if changed.is_err() {
                    return "the server is stopping".into();
                }
                let players = *queue_size.borrow_and_update();
                let status = ServerMessage::Queue(Some(QueueStatus { players }));
                if let Err(error) = send(socket, &status).await {
                    return error.to_string();
                }
                continue;
            }
            changed = leaderboard.changed() => {
                if changed.is_err() {
                    return "the server is stopping".into();
                }
                let board = leaderboard.borrow_and_update().as_ref().clone();
                if let Err(error) = send(socket, &ServerMessage::Leaderboard(board)).await {
                    return error.to_string();
                }
                // Theirs may be what moved it: a player who walked out of a race is back here
                // before the race has been settled.
                if let Err(error) = send_profile(socket, directory, player).await {
                    return error.to_string();
                }
                continue;
            }
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
                Ok(Some(ClientMessage::QuickPlay)) => {
                    directory.quick_play(player.clone()).await
                }
                Ok(Some(ClientMessage::QueueRanked)) => {
                    queued = Some(directory.enqueue(player.clone()).await);
                    // Says so at once, and the size as soon as the directory has counted them in.
                    queue_size.mark_changed();
                    continue;
                }
                Ok(Some(ClientMessage::LeaveQueue)) => {
                    queued = None;
                    directory.dequeue(player.id).await;
                    if let Err(error) = send(socket, &ServerMessage::Queue(None)).await {
                        return error.to_string();
                    }
                    continue;
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
                // Entering a lobby is leaving the queue.
                if queued.take().is_some() {
                    directory.dequeue(player.id).await;
                    if let Err(error) = send(socket, &ServerMessage::Queue(None)).await {
                        return error.to_string();
                    }
                }
                online.entered_lobby(membership.lobby.id());
                match play(socket, membership, replaced).await {
                    // The guard takes the player off the list of connected players.
                    Ending::Lost(reason) => return reason,
                    Ending::Left => {}
                    Ending::LobbyClosed => {
                        if let Err(error) = send(socket, &ServerMessage::LobbyEnded).await {
                            return error.to_string();
                        }
                    }
                }
                online.browsing();
                // A match they just finished may have moved their rating.
                if let Err(error) = send_profile(socket, directory, player).await {
                    return error.to_string();
                }
                // Back to browsing: both lists may have changed meanwhile, send them again.
                lobbies.mark_changed();
                players.mark_changed();
                leaderboard.mark_changed();
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

/// Sends the player where they stand on the ladder, unless the server is stopping.
async fn send_profile(
    socket: &mut WebSocket,
    directory: &Directory,
    player: &Player,
) -> Result<()> {
    match directory.profile(player.id).await {
        Some(profile) => send(socket, &ServerMessage::Profile(profile)).await,
        None => Ok(()),
    }
}

/// How a player's time in a lobby ended.
enum Ending {
    /// They left, and browse lobbies again.
    Left,
    /// The lobby closed around them, as a ranked match does: they browse lobbies again.
    LobbyClosed,
    /// The connection ended, and why.
    Lost(String),
}

/// Relays between the player and their lobby, until they leave it or the connection ends.
async fn play(socket: &mut WebSocket, membership: Membership, replaced: &mut Replaced) -> Ending {
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
        return Ending::Lost(error.to_string());
    }

    let ending = loop {
        tokio::select! {
            // The state first, so a player hears of a phase before the snapshots that follow it.
            biased;
            _ = &mut *replaced => {
                // Out of the lobby first, so the car does not linger while the client is told.
                lobby.leave(car).await;
                return Ending::Lost(close_replaced(socket).await);
            }
            changed = state.changed() => {
                if changed.is_err() {
                    break Ending::LobbyClosed;
                }
                let bytes = state.borrow_and_update().clone();
                if let Err(error) = socket.send(Message::Binary(bytes)).await {
                    break Ending::Lost(error.to_string());
                }
            }
            message = receive(socket) => match message {
                Ok(Some(ClientMessage::Input { input, tick, seq })) => {
                    lobby.input(car, input, tick, seq).await
                }
                Ok(Some(ClientMessage::Ping(number))) => {
                    if let Err(error) = send(socket, &ServerMessage::Pong(number)).await {
                        break Ending::Lost(error.to_string());
                    }
                }
                Ok(Some(ClientMessage::LeaveLobby)) => break Ending::Left,
                // Sent before the client knew it was in a match: it was in the queue a moment ago.
                Ok(Some(ClientMessage::LeaveQueue | ClientMessage::QueueRanked)) => {}
                Ok(Some(_)) => break Ending::Lost("unexpected message".into()),
                Ok(None) => break Ending::Lost("connection closed".into()),
                Err(error) => break Ending::Lost(error.to_string()),
            },
            snapshot = snapshots.recv() => {
                let Some(bytes) = snapshot else {
                    break Ending::LobbyClosed;
                };
                if let Err(error) = socket.send(Message::Binary(bytes)).await {
                    break Ending::Lost(error.to_string());
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
