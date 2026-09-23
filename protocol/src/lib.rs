//! Messages exchanged between the client and the server, one message per binary WebSocket frame.
//!
//! Authentication lives here too, in [`auth`], rather than in a crate of its own: a public key, a
//! nonce and a signature are three of the fields these messages carry, and nothing in the game
//! signs anything without also speaking this protocol.

use std::fmt;

use bitcode::{Decode, DecodeOwned, Encode};

use crate::auth::{Nonce, PublicKey, SignatureBytes};
use space_race_sim::car::{Car, CarInput, CarTuning};
use space_race_sim::race::StartGrade;
use space_race_sim::track::TrackDescription;

pub mod auth;

/// Bump on every incompatible protocol change.
pub const PROTOCOL_VERSION: u32 = 14;

/// Local server port. In production, the reverse proxy terminates TLS and forwards here.
pub const DEFAULT_PORT: u16 = 8080;

pub const WS_PATH: &str = "/ws";

/// Caps the memory a peer can make us allocate with a single message.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024;

pub const MAX_NICKNAME_CHARS: usize = 24;

pub const MAX_LOBBY_NAME_CHARS: usize = 32;

/// Most players a lobby can hold, spectators included.
pub const MAX_LOBBY_PLAYERS: u8 = 16;

pub const MAX_LAPS: u8 = 20;

/// Furthest ahead of the server a client may schedule an input, in ticks: half a second, more
/// than any playable round trip. An input asked for further ahead takes effect sooner than asked,
/// and the client's prediction is corrected. It also bounds how long before the start a press can
/// be placed on it.
pub const MAX_INPUT_LEAD: u32 = 30;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum ClientMessage {
    /// First message after connecting.
    Hello {
        protocol_version: u32,
        nickname: String,
        public_key: PublicKey,
    },
    /// Signature of the nonce received in [`ServerMessage::Challenge`].
    ChallengeResponse { signature: SignatureBytes },
    /// Creates a lobby and enters it. Only while browsing lobbies.
    CreateLobby(LobbySettings),
    /// Only while browsing lobbies.
    JoinLobby(LobbyId),
    /// Back to browsing lobbies. Only while in a lobby.
    LeaveLobby,
    /// What the player presses now. Sent only when it changes: the connection is reliable, so the
    /// server always knows the latest input.
    Input {
        input: CarInput,
        /// The tick the input takes effect on: the one the client applied it on as it predicted
        /// the player's car, ahead of the server by a round trip and a margin. The server holds it
        /// until then, so both sides step the car with the same inputs on the same ticks, and
        /// grades the start on it. Never before the server's next tick, nor more than
        /// [`MAX_INPUT_LEAD`] ahead, nor before an earlier input: the server clamps it.
        tick: u32,
        /// Counts the inputs sent in this lobby, from 1. Snapshots say which of them the server
        /// has acted on (see [`CarSnapshot::input_seq`]).
        seq: u32,
    },
    /// Answered at once with a [`ServerMessage::Pong`] carrying the same number, whether browsing
    /// or in a lobby. The client times the round trip; the server does not.
    Ping(u32),
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum ServerMessage {
    /// The client must prove it owns the private key matching its public key.
    Challenge {
        nonce: Nonce,
    },
    /// The player is authenticated and browses lobbies. `tracks` holds every track a lobby can race
    /// on.
    Welcome {
        tracks: Vec<TrackInfo>,
    },
    /// Every lobby. Sent to a browsing player when they arrive and whenever a lobby changes.
    Lobbies(Vec<LobbySummary>),
    /// Everyone connected to the server. Sent to a browsing player when they arrive and whenever
    /// a player connects, leaves, or moves between the lobby list and a lobby.
    Players(Vec<OnlinePlayer>),
    /// The player entered a lobby, by creating or joining it, and is `car` there.
    JoinedLobby {
        lobby: LobbyId,
        car: CarId,
        settings: LobbySettings,
    },
    /// Creating or joining a lobby failed. The player keeps browsing.
    JoinFailed(JoinError),
    /// The phase and players of the lobby. Sent on entering it and whenever they change.
    Lobby(LobbyState),
    /// Every car of the lobby, once per simulation tick.
    Snapshot(Snapshot),
    /// The answer to [`ClientMessage::Ping`], sent as soon as the ping arrives.
    Pong(u32),
    Rejected(RejectReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum RejectReason {
    ProtocolMismatch {
        server_version: u32,
    },
    InvalidNickname,
    AuthenticationFailed,
    /// The same identity is already connected, from another connection.
    AlreadyConnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum JoinError {
    /// The lobby does not exist, or no longer does: it closes when its last player leaves.
    NoSuchLobby,
    LobbyFull,
    /// The settings fail [`LobbySettings::is_valid`].
    InvalidSettings,
    /// No track has this key.
    UnknownTrack,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct TrackInfo {
    /// What lobby settings name the track by.
    pub key: String,
    pub description: TrackDescription,
}

/// Identifies a lobby for as long as it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub struct LobbyId(pub u32);

impl fmt::Display for LobbyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Chosen by the player who creates the lobby, and fixed for its lifetime.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LobbySettings {
    pub name: String,
    /// [`TrackInfo::key`] of the track.
    pub track: String,
    pub laps: u8,
    /// Players needed before the cars line up for a race.
    pub min_players: u8,
    /// Players the lobby holds, spectators included.
    pub max_players: u8,
}

impl LobbySettings {
    pub fn is_valid(&self) -> bool {
        is_valid_lobby_name(&self.name)
            && (1..=MAX_LAPS).contains(&self.laps)
            && (1..=self.max_players).contains(&self.min_players)
            && self.max_players <= MAX_LOBBY_PLAYERS
    }
}

/// A lobby as listed to browsing players.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct LobbySummary {
    pub id: LobbyId,
    pub settings: LobbySettings,
    /// Nicknames, in the order the players joined.
    pub players: Vec<String>,
    pub status: LobbyStatus,
}

/// What a lobby is doing, as listed. Durations count from when the message was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum LobbyStatus {
    /// Fewer players than the lobby needs to start a race.
    Waiting,
    /// The cars wait on the grid, and the race starts in `remaining_ms`. Joining still races.
    Starting { remaining_ms: u32 },
    /// A race is on. `lap` is the leader's, 0 until a car crosses the start line.
    Racing { lap: u8 },
    /// Showing the results of the last race for `remaining_ms` more.
    Results { remaining_ms: u32 },
}

/// A player connected to the server, as the lobby browser lists them.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct OnlinePlayer {
    pub nickname: String,
    pub presence: Presence,
}

/// Where a connected player is. A lobby is named by its id alone: the lobby list already says
/// what it is called and what it is doing, so the two can never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum Presence {
    /// In the menus, browsing the lobby list.
    Browsing,
    InLobby(LobbyId),
}

/// A lobby as seen from inside. Ticks are simulation ticks, the clock of
/// [`ServerMessage::Snapshot`].
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct LobbyState {
    pub phase: LobbyPhase,
    /// In the order they joined.
    pub players: Vec<LobbyPlayer>,
    /// The car tuning the lobby drives with, which the client predicts the player's car with.
    /// Sent again whenever the server reloads it.
    pub tuning: CarTuning,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum LobbyPhase {
    /// Fewer players than the lobby needs: everyone drives freely.
    Waiting,
    /// The cars were lined up on the grid as soon as the lobby had enough players, and stand
    /// still until `start_tick`, the start. Players joining before it race too.
    Racing { start_tick: u32 },
    /// The race is over and everyone drives freely again until `end_tick`, when the lobby waits
    /// for its next race.
    Results {
        /// Best first. Players who left before finishing are not listed.
        results: Vec<RaceResult>,
        end_tick: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LobbyPlayer {
    pub car: CarId,
    pub nickname: String,
    pub status: PlayerStatus,
    /// How well the player started the current or last race, once graded.
    pub start: Option<StartGrade>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PlayerStatus {
    /// Driving freely, outside a race.
    Driving,
    Racing,
    /// Crossed the finish line `time_ms` milliseconds after the start. The car keeps driving,
    /// but by itself: the player watches it until the next race lines up.
    Finished {
        time_ms: u32,
    },
    /// Joined during a race: has no car, watches, and races in the next one.
    Spectating,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct RaceResult {
    pub car: CarId,
    pub nickname: String,
    /// `None` if the player did not finish in time.
    pub time_ms: Option<u32>,
}

/// Identifies a player within a lobby, and the car they drive there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub struct CarId(pub u16);

impl fmt::Display for CarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Snapshot {
    /// Simulation tick this state was reached at. Ticks are `TICK_SECONDS` apart.
    pub tick: u32,
    /// Every car on the track. Spectators have none.
    pub cars: Vec<CarSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
pub struct CarSnapshot {
    pub id: CarId,
    pub car: Car,
    /// Meters driven since the start line during a race (see [`Progress`]), 0 outside races.
    ///
    /// [`Progress`]: space_race_sim::race::Progress
    pub progress: f32,
    /// The boost letting go of the current drift would give, as a share of the longest boost a
    /// drift can give: 0 when not drifting or not yet, 1 at the most. The tuning stays on the
    /// server, which works these gauges out so the client can show them.
    pub drift_gauge: f32,
    /// Boost left, as a share of the longest boost a drift can give.
    pub boost_gauge: f32,
    /// The last of the driver's inputs in effect when this snapshot was taken, by its
    /// [`ClientMessage::Input::seq`]: the state above already includes it. On the player's own
    /// car, it tells them which of their inputs the server has acted on, which is how the client
    /// times its input lag. 0 before any input.
    pub input_seq: u32,
}

impl CarSnapshot {
    /// The snapshot of `car`, driven with inputs up to `input_seq`, with its gauges worked out
    /// from `tuning`.
    pub fn new(id: CarId, car: Car, progress: f32, input_seq: u32, tuning: &CarTuning) -> Self {
        let share = |seconds: f32| {
            if tuning.drift_max_boost > 0.0 {
                (seconds / tuning.drift_max_boost).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        Self {
            id,
            car,
            progress,
            drift_gauge: share(car.pending_boost(tuning)),
            boost_gauge: share(car.boost),
            input_seq,
        }
    }
}

pub fn encode<T: Encode + ?Sized>(message: &T) -> Vec<u8> {
    bitcode::encode(message)
}

pub fn decode<T: DecodeOwned>(bytes: &[u8]) -> Result<T, bitcode::Error> {
    bitcode::decode(bytes)
}

pub fn is_valid_nickname(nickname: &str) -> bool {
    is_valid_name(nickname, MAX_NICKNAME_CHARS)
}

pub fn is_valid_lobby_name(name: &str) -> bool {
    is_valid_name(name, MAX_LOBBY_NAME_CHARS)
}

/// 1 to `max_chars` characters, no leading or trailing whitespace, no control characters.
fn is_valid_name(name: &str, max_chars: usize) -> bool {
    let chars = name.chars().count();
    (1..=max_chars).contains(&chars) && name.trim() == name && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trip() {
        let hello = ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            nickname: "Primo".into(),
            public_key: [7; 32],
        };

        assert_eq!(decode::<ClientMessage>(&encode(&hello)).unwrap(), hello);
    }

    #[test]
    fn race_messages_round_trip() {
        use glam::Vec2;

        use space_race_sim::track::scenery::{Prop, Side};
        use space_race_sim::track::{Narrows, Segment};

        let welcome = ServerMessage::Welcome {
            tracks: vec![TrackInfo {
                key: "oval".into(),
                description: TrackDescription {
                    name: "Oval".into(),
                    width: 16.0,
                    segments: vec![
                        Segment::Straight { length: 50.0 },
                        Segment::Turn {
                            angle: -90.0,
                            radius: 30.0,
                            transition: 10.0,
                            banking: 15.0,
                        },
                    ],
                    narrows: vec![Narrows {
                        at: 120.0,
                        length: 30.0,
                        width: 12.0,
                        blend: 20.0,
                    }],
                    scenery: vec![
                        Prop::Gantry {
                            at: 0.0,
                            clearance: 7.0,
                            depth: 2.0,
                        },
                        Prop::Pylon {
                            at: 40.0,
                            side: Side::Left,
                            offset: 3.0,
                            height: 16.0,
                        },
                    ],
                },
            }],
        };
        let lobby = ServerMessage::Lobby(LobbyState {
            phase: LobbyPhase::Results {
                results: vec![RaceResult {
                    car: CarId(2),
                    nickname: "Zoë".into(),
                    time_ms: Some(61_250),
                }],
                end_tick: 9000,
            },
            players: vec![LobbyPlayer {
                car: CarId(2),
                nickname: "Zoë".into(),
                status: PlayerStatus::Finished { time_ms: 61_250 },
                start: Some(StartGrade::A),
            }],
            // The server's own tuning: every field of it crosses the wire.
            tuning: ron::from_str::<CarTuning>(include_str!("../../server/data/car.ron")).unwrap(),
        });
        let snapshot = ServerMessage::Snapshot(Snapshot {
            tick: 1234,
            cars: vec![CarSnapshot {
                id: CarId(3),
                car: Car {
                    position: Vec2::new(1.5, -2.0),
                    velocity: Vec2::new(30.0, 0.25),
                    heading: 0.1,
                    slip: -0.4,
                    yaw_rate: -0.5,
                    drift: -1,
                    drift_charge: 1.25,
                    boost: 0.5,
                },
                progress: 812.5,
                drift_gauge: 0.4,
                boost_gauge: 0.3,
                input_seq: 41,
            }],
        });
        let input = ClientMessage::Input {
            input: CarInput::new(true, -0.5).with_drift(true),
            tick: 1231,
            seq: 42,
        };

        for message in [welcome, lobby, snapshot, ServerMessage::Pong(7)] {
            assert_eq!(decode::<ServerMessage>(&encode(&message)).unwrap(), message);
        }
        for message in [input, ClientMessage::Ping(7)] {
            assert_eq!(decode::<ClientMessage>(&encode(&message)).unwrap(), message);
        }
    }

    #[test]
    fn nickname_validation() {
        assert!(is_valid_nickname("Primo"));
        assert!(is_valid_nickname("Zoë 2000"));
        assert!(!is_valid_nickname(""));
        assert!(!is_valid_nickname(" Primo"));
        assert!(!is_valid_nickname("Pri\nmo"));
        assert!(is_valid_nickname(&"é".repeat(MAX_NICKNAME_CHARS)));
        assert!(!is_valid_nickname(&"é".repeat(MAX_NICKNAME_CHARS + 1)));
    }

    #[test]
    fn lobby_settings_validation() {
        let settings = LobbySettings {
            name: "Friday night".into(),
            track: "esplanade".into(),
            laps: 3,
            min_players: 2,
            max_players: 8,
        };
        assert!(settings.is_valid());

        let invalid = [
            LobbySettings {
                name: " Friday".into(),
                ..settings.clone()
            },
            LobbySettings {
                laps: 0,
                ..settings.clone()
            },
            LobbySettings {
                laps: MAX_LAPS + 1,
                ..settings.clone()
            },
            LobbySettings {
                min_players: 0,
                ..settings.clone()
            },
            LobbySettings {
                min_players: 9,
                ..settings.clone()
            },
            LobbySettings {
                max_players: MAX_LOBBY_PLAYERS + 1,
                ..settings.clone()
            },
        ];
        for settings in invalid {
            assert!(!settings.is_valid(), "{settings:?}");
        }
    }
}
