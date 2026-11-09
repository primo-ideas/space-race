//! Every lobby on the server, every player connected to it, and the ranked ladder.
//!
//! A single task owns the list of lobbies. It creates lobbies on request, finds them for players
//! who want to join, and publishes the list through a watch channel whenever a lobby changes, so
//! every browsing player gets the latest list without the lobbies knowing who browses. The same
//! task owns the ranked queue and the ratings: it forms matches out of the queue, and records what
//! finished ones did to the ladder.
//!
//! The connected players are kept here too, and published the same way: a browsing player sees
//! who else is on the server and whether they are on the list or in a lobby.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use space_race_protocol::ranked::{LeaderboardEntry, RankedProfile};
use space_race_protocol::{JoinError, LobbyId, LobbySettings, OnlinePlayer, Presence, TrackInfo};
use space_race_sim::car::CarTuning;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{self, MissedTickBehavior};
use tracing::{error, info};

use crate::content::{LoadedTrack, Tracks};
use crate::lobby::{
    self, Entrant, InfoStatus, LobbyEvent, LobbyHandle, LobbyInfo, LobbyKind, Membership, Timings,
};
use crate::matchmaking::{self, Candidate};
use crate::ranking::Ratings;
use crate::session::{Player, PlayerId};

const COMMAND_BUFFER: usize = 256;

/// How often the matchmaker looks at the queue.
const MATCHMAKING_PERIOD: Duration = Duration::from_millis(250);

/// Laps of a ranked race, and of the lobbies quick play opens.
const LAPS: u8 = 3;
/// How many players a lobby quick play opens wants, and holds.
const QUICK_PLAY_MIN_PLAYERS: u8 = 2;
const QUICK_PLAY_MAX_PLAYERS: u8 = 8;
/// How many lobbies quick play tries before opening one of its own.
const QUICK_PLAY_TRIES: usize = 3;

/// The lobbies, as seen by the sessions. Cheap to clone.
#[derive(Clone)]
pub struct Directory {
    commands: mpsc::Sender<Command>,
    lobbies: watch::Receiver<Arc<Vec<LobbyInfo>>>,
    leaderboard: watch::Receiver<Arc<Vec<LeaderboardEntry>>>,
    queue_size: watch::Receiver<u16>,
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
    QuickPlay {
        player: Player,
        /// Open a lobby even if one looks joinable: the last tries found them full.
        open: bool,
        reply: oneshot::Sender<Result<QuickPlayed, JoinError>>,
    },
    Enqueue {
        player: Player,
        matched: oneshot::Sender<Membership>,
    },
    Dequeue {
        player: PlayerId,
    },
    Profile {
        player: PlayerId,
        reply: oneshot::Sender<RankedProfile>,
    },
}

/// Where quick play sends a player.
enum QuickPlayed {
    /// A lobby to join, which may have filled up by the time they do.
    Join(LobbyHandle),
    /// A new lobby, with the player already in it.
    Opened(Membership),
}

/// A player waiting for a ranked match.
struct Queued {
    player: Player,
    since: Instant,
    /// Hands the player their place in the match. Closed once their session stops waiting.
    matched: oneshot::Sender<Membership>,
}

impl Directory {
    /// Starts the directory on its own task. Lobbies race on `tracks`, driven with `tuning`.
    pub fn spawn(
        tracks: Tracks,
        tuning: watch::Receiver<CarTuning>,
        timings: Timings,
        ratings: Ratings,
    ) -> Self {
        let (commands, command_receiver) = mpsc::channel(COMMAND_BUFFER);
        let (list, lobbies) = watch::channel(Arc::new(Vec::new()));
        let (leaderboard_sender, leaderboard) = watch::channel(Arc::new(ratings.leaderboard()));
        let (queue_size_sender, queue_size) = watch::channel(0);
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
            queue: Vec::new(),
            ratings,
            leaderboard: leaderboard_sender,
            queue_size: queue_size_sender,
        };
        tokio::spawn(registry.run(command_receiver, event_receiver));

        Self {
            commands,
            lobbies,
            leaderboard,
            queue_size,
            tracks: Arc::new(track_infos),
            online: Online::default(),
        }
    }

    /// The best players, updated whenever a match changes them.
    pub fn leaderboard(&self) -> watch::Receiver<Arc<Vec<LeaderboardEntry>>> {
        self.leaderboard.clone()
    }

    /// How many players are in the ranked queue.
    pub fn queue_size(&self) -> watch::Receiver<u16> {
        self.queue_size.clone()
    }

    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.tracks.as_ref().clone()
    }

    /// Every lobby, updated whenever one changes.
    pub fn lobbies(&self) -> watch::Receiver<Arc<Vec<LobbyInfo>>> {
        self.lobbies.clone()
    }

    /// Marks the player as connected until the guard is dropped. If they already were, from another
    /// connection, this one takes their place: the other is told through the receiver it was
    /// given, and ends. The receiver returned here is this connection's own.
    pub fn connect(&self, player: &Player) -> (OnlineGuard, Replaced) {
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

    /// Puts the player in the lobby that will race soonest, or opens one for them.
    pub async fn quick_play(&self, player: Player) -> Result<Membership, JoinError> {
        // A lobby can fill up between being picked and being joined: another is tried, and in the
        // end one of the player's own.
        for attempt in 0.. {
            let (reply, played) = oneshot::channel();
            let command = Command::QuickPlay {
                player: player.clone(),
                open: attempt >= QUICK_PLAY_TRIES,
                reply,
            };
            self.commands
                .send(command)
                .await
                .map_err(|_| JoinError::NoSuchLobby)?;
            match played.await.map_err(|_| JoinError::NoSuchLobby)?? {
                QuickPlayed::Opened(membership) => return Ok(membership),
                QuickPlayed::Join(handle) => {
                    if let Ok(membership) = handle.join(player.clone()).await {
                        return Ok(membership);
                    }
                }
            }
        }
        unreachable!("the loop only ends by returning")
    }

    /// Puts the player in the ranked queue. The receiver gets their place in a match, once there
    /// is one; dropping it, or calling [`Directory::dequeue`], takes them out of the queue.
    pub async fn enqueue(&self, player: Player) -> oneshot::Receiver<Membership> {
        let (matched, receiver) = oneshot::channel();
        // Failing means the server is stopping: the receiver then closes unanswered.
        let _ = self
            .commands
            .send(Command::Enqueue { player, matched })
            .await;
        receiver
    }

    pub async fn dequeue(&self, player: PlayerId) {
        let _ = self.commands.send(Command::Dequeue { player }).await;
    }

    /// Where the player stands on the ladder, or `None` if the server is stopping.
    pub async fn profile(&self, player: PlayerId) -> Option<RankedProfile> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Profile { player, reply })
            .await
            .ok()?;
        answer.await.ok()
    }
}

/// How much a lobby suits a player who just wants to race, or `None` if it does not. The sooner
/// its next race starts the better: already on the grid, then one more player away from starting,
/// then between races, then on its last lap. A lobby in the middle of a race is not offered: the
/// player would only watch it.
fn quick_play_score(info: &LobbyInfo) -> Option<u8> {
    if info.ranked || info.players.len() >= usize::from(info.settings.max_players) {
        return None;
    }
    match info.status {
        InfoStatus::Starting { .. } => Some(4),
        InfoStatus::Waiting if info.players.len() + 1 >= usize::from(info.settings.min_players) => {
            Some(3)
        }
        InfoStatus::Waiting => None,
        InfoStatus::Results { .. } => Some(2),
        InfoStatus::Racing { lap } if lap >= info.settings.laps => Some(1),
        InfoStatus::Racing { .. } => None,
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
    /// Longest waiting first.
    queue: Vec<Queued>,
    ratings: Ratings,
    leaderboard: watch::Sender<Arc<Vec<LeaderboardEntry>>>,
    queue_size: watch::Sender<u16>,
}

impl Registry {
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut events: mpsc::UnboundedReceiver<LobbyEvent>,
    ) {
        let mut matchmaking = time::interval(MATCHMAKING_PERIOD);
        matchmaking.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.handle(command),
                    // Every handle is gone: the server is stopping.
                    None => break,
                },
                // Never `None`: the registry holds a sender.
                Some(event) = events.recv() => self.apply(event),
                _ = matchmaking.tick() => self.matchmake(),
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
            Command::QuickPlay {
                player,
                open,
                reply,
            } => {
                let _ = reply.send(self.quick_play(player, open));
            }
            Command::Enqueue { player, matched } => {
                // One place in the queue per player, the latest.
                self.queue.retain(|queued| queued.player.id != player.id);
                self.queue.push(Queued {
                    player,
                    since: Instant::now(),
                    matched,
                });
                self.publish_queue_size();
            }
            Command::Dequeue { player } => {
                self.queue.retain(|queued| queued.player.id != player);
                self.publish_queue_size();
            }
            Command::Profile { player, reply } => {
                let _ = reply.send(self.ratings.profile(player));
            }
        }
    }

    fn create(
        &mut self,
        settings: LobbySettings,
        creator: Player,
    ) -> Result<Membership, JoinError> {
        self.open(settings, LobbyKind::Custom, creator)
    }

    fn open(
        &mut self,
        settings: LobbySettings,
        kind: LobbyKind,
        creator: Player,
    ) -> Result<Membership, JoinError> {
        if !settings.is_valid() {
            return Err(JoinError::InvalidSettings);
        }
        let Some(track) = self.tracks.get(&settings.track).map(Arc::clone) else {
            return Err(JoinError::UnknownTrack);
        };

        let id = self.next_lobby_id();
        let (membership, info) = lobby::spawn(
            id,
            settings,
            track,
            self.tuning.clone(),
            self.timings,
            kind,
            creator,
            self.events.clone(),
        );
        self.lobbies.insert(id, (membership.lobby.clone(), info));
        self.publish();
        Ok(membership)
    }

    fn next_lobby_id(&mut self) -> LobbyId {
        let id = LobbyId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// The track a new lobby races on, in turn among all of them.
    fn next_track(&self) -> Option<(&String, &Arc<LoadedTrack>)> {
        let turn = self.next_id as usize % self.tracks.len().max(1);
        self.tracks.iter().nth(turn)
    }

    fn quick_play(&mut self, player: Player, open: bool) -> Result<QuickPlayed, JoinError> {
        if !open {
            let best = self
                .lobbies
                .values()
                .filter_map(|(handle, info)| {
                    quick_play_score(info).map(|score| (score, info.players.len(), handle))
                })
                .max_by_key(|(score, players, _)| (*score, *players));
            if let Some((_, _, handle)) = best {
                return Ok(QuickPlayed::Join(handle.clone()));
            }
        }
        let track = self
            .next_track()
            .map(|(key, _)| key.clone())
            .ok_or(JoinError::UnknownTrack)?;
        let settings = LobbySettings {
            name: format!("Quick Play {}", self.next_id),
            track,
            laps: LAPS,
            min_players: QUICK_PLAY_MIN_PLAYERS,
            max_players: QUICK_PLAY_MAX_PLAYERS,
        };
        self.open(settings, LobbyKind::QuickPlay, player)
            .map(QuickPlayed::Opened)
    }

    /// Starts every match the queue can make now.
    fn matchmake(&mut self) {
        self.queue.retain(|queued| !queued.matched.is_closed());
        let now = Instant::now();
        loop {
            let candidates: Vec<Candidate> = self
                .queue
                .iter()
                .map(|queued| Candidate {
                    rating: self.ratings.standing(queued.player.id).0,
                    waited: now.saturating_duration_since(queued.since),
                })
                .collect();
            let Some(mut picked) = matchmaking::pick(&candidates, self.timings.gather) else {
                break;
            };
            // Taken out from the end, so the indices left to take stay right.
            picked.sort_unstable_by(|a, b| b.cmp(a));
            let players: Vec<Queued> = picked
                .into_iter()
                .map(|index| self.queue.remove(index))
                .collect();
            self.open_match(players);
        }
        self.publish_queue_size();
    }

    fn open_match(&mut self, players: Vec<Queued>) {
        let Some((key, track)) = self
            .next_track()
            .map(|(key, track)| (key.clone(), Arc::clone(track)))
        else {
            error!("no track to race a ranked match on");
            return;
        };
        let id = self.next_lobby_id();
        let size = players.len() as u8;
        let settings = LobbySettings {
            name: format!("Ranked match {}", id.0),
            track: key,
            laps: LAPS,
            min_players: size,
            max_players: size,
        };
        let (matched, entrants): (Vec<_>, Vec<_>) = players
            .into_iter()
            .map(|queued| {
                let (rating, games) = self.ratings.standing(queued.player.id);
                let entrant = Entrant {
                    player: queued.player,
                    rating,
                    games,
                };
                (queued.matched, entrant)
            })
            .unzip();
        info!(lobby = %id, players = size, "ranked match found");
        let (memberships, info) = lobby::spawn_match(
            id,
            settings,
            track,
            self.tuning.clone(),
            self.timings,
            entrants,
            self.events.clone(),
        );
        let handle = memberships[0].lobby.clone();
        self.lobbies.insert(id, (handle, info));
        for (sender, membership) in matched.into_iter().zip(memberships) {
            if let Err(membership) = sender.send(membership) {
                // Their session stopped waiting a moment too late: they are simply not there.
                tokio::spawn(async move { membership.lobby.leave(membership.car).await });
            }
        }
    }

    fn publish_queue_size(&self) {
        let size = u16::try_from(self.queue.len()).unwrap_or(u16::MAX);
        self.queue_size.send_if_modified(|published| {
            let changed = *published != size;
            *published = size;
            changed
        });
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
            LobbyEvent::MatchFinished(placed) => {
                self.ratings.apply(&placed);
                self.leaderboard
                    .send_replace(Arc::new(self.ratings.leaderboard()));
                return;
            }
        }
        self.publish();
    }

    /// Lists the lobbies players can see. A ranked match is nobody else's business.
    fn publish(&self) {
        let list = self
            .lobbies
            .values()
            .filter(|(_, info)| !info.ranked)
            .map(|(_, info)| info.clone())
            .collect();
        self.list.send_replace(Arc::new(list));
    }
}

/// Fires when another connection of the same player has taken this one's place, which must then
/// end.
pub type Replaced = oneshot::Receiver<()>;

/// The players connected to the server, and where each of them is: an identity plays from one
/// connection at a time, and the browser shows who else is here.
///
/// The newest connection of an identity wins. A phone that reloads the page, comes back to a
/// killed tab, or moves from Wi-Fi to its mobile network connects again while the server still
/// holds the old connection, which may never say it has gone; turning the new one away would leave
/// the player outside until it timed out. The new one has just proved it holds the key, so it is
/// the player.
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
    next_connection: u64,
}

struct Session {
    nickname: String,
    presence: Presence,
    /// Rank of arrival: the published list follows it, so nobody jumps around in it. A connection
    /// that takes another's place keeps it.
    arrival: u64,
    /// Which connection this is, so that one replaced cannot move or remove its successor.
    connection: u64,
    /// Tells the connection it has been replaced.
    replace: Option<oneshot::Sender<()>>,
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
    fn connect(&self, player: &Player) -> (OnlineGuard, Replaced) {
        let mut connected = self.connected.lock().expect("online players poisoned");
        let connection = connected.next_connection;
        connected.next_connection += 1;
        let arrival = match connected.players.get_mut(&player.id) {
            Some(previous) => {
                if let Some(replace) = previous.replace.take() {
                    // The previous connection may already be on its way out: nothing to tell.
                    let _ = replace.send(());
                }
                previous.arrival
            }
            None => {
                connected.next_arrival += 1;
                connected.next_arrival - 1
            }
        };
        let (replace, replaced) = oneshot::channel();
        connected.players.insert(
            player.id,
            Session {
                nickname: player.nickname.clone(),
                presence: Presence::Browsing,
                arrival,
                connection,
                replace: Some(replace),
            },
        );
        self.publish(&connected);
        let guard = OnlineGuard {
            online: self.clone(),
            player: player.id,
            connection,
        };
        (guard, replaced)
    }

    fn move_to(&self, player: PlayerId, connection: u64, presence: Presence) {
        let mut connected = self.connected.lock().expect("online players poisoned");
        let Some(session) = connected
            .players
            .get_mut(&player)
            .filter(|session| session.connection == connection)
        else {
            return;
        };
        session.presence = presence;
        self.publish(&connected);
    }

    fn disconnect(&self, player: PlayerId, connection: u64) {
        // A poisoned lock would leave the player listed forever, and there is nothing to do about
        // it here: the connection is ending either way.
        if let Ok(mut connected) = self.connected.lock()
            && connected
                .players
                .get(&player)
                .is_some_and(|session| session.connection == connection)
        {
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
    connection: u64,
}

impl OnlineGuard {
    /// The player entered a lobby.
    pub fn entered_lobby(&self, lobby: LobbyId) {
        self.online
            .move_to(self.player, self.connection, Presence::InLobby(lobby));
    }

    /// The player is back on the lobby list.
    pub fn browsing(&self) {
        self.online
            .move_to(self.player, self.connection, Presence::Browsing);
    }
}

impl Drop for OnlineGuard {
    fn drop(&mut self) {
        self.online.disconnect(self.player, self.connection);
    }
}
