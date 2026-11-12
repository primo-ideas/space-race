//! Server connection. It hands what the server says to Bevy as [`NetEvent`] messages, and sends
//! what Bevy asks through [`Network::send`].
//!
//! The player connects from the first screen, as a guest or to their account, and can disconnect
//! to come in another way: the [`Network`] resource starts and ends sessions on demand. A session runs outside Bevy's
//! systems: on a thread of its own with a tokio runtime on native, on the page's event loop on the
//! web, where there are no threads.

pub mod identity;

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::Result;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use futures_util::{SinkExt, StreamExt};
use star_racer_protocol::auth;
use star_racer_protocol::ranked::{LeaderboardEntry, QueueStatus, RankedProfile};
use star_racer_protocol::{
    self as protocol, CarId, ClientMessage, Identity, JoinError, LobbyId, LobbySettings,
    LobbyState, LobbySummary, Login, OnlinePlayer, PROTOCOL_VERSION, RejectReason, ServerMessage,
    Snapshot, TrackInfo,
};
use tokio::sync::{mpsc as async_mpsc, oneshot};
use tokio_tungstenite_wasm::{Message, WebSocketStream};

use self::identity::IdentityStore;
use crate::preferences::StoredPreferences;
use crate::settings::server_to_use;

pub struct NetPlugin {
    /// The server asked for on the command line or in the page address, as a WebSocket URL
    /// (`ws://` locally, `wss://` in production). Without one, the host the player set in the
    /// settings, or the default (see [`crate::settings::server_to_use`]).
    pub server: Option<String>,
    pub identity: IdentityStore,
}

/// What the server said, in the order it said it.
#[derive(Message, Debug)]
pub enum NetEvent {
    Welcome {
        tracks: Vec<TrackInfo>,
        /// Who the server took the player for.
        identity: Identity,
    },
    Lobbies(Vec<LobbySummary>),
    /// Everyone connected to the server, sent while browsing lobbies.
    Players(Vec<OnlinePlayer>),
    JoinedLobby {
        lobby: LobbyId,
        car: CarId,
        settings: LobbySettings,
    },
    JoinFailed(JoinError),
    Lobby(LobbyState),
    /// The lobby closed around the player, as a ranked match does after its results.
    LobbyEnded,
    /// Where the player stands on the ladder.
    Profile(RankedProfile),
    /// The best players.
    Leaderboard(Vec<LeaderboardEntry>),
    /// Where the ranked queue stands while the player is in it, `None` once they are out.
    Queue(Option<QueueStatus>),
    Snapshot {
        snapshot: Snapshot,
        /// When the session received it, so frame timing does not add jitter.
        received: Instant,
    },
    /// The answer to a ping, and when it came in, for the same reason.
    Pong {
        number: u32,
        received: Instant,
    },
    Rejected(RejectReason),
    /// Always the last event of a session.
    Disconnected(String),
}

/// Where the player stands with the server.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub enum Connection {
    /// Not connected, and nothing went wrong.
    #[default]
    Offline,
    /// Under this name: a guest's, or the account's being signed in to.
    Connecting {
        nickname: String,
    },
    /// Authenticated, under this name, as a guest or not.
    Online {
        nickname: String,
        guest: bool,
    },
    Rejected(RejectReason),
    Disconnected {
        reason: String,
        /// Whether the server had welcomed the player, or could not be reached at all.
        was_online: bool,
    },
}

/// The connection to the server, if any.
#[derive(Resource)]
pub struct Network {
    server: String,
    identity: IdentityStore,
    session: Option<Session>,
    /// How to come back the way the server last welcomed the player: the same guest, or the
    /// account this device is now signed in to, without its password.
    resume: Option<(String, Login)>,
}

/// Bevy's ends of the channels to a running session.
struct Session {
    outbox: async_mpsc::UnboundedSender<ClientMessage>,
    events: Mutex<Receiver<NetEvent>>,
    /// Tells the session to close the connection. Dropping it does too.
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl Network {
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Plays on another server from now on. A player connected to the one before is not any
    /// more: `true` if the server changed.
    pub fn set_server(&mut self, server: String, connection: &mut Connection) -> bool {
        if self.server == server {
            return false;
        }
        self.disconnect(connection);
        self.server = server;
        true
    }

    /// Connects the way the server last welcomed the player, if it has. `false` if it never has.
    pub fn resume(&mut self, connection: &mut Connection) -> bool {
        let Some((nickname, login)) = self.resume.clone() else {
            return false;
        };
        self.connect(nickname, login, connection);
        true
    }

    /// Signs this device out of the account it is connected to, and ends the session: the server
    /// forgets the device. Nothing comes back the way it came in.
    pub fn sign_out(&mut self, connection: &mut Connection) {
        self.send(ClientMessage::SignOut);
        self.disconnect(connection);
        self.forget();
    }

    /// Forgets how the player last came in.
    pub fn forget(&mut self) {
        self.resume = None;
    }

    /// Makes this device a new one: a new key, which no account knows and no guest has played
    /// under. What signing out does when the server cannot be told.
    pub fn new_device(&mut self) {
        if let Err(error) = self.identity.replace() {
            warn!(%error, "the device key could not be replaced");
        }
    }

    /// Connects with `login`, showing `nickname` meanwhile, ending any previous session.
    pub fn connect(&mut self, nickname: String, login: Login, connection: &mut Connection) {
        self.disconnect(connection);

        let (sender, receiver) = mpsc::channel();
        let (outbox, outgoing) = async_mpsc::unbounded_channel();
        let (shutdown_sender, shutdown) = oneshot::channel();
        let server = self.server.clone();
        let identity = self.identity.clone();
        *connection = Connection::Connecting {
            nickname: nickname.clone(),
        };

        spawn_session(move || async move {
            let channels = Channels {
                events: &sender,
                outgoing,
                shutdown,
            };
            let reason = run_session(&server, login, &identity, channels)
                .await
                .unwrap_or_else(|error| format!("{error:#}"));
            let _ = sender.send(NetEvent::Disconnected(reason));
        });

        self.session = Some(Session {
            outbox,
            events: Mutex::new(receiver),
            shutdown: Mutex::new(Some(shutdown_sender)),
        });
    }

    /// Closes the connection cleanly, if there is one. What the session still says is dropped.
    pub fn disconnect(&mut self, connection: &mut Connection) {
        if let Some(session) = self.session.take() {
            session.request_shutdown();
        }
        *connection = Connection::Offline;
    }

    pub fn send(&self, message: ClientMessage) {
        // Without a session, there is nobody to tell.
        if let Some(session) = &self.session {
            // Fails only once the connection is over.
            let _ = session.outbox.send(message);
        }
    }
}

impl Session {
    /// `true` if the session was still running to hear it.
    fn request_shutdown(&self) -> bool {
        let sender = self
            .shutdown
            .lock()
            .expect("shutdown sender poisoned")
            .take();
        sender.is_some_and(|sender| sender.send(()).is_ok())
    }
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        // The preferences are loaded by the time this is built: their plugin is added first.
        let chosen = app
            .world()
            .get_resource::<StoredPreferences>()
            .and_then(|preferences| preferences.get().host.clone());
        let server = server_to_use(self.server.clone(), chosen.as_deref());

        // rustls is told who does its arithmetic once, before the first `wss://` connection. A
        // browser does its own TLS.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = rustls::crypto::ring::default_provider().install_default();

        app.add_message::<NetEvent>()
            .init_resource::<Connection>()
            .insert_resource(Network {
                server,
                identity: self.identity.clone(),
                session: None,
                resume: None,
            })
            .add_systems(First, forward_net_events);

        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Last, close_on_exit.after(bevy::window::ExitSystems));
    }
}

/// Native: a thread of its own, running a single-threaded tokio runtime.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_session<F: Future<Output = ()>>(session: impl FnOnce() -> F + Send + 'static) {
    std::thread::Builder::new()
        .name("net".into())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime")
                .block_on(session());
        })
        .expect("failed to spawn network thread");
}

/// Web: no threads, so the session runs on the page's event loop, between frames.
#[cfg(target_arch = "wasm32")]
fn spawn_session<F: Future<Output = ()> + 'static>(session: impl FnOnce() -> F) {
    wasm_bindgen_futures::spawn_local(session());
}

/// The session's ends of the channels to Bevy.
struct Channels<'a> {
    events: &'a Sender<NetEvent>,
    outgoing: async_mpsc::UnboundedReceiver<ClientMessage>,
    shutdown: oneshot::Receiver<()>,
}

/// Connects, authenticates, then relays until the server leaves or `shutdown` fires. Returns the
/// disconnection reason.
async fn run_session(
    server: &str,
    login: Login,
    identity: &IdentityStore,
    channels: Channels<'_>,
) -> Result<String> {
    let Channels {
        events,
        mut outgoing,
        mut shutdown,
    } = channels;
    let key = identity.load_or_create()?;
    let mut socket = tokio::select! {
        socket = tokio_tungstenite_wasm::connect(server) => socket?,
        _ = &mut shutdown => return Ok("client quit".into()),
    };

    let hello = ClientMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        public_key: key.verifying_key().to_bytes(),
        login,
    };
    send(&mut socket, &hello).await?;

    loop {
        tokio::select! {
            message = receive(&mut socket) => {
                let Some(message) = message? else {
                    return Ok("connection closed by server".into());
                };
                let event = match message {
                    ServerMessage::Challenge { nonce } => {
                        let signature = auth::sign_challenge(&key, &nonce);
                        send(&mut socket, &ClientMessage::ChallengeResponse { signature }).await?;
                        continue;
                    }
                    ServerMessage::Welcome { tracks, identity } => {
                        NetEvent::Welcome { tracks, identity }
                    }
                    ServerMessage::Lobbies(lobbies) => NetEvent::Lobbies(lobbies),
                    ServerMessage::Players(players) => NetEvent::Players(players),
                    ServerMessage::JoinedLobby { lobby, car, settings } => {
                        NetEvent::JoinedLobby { lobby, car, settings }
                    }
                    ServerMessage::JoinFailed(error) => NetEvent::JoinFailed(error),
                    ServerMessage::Lobby(state) => NetEvent::Lobby(state),
                    ServerMessage::LobbyEnded => NetEvent::LobbyEnded,
                    ServerMessage::Profile(profile) => NetEvent::Profile(profile),
                    ServerMessage::Leaderboard(board) => NetEvent::Leaderboard(board),
                    ServerMessage::Queue(status) => NetEvent::Queue(status),
                    ServerMessage::Snapshot(snapshot) => NetEvent::Snapshot {
                        snapshot,
                        received: Instant::now(),
                    },
                    ServerMessage::Pong(number) => NetEvent::Pong {
                        number,
                        received: Instant::now(),
                    },
                    ServerMessage::Rejected(reason) => {
                        let _ = events.send(NetEvent::Rejected(reason));
                        return Ok("rejected by server".into());
                    }
                };
                let _ = events.send(event);
            }
            Some(message) = outgoing.recv() => send(&mut socket, &message).await?,
            // Also fires if the sender is dropped, which is just as much a reason to stop.
            _ = &mut shutdown => {
                // What was asked for before is said first: signing out, for one.
                while let Ok(message) = outgoing.try_recv() {
                    send(&mut socket, &message).await?;
                }
                socket.close().await?;
                return Ok("client quit".into());
            }
        }
    }
}

async fn send(socket: &mut WebSocketStream, message: &ClientMessage) -> Result<()> {
    socket
        .send(Message::Binary(protocol::encode(message).into()))
        .await?;
    Ok(())
}

/// Next message from the server, or `None` if it closed the connection.
async fn receive(socket: &mut WebSocketStream) -> Result<Option<ServerMessage>> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Binary(bytes) => return Ok(Some(protocol::decode(&bytes)?)),
            Message::Close(_) => break,
            // The protocol doesn't use text frames.
            Message::Text(_) => {}
        }
    }
    Ok(None)
}

/// Hands the session's events to Bevy, and keeps [`Connection`] up to date.
fn forward_net_events(
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
    mut messages: MessageWriter<NetEvent>,
) {
    let Some(session) = &network.session else {
        return;
    };
    let events: Vec<NetEvent> = session
        .events
        .lock()
        .expect("network event channel poisoned")
        .try_iter()
        .collect();

    for event in events {
        log_event(&event);
        match &event {
            NetEvent::Welcome { identity, .. } => {
                if matches!(*connection, Connection::Connecting { .. }) {
                    *connection = Connection::Online {
                        nickname: identity.name.clone(),
                        guest: identity.guest,
                    };
                }
                let login = if identity.guest {
                    Login::Guest {
                        name: identity.name.clone(),
                    }
                } else {
                    Login::Device
                };
                network.resume = Some((identity.name.clone(), login));
            }
            NetEvent::Rejected(reason) => *connection = Connection::Rejected(reason.clone()),
            NetEvent::Disconnected(reason) => {
                if !matches!(*connection, Connection::Rejected(_)) {
                    *connection = Connection::Disconnected {
                        reason: reason.clone(),
                        was_online: matches!(*connection, Connection::Online { .. }),
                    };
                }
                network.session = None;
            }
            _ => {}
        }
        messages.write(event);
    }
}

/// Closes the connection before the app exits, so the server sees the player leave instead of a
/// dropped connection. Blocks the last frame for at most one second.
///
/// Native only: on the web, the session runs on the page's event loop, which cannot make progress
/// while a frame blocks, and closing the page closes the connection anyway.
#[cfg(not(target_arch = "wasm32"))]
fn close_on_exit(mut exits: MessageReader<AppExit>, network: Res<Network>) {
    const CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

    if exits.read().next().is_none() {
        return;
    }
    let Some(session) = &network.session else {
        return;
    };
    if !session.request_shutdown() {
        return;
    }

    let receiver = session
        .events
        .lock()
        .expect("network event channel poisoned");
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = receiver.recv_timeout(remaining) else {
            break;
        };
        log_event(&event);
        if matches!(event, NetEvent::Disconnected(_)) {
            break;
        }
    }
}

fn log_event(event: &NetEvent) {
    match event {
        NetEvent::Welcome { tracks, identity } => info!(
            tracks = tracks.len(),
            name = %identity.name,
            guest = identity.guest,
            "connected"
        ),
        NetEvent::JoinedLobby {
            lobby,
            car,
            settings,
        } => info!(%lobby, %car, name = %settings.name, "entered a lobby"),
        NetEvent::JoinFailed(error) => warn!(?error, "could not enter the lobby"),
        NetEvent::Rejected(reason) => warn!(?reason, "rejected by server"),
        NetEvent::Disconnected(reason) => warn!(%reason, "disconnected from server"),
        NetEvent::Lobbies(_)
        | NetEvent::Players(_)
        | NetEvent::Lobby(_)
        | NetEvent::LobbyEnded
        | NetEvent::Profile(_)
        | NetEvent::Leaderboard(_)
        | NetEvent::Queue(_)
        | NetEvent::Snapshot { .. }
        | NetEvent::Pong { .. } => {}
    }
}
