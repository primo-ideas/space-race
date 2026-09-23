//! What the client knows of lobbies: the tracks they race on, the lobby list, and the lobby the
//! player is in. Every screen reads these; only this module writes them from the server's messages.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use space_race_protocol::{
    CarId, ClientMessage, JoinError, LobbyPlayer, LobbySettings, LobbyState, LobbySummary,
    OnlinePlayer, PlayerStatus,
};
use space_race_sim::track::{Track, TrackDescription};

use crate::net::{Connection, NetEvent, Network};
use crate::race::SnapshotBuffer;
use crate::screen::Screen;
use crate::settings::Script;
use crate::ui::thumbnail;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tracks>()
            .init_resource::<LobbyList>()
            .init_resource::<OnlinePlayers>()
            .init_resource::<CurrentLobby>()
            .init_resource::<Notice>()
            .add_systems(
                PreUpdate,
                (receive_lobby_events, follow_script)
                    .chain()
                    .in_set(LobbyUpdate),
            );
    }
}

/// Updates the lobby resources from the server's messages. Runs before the race and the UI.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LobbyUpdate;

/// Every track of the server, by key.
#[derive(Resource, Default)]
pub struct Tracks(pub BTreeMap<String, TrackEntry>);

pub struct TrackEntry {
    pub description: TrackDescription,
    pub track: Arc<Track>,
    /// The track seen from above, for lists and pickers.
    pub thumbnail: Handle<Image>,
}

impl Tracks {
    /// The track's name, or its key if the server did not send it.
    pub fn name<'a>(&'a self, key: &'a str) -> &'a str {
        self.0
            .get(key)
            .map_or(key, |entry| entry.description.name.as_str())
    }
}

/// The lobbies, as last listed by the server.
#[derive(Resource, Default)]
pub struct LobbyList {
    pub lobbies: Vec<LobbySummary>,
    /// Real time the list arrived, in seconds: its countdowns count from then.
    pub received_at: f64,
    /// Whether a list arrived since connecting.
    pub loaded: bool,
}

/// Everyone connected to the server, in the order they arrived, as last listed. The server only
/// tells players who browse lobbies, so this is what the browser shows and nothing else.
#[derive(Resource, Default)]
pub struct OnlinePlayers(pub Vec<OnlinePlayer>);

/// The lobby the player is in, if any.
#[derive(Resource, Default)]
pub struct CurrentLobby(pub Option<Membership>);

pub struct Membership {
    /// The player's own car id, whether or not they have a car right now.
    pub car: CarId,
    pub settings: LobbySettings,
    /// `None` until the server sends it, right after entering.
    pub state: Option<LobbyState>,
}

impl Membership {
    pub fn player(&self, car: CarId) -> Option<&LobbyPlayer> {
        self.state
            .as_ref()?
            .players
            .iter()
            .find(|player| player.car == car)
    }

    pub fn own_status(&self) -> Option<PlayerStatus> {
        self.player(self.car).map(|player| player.status)
    }
}

/// A short message for the player, shown on the lobby list, such as why joining failed.
#[derive(Resource, Default)]
pub struct Notice(pub Option<(String, f64)>);

/// How long a notice stays, in seconds.
const NOTICE_SECONDS: f64 = 5.0;

impl Notice {
    pub fn show(&mut self, text: impl Into<String>, now: f64) {
        self.0 = Some((text.into(), now + NOTICE_SECONDS));
    }

    pub fn current(&self, now: f64) -> Option<&str> {
        self.0
            .as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(text, _)| text.as_str())
    }
}

pub fn join_error_text(error: JoinError) -> &'static str {
    match error {
        JoinError::NoSuchLobby => "This lobby no longer exists.",
        JoinError::LobbyFull => "This lobby is full.",
        JoinError::InvalidSettings => "The server refused these lobby settings.",
        JoinError::UnknownTrack => "The server does not have this track.",
    }
}

/// Leaves the current lobby and goes back to the list.
pub fn leave_lobby(
    network: &Network,
    current: &mut CurrentLobby,
    next_screen: &mut NextState<Screen>,
) {
    network.send(ClientMessage::LeaveLobby);
    current.0 = None;
    next_screen.set(Screen::Lobbies);
}

fn receive_lobby_events(
    mut events: MessageReader<NetEvent>,
    mut tracks: ResMut<Tracks>,
    mut list: ResMut<LobbyList>,
    mut online: ResMut<OnlinePlayers>,
    mut current: ResMut<CurrentLobby>,
    mut notice: ResMut<Notice>,
    mut buffer: ResMut<SnapshotBuffer>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time<Real>>,
) {
    let now = time.elapsed_secs_f64();
    for event in events.read() {
        match event {
            NetEvent::Welcome { tracks: infos } => {
                tracks.0.clear();
                for info in infos {
                    match Track::build(&info.description) {
                        Ok(track) => {
                            let thumbnail = images.add(thumbnail::render(&track));
                            tracks.0.insert(
                                info.key.clone(),
                                TrackEntry {
                                    description: info.description.clone(),
                                    track: Arc::new(track),
                                    thumbnail,
                                },
                            );
                        }
                        Err(error) => {
                            error!(key = %info.key, "the server sent an invalid track: {error}")
                        }
                    }
                }
                *list = LobbyList::default();
                online.0.clear();
                current.0 = None;
            }
            NetEvent::Lobbies(lobbies) => {
                *list = LobbyList {
                    lobbies: lobbies.clone(),
                    received_at: now,
                    loaded: true,
                };
            }
            NetEvent::Players(players) => online.0 = players.clone(),
            NetEvent::JoinedLobby { car, settings, .. } => {
                *buffer = SnapshotBuffer::default();
                current.0 = Some(Membership {
                    car: *car,
                    settings: settings.clone(),
                    state: None,
                });
            }
            NetEvent::JoinFailed(error) => notice.show(join_error_text(*error), now),
            NetEvent::Lobby(state) => {
                // Ignored once the player has left: the server may have sent it just before.
                if let Some(membership) = &mut current.0 {
                    membership.state = Some(state.clone());
                }
            }
            NetEvent::Rejected(_) | NetEvent::Disconnected(_) => {
                current.0 = None;
                *list = LobbyList::default();
                online.0.clear();
            }
            NetEvent::Snapshot { .. } | NetEvent::Pong { .. } => {}
        }
    }
}

const SCRIPT_TRACK: &str = "esplanade";

/// Enters the scripted lobby once connected and listed.
fn follow_script(
    mut script: ResMut<Script>,
    connection: Res<Connection>,
    list: Res<LobbyList>,
    tracks: Res<Tracks>,
    network: Res<Network>,
) {
    if !matches!(*connection, Connection::Online { .. }) || !list.loaded {
        return;
    }
    let Some(lobby) = script.lobby.take() else {
        return;
    };

    let existing = list
        .lobbies
        .iter()
        .find(|summary| summary.settings.name == lobby.name);
    let message = match existing {
        Some(summary) => ClientMessage::JoinLobby(summary.id),
        None => {
            // The track asked for, or the test circuit when there is one, so unattended runs look
            // the same every time.
            let wanted = lobby.track.as_deref().unwrap_or(SCRIPT_TRACK);
            let Some(track) = tracks
                .0
                .keys()
                .find(|key| *key == wanted)
                .or_else(|| tracks.0.keys().next())
            else {
                error!("the server has no track to create a lobby on");
                return;
            };
            if track != wanted {
                warn!(wanted, used = %track, "no such track, creating the lobby on another one");
            }
            ClientMessage::CreateLobby(LobbySettings {
                name: lobby.name,
                track: track.clone(),
                laps: lobby.laps,
                min_players: lobby.min_players,
                max_players: 8,
            })
        }
    };
    network.send(message);
}
