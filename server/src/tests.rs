//! Tests against a real server on an ephemeral port, on the shipped data, driven by a raw WebSocket
//! client. They cover what the game client cannot trigger on purpose (bad versions, bad signatures,
//! a duplicate identity, invalid lobby settings) and the flow of a player: browsing lobbies,
//! creating and joining them, driving, leaving.
//!
//! The server sends snapshots and lobby lists on its own schedule, and CI runners are slower than a
//! desktop: tests never count messages or assume timing, they wait for the message they expect,
//! with a timeout.

use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use space_race_protocol::auth::{self, SigningKey};
use space_race_protocol::{
    self as protocol, CarId, ClientMessage, JoinError, LobbyId, LobbyPhase, LobbySettings,
    LobbyState, LobbyStatus, LobbySummary, OnlinePlayer, PROTOCOL_VERSION, PlayerStatus, Presence,
    RejectReason, ServerMessage, Snapshot, TrackInfo, WS_PATH,
};
use space_race_sim::car::{Car, CarInput};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::content;
use crate::directory::Directory;
use crate::lobby::Timings;

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long a test waits for what it expects.
const PATIENCE: Duration = Duration::from_secs(5);

/// Starts a server on a free port, on the shipped data, and returns its URL.
async fn start_server() -> String {
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let tracks = content::load_tracks(&data_dir).unwrap();
    let tuning =
        content::load_car_tuning(&data_dir.join(content::CAR_TUNING_FILE), &tracks).unwrap();
    // Nothing will change the tuning: the sender can go.
    let (_, tuning) = watch::channel(tuning);
    let timings = Timings::default();
    let directory = Directory::spawn(tracks, tuning, timings);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, crate::router(directory))
            .await
            .unwrap()
    });
    format!("ws://{addr}{WS_PATH}")
}

async fn connect(url: &str) -> Client {
    let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    socket
}

async fn send(socket: &mut Client, message: &ClientMessage) {
    socket
        .send(Message::Binary(protocol::encode(message).into()))
        .await
        .unwrap();
}

async fn receive(socket: &mut Client) -> ServerMessage {
    let next = async {
        loop {
            let message = socket.next().await.expect("connection closed").unwrap();
            if let Message::Binary(bytes) = message {
                return protocol::decode(&bytes).unwrap();
            }
        }
    };
    timeout(PATIENCE, next)
        .await
        .expect("no message from the server")
}

/// Skips messages until `pick` returns something, and fails after [`PATIENCE`].
async fn wait_for<T>(
    socket: &mut Client,
    what: &str,
    mut pick: impl FnMut(ServerMessage) -> Option<T>,
) -> T {
    let wait = async {
        loop {
            if let Some(found) = pick(receive(socket).await) {
                return found;
            }
        }
    };
    timeout(PATIENCE, wait)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// After a rejection, the server must close the connection.
async fn assert_closed(socket: &mut Client) {
    match socket.next().await {
        None | Some(Ok(Message::Close(_))) => {}
        other => panic!("expected the connection to close, got {other:?}"),
    }
}

fn hello(protocol_version: u32, nickname: &str, key: &SigningKey) -> ClientMessage {
    ClientMessage::Hello {
        protocol_version,
        nickname: nickname.into(),
        public_key: key.verifying_key().to_bytes(),
    }
}

/// Sends `Hello` under `nickname`, then answers the challenge with a signature made by `signer`.
async fn handshake_as(
    socket: &mut Client,
    nickname: &str,
    key: &SigningKey,
    signer: &SigningKey,
) -> ServerMessage {
    send(socket, &hello(PROTOCOL_VERSION, nickname, key)).await;
    let ServerMessage::Challenge { nonce } = receive(socket).await else {
        panic!("expected Challenge");
    };
    let signature = auth::sign_challenge(signer, &nonce);
    send(socket, &ClientMessage::ChallengeResponse { signature }).await;
    receive(socket).await
}

/// The same, under the nickname every test uses when it does not care which.
async fn handshake(socket: &mut Client, key: &SigningKey, signer: &SigningKey) -> ServerMessage {
    handshake_as(socket, "Tester", key, signer).await
}

/// Connects with a new identity, and returns once the lobby list has arrived.
async fn browse(url: &str) -> (Client, Vec<TrackInfo>, Vec<LobbySummary>) {
    let key = auth::new_signing_key();
    let mut socket = connect(url).await;
    let ServerMessage::Welcome { tracks } = handshake(&mut socket, &key, &key).await else {
        panic!("expected Welcome");
    };
    let lobbies = next_lobbies(&mut socket).await;
    (socket, tracks, lobbies)
}

/// Connects with a new identity under `nickname`, and returns as soon as it is welcomed. The
/// lobby list and the player list follow in either order, so this waits for neither: waiting for
/// one would throw away the other whenever it came first.
async fn welcome_as(url: &str, nickname: &str) -> Client {
    let key = auth::new_signing_key();
    let mut socket = connect(url).await;
    let ServerMessage::Welcome { .. } = handshake_as(&mut socket, nickname, &key, &key).await
    else {
        panic!("expected Welcome");
    };
    socket
}

async fn next_lobbies(socket: &mut Client) -> Vec<LobbySummary> {
    wait_for(socket, "the lobby list", |message| match message {
        ServerMessage::Lobbies(lobbies) => Some(lobbies),
        _ => None,
    })
    .await
}

/// The next list of connected players in which `enough` holds.
async fn next_players(
    socket: &mut Client,
    what: &str,
    mut enough: impl FnMut(&[OnlinePlayer]) -> bool,
) -> Vec<OnlinePlayer> {
    wait_for(socket, what, |message| match message {
        ServerMessage::Players(players) if enough(&players) => Some(players),
        _ => None,
    })
    .await
}

fn settings(name: &str, min_players: u8, max_players: u8) -> LobbySettings {
    LobbySettings {
        name: name.into(),
        track: "esplanade".into(),
        laps: 3,
        min_players,
        max_players,
    }
}

/// Sends `message`, and returns where the player entered, or why they could not.
async fn enter(socket: &mut Client, message: ClientMessage) -> Result<(LobbyId, CarId), JoinError> {
    send(socket, &message).await;
    wait_for(socket, "entering a lobby", |message| match message {
        ServerMessage::JoinedLobby { lobby, car, .. } => Some(Ok((lobby, car))),
        ServerMessage::JoinFailed(error) => Some(Err(error)),
        _ => None,
    })
    .await
}

async fn next_state(socket: &mut Client) -> LobbyState {
    wait_for(socket, "the lobby state", |message| match message {
        ServerMessage::Lobby(state) => Some(state),
        _ => None,
    })
    .await
}

async fn next_snapshot(socket: &mut Client) -> Snapshot {
    wait_for(socket, "a snapshot", |message| match message {
        ServerMessage::Snapshot(snapshot) => Some(snapshot),
        _ => None,
    })
    .await
}

fn find(snapshot: &Snapshot, id: CarId) -> Option<Car> {
    snapshot
        .cars
        .iter()
        .find(|car| car.id == id)
        .map(|car| car.car)
}

#[tokio::test]
async fn welcome_lists_the_tracks_then_the_lobbies() {
    let url = start_server().await;
    let (_socket, tracks, lobbies) = browse(&url).await;

    let keys: Vec<_> = tracks.iter().map(|track| track.key.as_str()).collect();
    assert!(keys.contains(&"esplanade"), "{keys:?}");
    assert!(lobbies.is_empty(), "{lobbies:?}");
}

#[tokio::test]
async fn a_created_lobby_is_listed_until_its_last_player_leaves() {
    let url = start_server().await;
    let (mut creator, _, _) = browse(&url).await;
    let (mut browser, _, _) = browse(&url).await;

    let (lobby, car) = enter(
        &mut creator,
        ClientMessage::CreateLobby(settings("Friday night", 2, 8)),
    )
    .await
    .unwrap();
    let state = next_state(&mut creator).await;
    assert_eq!(state.phase, LobbyPhase::Waiting);
    assert_eq!(state.players.len(), 1);
    assert_eq!(state.players[0].car, car);
    assert_eq!(state.players[0].status, PlayerStatus::Driving);

    let listed = wait_for(&mut browser, "the new lobby", |message| match message {
        ServerMessage::Lobbies(lobbies) => lobbies.into_iter().find(|summary| summary.id == lobby),
        _ => None,
    })
    .await;
    assert_eq!(listed.settings.name, "Friday night");
    assert_eq!(listed.players, ["Tester"]);
    assert_eq!(listed.status, LobbyStatus::Waiting);

    // Leaving takes the creator back to the list, where the empty lobby is gone.
    send(&mut creator, &ClientMessage::LeaveLobby).await;
    wait_for(
        &mut creator,
        "an empty lobby list",
        |message| match message {
            ServerMessage::Lobbies(lobbies) if lobbies.is_empty() => Some(()),
            _ => None,
        },
    )
    .await;
    wait_for(
        &mut browser,
        "an empty lobby list",
        |message| match message {
            ServerMessage::Lobbies(lobbies) if lobbies.is_empty() => Some(()),
            _ => None,
        },
    )
    .await;
}

/// Whoever browses lobbies is told who else is connected, and whether they are on the list too
/// or inside a lobby. The list follows them in and out, and forgets them when they disconnect.
#[tokio::test]
async fn browsing_players_are_told_who_is_connected_and_where() {
    let url = start_server().await;
    let mut ayrton = welcome_as(&url, "Ayrton").await;

    let alone = next_players(&mut ayrton, "being listed alone", |players| {
        players.len() == 1
    })
    .await;
    assert_eq!(alone[0].nickname, "Ayrton");
    assert_eq!(alone[0].presence, Presence::Browsing);

    // A second player connects, and is listed after the first: the order they arrived in.
    let mut alain = welcome_as(&url, "Alain").await;
    let both = next_players(&mut ayrton, "the second player", |players| {
        players.len() == 2
    })
    .await;
    let nicknames: Vec<&str> = both.iter().map(|player| player.nickname.as_str()).collect();
    assert_eq!(nicknames, ["Ayrton", "Alain"]);

    // Creating a lobby is entering it, and everyone browsing sees where they went.
    let (lobby, _) = enter(
        &mut alain,
        ClientMessage::CreateLobby(settings("Suzuka", 2, 8)),
    )
    .await
    .unwrap();
    let inside = next_players(&mut ayrton, "the player in a lobby", |players| {
        players
            .iter()
            .any(|player| player.presence == Presence::InLobby(lobby))
    })
    .await;
    assert_eq!(inside.len(), 2);
    assert_eq!(inside[0].presence, Presence::Browsing);

    // Leaving puts them back on the list, and disconnecting takes them off it.
    send(&mut alain, &ClientMessage::LeaveLobby).await;
    next_players(&mut ayrton, "the player back on the list", |players| {
        players.len() == 2
            && players
                .iter()
                .all(|player| player.presence == Presence::Browsing)
    })
    .await;
    drop(alain);
    next_players(&mut ayrton, "the player gone", |players| players.len() == 1).await;
}

/// The two things the client times its latency with: snapshots say which of the player's inputs
/// they already include, and a ping comes straight back wherever the player is.
#[tokio::test]
async fn snapshots_say_which_input_they_include_and_pings_come_back() {
    let url = start_server().await;
    let (mut socket, _, _) = browse(&url).await;
    let pong = |number: u32| {
        move |message: ServerMessage| (message == ServerMessage::Pong(number)).then_some(())
    };

    send(&mut socket, &ClientMessage::Ping(7)).await;
    wait_for(&mut socket, "a pong while browsing", pong(7)).await;

    let (_, car) = enter(
        &mut socket,
        ClientMessage::CreateLobby(settings("Echo", 2, 8)),
    )
    .await
    .unwrap();
    let own_seq = |snapshot: &Snapshot| {
        snapshot
            .cars
            .iter()
            .find(|snapshot| snapshot.id == car)
            .map(|snapshot| snapshot.input_seq)
    };
    assert_eq!(own_seq(&next_snapshot(&mut socket).await), Some(0));

    let input = ClientMessage::Input {
        input: CarInput::new(true, 0.0),
        tick: 0,
        seq: 1,
    };
    send(&mut socket, &input).await;
    wait_for(
        &mut socket,
        "the input to be acted on",
        |message| match message {
            ServerMessage::Snapshot(snapshot) => (own_seq(&snapshot) == Some(1)).then_some(()),
            _ => None,
        },
    )
    .await;

    send(&mut socket, &ClientMessage::Ping(8)).await;
    wait_for(&mut socket, "a pong in a lobby", pong(8)).await;
}

#[tokio::test]
async fn enough_players_line_up_for_a_race() {
    let url = start_server().await;
    let (mut first, _, _) = browse(&url).await;
    let (mut second, _, _) = browse(&url).await;

    let (lobby, _) = enter(
        &mut first,
        ClientMessage::CreateLobby(settings("Duel", 2, 2)),
    )
    .await
    .unwrap();
    enter(&mut second, ClientMessage::JoinLobby(lobby))
        .await
        .unwrap();

    let players = wait_for(&mut second, "the race", |message| match message {
        ServerMessage::Lobby(LobbyState {
            phase: LobbyPhase::Racing { .. },
            players,
            ..
        }) => Some(players),
        _ => None,
    })
    .await;
    assert!(
        players
            .iter()
            .all(|player| player.status == PlayerStatus::Racing)
    );

    // Full: a third player cannot join.
    let (mut third, _, _) = browse(&url).await;
    assert_eq!(
        enter(&mut third, ClientMessage::JoinLobby(lobby)).await,
        Err(JoinError::LobbyFull)
    );
}

#[tokio::test]
async fn accelerating_moves_the_car_forward() {
    let url = start_server().await;
    let (mut socket, _, _) = browse(&url).await;
    let (_, car) = enter(
        &mut socket,
        ClientMessage::CreateLobby(settings("Solo", 2, 8)),
    )
    .await
    .unwrap();
    let start = find(&next_snapshot(&mut socket).await, car).unwrap();

    let accelerate = CarInput::new(true, 0.0);
    let input = ClientMessage::Input {
        input: accelerate,
        tick: 0,
        seq: 1,
    };
    send(&mut socket, &input).await;
    // Snapshots queued before the input still show the car standing, and how many there are
    // depends on the machine: wait for the movement instead of counting snapshots.
    let moved = wait_for(&mut socket, "the car to move", |message| match message {
        ServerMessage::Snapshot(snapshot) => {
            let moved = find(&snapshot, car).unwrap().position - start.position;
            (moved.length() > 1.0).then_some(moved)
        }
        _ => None,
    })
    .await;
    assert!(moved.dot(start.forward()) > 0.0, "{moved:?}");
}

#[tokio::test]
async fn players_see_each_other_and_leaving_removes_the_car() {
    let url = start_server().await;
    let (mut first, _, _) = browse(&url).await;
    let (mut second, _, _) = browse(&url).await;
    let (lobby, first_car) = enter(
        &mut first,
        ClientMessage::CreateLobby(settings("Pair", 3, 8)),
    )
    .await
    .unwrap();
    let (_, second_car) = enter(&mut second, ClientMessage::JoinLobby(lobby))
        .await
        .unwrap();
    assert_ne!(first_car, second_car);

    let (first_position, second_position) =
        wait_for(&mut first, "both cars", |message| match message {
            ServerMessage::Snapshot(snapshot) => {
                match (find(&snapshot, first_car), find(&snapshot, second_car)) {
                    (Some(first), Some(second)) => Some((first.position, second.position)),
                    _ => None,
                }
            }
            _ => None,
        })
        .await;
    assert!(first_position.distance(second_position) > 1.0);

    drop(second);
    wait_for(&mut first, "the car to leave", |message| match message {
        ServerMessage::Snapshot(snapshot) => find(&snapshot, second_car).is_none().then_some(()),
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn invalid_lobbies_are_refused() {
    let url = start_server().await;
    let (mut socket, _, _) = browse(&url).await;

    let no_players = settings("Nobody", 0, 8);
    assert_eq!(
        enter(&mut socket, ClientMessage::CreateLobby(no_players)).await,
        Err(JoinError::InvalidSettings)
    );
    let unknown_track = LobbySettings {
        track: "moon".into(),
        ..settings("Moon", 2, 8)
    };
    assert_eq!(
        enter(&mut socket, ClientMessage::CreateLobby(unknown_track)).await,
        Err(JoinError::UnknownTrack)
    );
    assert_eq!(
        enter(&mut socket, ClientMessage::JoinLobby(LobbyId(999))).await,
        Err(JoinError::NoSuchLobby)
    );
}

#[tokio::test]
async fn same_identity_twice_is_rejected() {
    let url = start_server().await;
    let key = auth::new_signing_key();
    let mut first = connect(&url).await;
    let ServerMessage::Welcome { .. } = handshake(&mut first, &key, &key).await else {
        panic!("expected Welcome");
    };

    let mut second = connect(&url).await;
    assert_eq!(
        handshake(&mut second, &key, &key).await,
        ServerMessage::Rejected(RejectReason::AlreadyConnected)
    );
    assert_closed(&mut second).await;

    // Once the first connection is gone, the identity can connect again.
    drop(first);
    let reconnected = async {
        loop {
            let mut third = connect(&url).await;
            if let ServerMessage::Welcome { .. } = handshake(&mut third, &key, &key).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    timeout(PATIENCE, reconnected)
        .await
        .expect("the identity could not connect again");
}

#[tokio::test]
async fn signature_from_another_key_is_rejected() {
    let url = start_server().await;
    let mut socket = connect(&url).await;
    let reply = handshake(
        &mut socket,
        &auth::new_signing_key(),
        &auth::new_signing_key(),
    )
    .await;

    assert_eq!(
        reply,
        ServerMessage::Rejected(RejectReason::AuthenticationFailed)
    );
    assert_closed(&mut socket).await;
}

#[tokio::test]
async fn protocol_mismatch_is_rejected() {
    let url = start_server().await;
    let mut socket = connect(&url).await;
    let key = auth::new_signing_key();
    send(&mut socket, &hello(PROTOCOL_VERSION + 1, "Tester", &key)).await;

    assert_eq!(
        receive(&mut socket).await,
        ServerMessage::Rejected(RejectReason::ProtocolMismatch {
            server_version: PROTOCOL_VERSION
        })
    );
    assert_closed(&mut socket).await;
}

#[tokio::test]
async fn invalid_nickname_is_rejected() {
    let url = start_server().await;
    let mut socket = connect(&url).await;
    let key = auth::new_signing_key();
    send(&mut socket, &hello(PROTOCOL_VERSION, " Tester", &key)).await;

    assert_eq!(
        receive(&mut socket).await,
        ServerMessage::Rejected(RejectReason::InvalidNickname)
    );
    assert_closed(&mut socket).await;
}
