//! Every lobby on the server, and every player connected to it.
//!
//! A single task owns the list of lobbies. It creates lobbies on request, finds them for players
//! who want to join, and publishes the list through a watch channel whenever a lobby changes, so
//! every browsing player gets the latest list without the lobbies knowing who browses.
//!
//! The connected players are kept here too, and published the same way: a browsing player sees
//! who else is on the server and whether they are on the list or in a lobby.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use space_race_protocol::{JoinError, LobbyId, LobbySettings, OnlinePlayer, Presence, TrackInfo};
use space_race_sim::car::CarTuning;
use tokio::sync::{mpsc, oneshot, watch};

use crate::content::Tracks;
use crate::lobby::{self, LobbyEvent, LobbyHandle, LobbyInfo, Membership, Timings};
use crate::session::{Player, PlayerId};

const COMMAND_BUFFER: usize = 256;

/// The lobbies, as seen by the sessions. Cheap to clone.
#[derive(Clone)]
pub struct Directory {
    commands: mpsc::Sender<Command>,
    lobbies: watch::Receiver<Arc<Vec<LobbyInfo>>>,
    tracks: Arc<Vec<TrackInfo>>,
    online: Online,
}

enum Command {
    Create {
        settings: LobbySettings,
        creator: Player,
        reply: oneshot::Sender<Result<Membership, JoinError>>,
    },
    Find {
        lobby: LobbyId,
        reply: oneshot::Sender<Option<LobbyHandle>>,
    },
}

impl Directory {
    /// Starts the directory on its own task. Lobbies race on `tracks`, driven with `tuning`.
    pub fn spawn(tracks: Tracks, tuning: watch::Receiver<CarTuning>, timings: Timings) -> Self {
        let (commands, command_receiver) = mpsc::channel(COMMAND_BUFFER);
        let (list, lobbies) = watch::channel(Arc::new(Vec::new()));
        let (events, event_receiver) = mpsc::unbounded_channel();
        let track_infos = tracks
            .iter()
            .map(|(key, loaded)| TrackInfo {
                key: key.clone(),
                description: loaded.description.clone(),
            })
            .collect();

        let registry = Registry {
            tracks,
            tuning,
            timings,
            next_id: 0,
            lobbies: BTreeMap::new(),
            events,
            list,
        };
        tokio::spawn(registry.run(command_receiver, event_receiver));

        Self {
            commands,
            lobbies,
            tracks: Arc::new(track_infos),
            online: Online::default(),
        }
    }

    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.tracks.as_ref().clone()
    }

    /// Every lobby, updated whenever one changes.
    pub fn lobbies(&self) -> watch::Receiver<Arc<Vec<LobbyInfo>>> {
        self.lobbies.clone()
    }

    /// Marks the player as connected until the guard is dropped. `None` if they already are.
    pub fn connect(&self, player: &Player) -> Option<OnlineGuard> {
        self.online.connect(player)
    }

    /// Everyone connected, updated whenever a player connects, leaves, enters a lobby or comes
    /// back to the list.
    pub fn online_players(&self) -> watch::Receiver<Arc<Vec<OnlinePlayer>>> {
        self.online.list.subscribe()
    }

    pub async fn create(
        &self,
        settings: LobbySettings,
        creator: Player,
    ) -> Result<Membership, JoinError> {
        let (reply, created) = oneshot::channel();
        let command = Command::Create {
            settings,
            creator,
            reply,
        };
        // The directory lives as long as the server; failing here means it is stopping.
        self.commands
            .send(command)
            .await
            .map_err(|_| JoinError::NoSuchLobby)?;
        created.await.map_err(|_| JoinError::NoSuchLobby)?
    }

    pub async fn join(&self, lobby: LobbyId, player: Player) -> Result<Membership, JoinError> {
        let (reply, found) = oneshot::channel();
        self.commands
            .send(Command::Find { lobby, reply })
            .await
            .map_err(|_| JoinError::NoSuchLobby)?;
        let handle = found.await.ok().flatten().ok_or(JoinError::NoSuchLobby)?;
        handle.join(player).await
    }
}

struct Registry {
    tracks: Tracks,
    tuning: watch::Receiver<CarTuning>,
    timings: Timings,
    next_id: u32,
    lobbies: BTreeMap<LobbyId, (LobbyHandle, LobbyInfo)>,
    events: mpsc::UnboundedSender<LobbyEvent>,
    list: watch::Sender<Arc<Vec<LobbyInfo>>>,
}

impl Registry {
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut events: mpsc::UnboundedReceiver<LobbyEvent>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.handle(command),
                    // Every handle is gone: the server is stopping.
                    None => break,
                },
                // Never `None`: the registry holds a sender.
                Some(event) = events.recv() => self.apply(event),
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Create {
                settings,
                creator,
                reply,
            } => {
                let _ = reply.send(self.create(settings, creator));
            }
            Command::Find { lobby, reply } => {
                let _ = reply.send(self.lobbies.get(&lobby).map(|(handle, _)| handle.clone()));
            }
        }
    }

    fn create(
        &mut self,
        settings: LobbySettings,
        creator: Player,
    ) -> Result<Membership, JoinError> {
        if !settings.is_valid() {
            return Err(JoinError::InvalidSettings);
        }
        let Some(track) = self.tracks.get(&settings.track) else {
            return Err(JoinError::UnknownTrack);
        };

        let id = LobbyId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let (membership, info) = lobby::spawn(
            id,
            settings,
            Arc::clone(track),
            self.tuning.clone(),
            self.timings,
            creator,
            self.events.clone(),
        );
        self.lobbies.insert(id, (membership.lobby.clone(), info));
        self.publish();
        Ok(membership)
    }

    fn apply(&mut self, event: LobbyEvent) {
        match event {
            LobbyEvent::Changed(info) => {
                // A lobby that already closed may still have had a change queued.
                let Some((_, listed)) = self.lobbies.get_mut(&info.id) else {
                    return;
                };
                *listed = info;
            }
            LobbyEvent::Closed(id) => {
                self.lobbies.remove(&id);
            }
        }
        self.publish();
    }

    fn publish(&self) {
        let list = self
            .lobbies
            .values()
            .map(|(_, info)| info.clone())
            .collect();
        self.list.send_replace(Arc::new(list));
    }
}

/// The players connected to the server, and where each of them is: an identity plays from one
/// connection at a time, and the browser shows who else is here.
///
/// The only lock of the server: it is held while one player is added, moved or removed, and while
/// the list that follows is published, never across an await, and a guard removes the player on
/// every way a connection can end, panics included.
#[derive(Clone)]
struct Online {
    connected: Arc<Mutex<Connected>>,
    list: watch::Sender<Arc<Vec<OnlinePlayer>>>,
}

/// Who is connected, by identity.
#[derive(Default)]
struct Connected {
    players: HashMap<PlayerId, Session>,
    next_arrival: u64,
}

struct Session {
    nickname: String,
    presence: Presence,
    /// Rank of arrival: the published list follows it, so nobody jumps around in it.
    arrival: u64,
}

impl Default for Online {
    fn default() -> Self {
        // The receiver is dropped here: sessions subscribe to the sender when they need the list.
        let (list, _) = watch::channel(Arc::new(Vec::new()));
        Self {
            connected: Arc::default(),
            list,
        }
    }
}

impl Online {
    fn connect(&self, player: &Player) -> Option<OnlineGuard> {
        let mut connected = self.connected.lock().expect("online players poisoned");
        if connected.players.contains_key(&player.id) {
            return None;
        }
        let arrival = connected.next_arrival;
        connected.next_arrival += 1;
        connected.players.insert(
            player.id,
            Session {
                nickname: player.nickname.clone(),
                presence: Presence::Browsing,
                arrival,
            },
        );
        self.publish(&connected);
        Some(OnlineGuard {
            online: self.clone(),
            player: player.id,
        })
    }

    fn move_to(&self, player: PlayerId, presence: Presence) {
        let mut connected = self.connected.lock().expect("online players poisoned");
        let Some(session) = connected.players.get_mut(&player) else {
            return;
        };
        session.presence = presence;
        self.publish(&connected);
    }

    fn disconnect(&self, player: PlayerId) {
        // A poisoned lock would leave the player listed forever, and there is nothing to do about
        // it here: the connection is ending either way.
        if let Ok(mut connected) = self.connected.lock() {
            connected.players.remove(&player);
            self.publish(&connected);
        }
    }

    /// Publishes the list, in the order the players connected.
    fn publish(&self, connected: &Connected) {
        let mut sessions: Vec<&Session> = connected.players.values().collect();
        sessions.sort_by_key(|session| session.arrival);
        let list = sessions
            .into_iter()
            .map(|session| OnlinePlayer {
                nickname: session.nickname.clone(),
                presence: session.presence,
            })
            .collect();
        self.list.send_replace(Arc::new(list));
    }
}

pub struct OnlineGuard {
    online: Online,
    player: PlayerId,
}

impl OnlineGuard {
    /// The player entered a lobby.
    pub fn entered_lobby(&self, lobby: LobbyId) {
        self.online.move_to(self.player, Presence::InLobby(lobby));
    }

    /// The player is back on the lobby list.
    pub fn browsing(&self) {
        self.online.move_to(self.player, Presence::Browsing);
    }
}

impl Drop for OnlineGuard {
    fn drop(&mut self) {
        self.online.disconnect(self.player);
    }
}
