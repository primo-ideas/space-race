//! Messages exchanged between the client and the server, one message per binary WebSocket frame.
//!
//! Authentication lives here too, in [`auth`], rather than in a crate of its own: a public key, a
//! nonce and a signature are three of the fields these messages carry, and nothing in the game
//! signs anything without also speaking this protocol.

use std::fmt;

use bitcode::{Decode, DecodeOwned, Encode};

use crate::auth::{Nonce, PublicKey, SignatureBytes};
use crate::ranked::{LeaderboardEntry, QueueStatus, RankedProfile};
use star_racer_sim::car::{Car, CarInput, CarTuning};
use star_racer_sim::race::StartGrade;
use star_racer_sim::track::TrackDescription;

pub mod auth;
pub mod names;
pub mod ranked;

/// Bump on every incompatible protocol change.
pub const PROTOCOL_VERSION: u32 = 29;

/// Local server port. In production, the reverse proxy terminates TLS and forwards here.
pub const DEFAULT_PORT: u16 = 8080;

pub const WS_PATH: &str = "/ws";

/// Caps the memory a peer can make us allocate with a single message.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024;

pub const MAX_NICKNAME_CHARS: usize = 24;

/// A password is at least this long, in characters, and at most [`MAX_PASSWORD_CHARS`].
pub const MIN_PASSWORD_CHARS: usize = 8;

pub const MAX_PASSWORD_CHARS: usize = 128;

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
    /// First message after connecting. Whoever the player is, the device proves it holds the
    /// private key of `public_key` (see [`ServerMessage::Challenge`]): it is a guest's identity,
    /// and what keeps a device signed in to its account.
    Hello {
        protocol_version: u32,
        public_key: PublicKey,
        login: Login,
        /// How the player's car looks, as chosen in the garage.
        look: CarLook,
    },
    /// Signature of the nonce received in [`ServerMessage::Challenge`].
    ChallengeResponse { signature: SignatureBytes },
    /// The player chose another look for their car in the garage. Only while browsing: it is what
    /// the next lobby they enter sees.
    SetLook(CarLook),
    /// Signs this device out of its account: the server forgets the device, so the next
    /// connection has to sign in again. Only while browsing; the server closes the connection.
    SignOut,
    /// Creates a lobby and enters it. Only while browsing lobbies.
    CreateLobby(LobbySettings),
    /// Only while browsing lobbies.
    JoinLobby(LobbyId),
    /// Puts the player in the lobby that will race soonest, or opens one. Only while browsing
    /// lobbies.
    QuickPlay,
    /// Enters the ranked queue. The server pulls the player into a match when it has one for
    /// them. Only while browsing lobbies; entering a lobby leaves the queue.
    QueueRanked,
    /// Leaves the ranked queue.
    LeaveQueue,
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
        /// Who the server took them for.
        identity: Identity,
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
    /// The lobby closed around the player, as a ranked match does once its results have shown. The
    /// player browses lobbies again.
    LobbyEnded,
    /// Where the player stands on the ladder. Sent after [`ServerMessage::Welcome`], and each
    /// time they come back from a lobby.
    Profile(RankedProfile),
    /// The best players, best first. Sent to a browsing player when they arrive and whenever it
    /// changes.
    Leaderboard(Vec<LeaderboardEntry>),
    /// Where the ranked queue stands while the player is in it, `None` once they are out of it.
    Queue(Option<QueueStatus>),
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
    /// A name that is not a valid nickname, or, for a guest, not a generated one.
    InvalidNickname,
    AuthenticationFailed,
    /// The same identity is already connected, from another connection.
    AlreadyConnected,
    /// [`Login::Device`] from a device no account knows, or no longer does: it has to sign in.
    NotSignedIn,
    /// No account has this name, or its password is another.
    WrongPassword,
    /// Creating an account under a name another account already has, whatever its case.
    NameTaken,
    /// Creating an account with a password shorter than [`MIN_PASSWORD_CHARS`] or longer than
    /// [`MAX_PASSWORD_CHARS`].
    InvalidPassword,
}

/// How a player comes in.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Login {
    /// Without an account, under a generated name (see [`names`]): everything but ranked.
    Guest { name: String },
    /// As the account this device last signed in to.
    Device,
    /// Signs in to an account, or creates it with `create`, and keeps this device signed in.
    Password {
        name: String,
        password: String,
        create: bool,
    },
}

/// How many pods there are to choose from, paints and neon colors. The client names and draws
/// them (`car_model.rs`); the wire only carries which.
pub const CAR_MODELS: u8 = 4;
pub const CAR_PAINTS: u8 = 10;
pub const CAR_NEONS: u8 = 8;

/// How a player's car looks: which pod, and its paint and neon, each an index into the client's
/// lists. Purely what is drawn: every pod drives the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
pub struct CarLook {
    pub model: u8,
    pub paint: u8,
    pub neon: u8,
}

impl CarLook {
    /// A look picked at random, for a player who has never been to the garage: a grid of players
    /// who never chose is not a grid of the same car.
    pub fn random() -> Self {
        let mut bytes = [0; 3];
        getrandom::fill(&mut bytes).expect("OS random generator unavailable");
        Self {
            model: bytes[0],
            paint: bytes[1],
            neon: bytes[2],
        }
        .checked()
    }

    /// The same look with anything out of range brought back in, so a peer cannot ask for a pod
    /// that does not exist.
    pub fn checked(self) -> Self {
        Self {
            model: self.model % CAR_MODELS,
            paint: self.paint % CAR_PAINTS,
            neon: self.neon % CAR_NEONS,
        }
    }
}

/// Who the server took a player for.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Identity {
    pub name: String,
    pub guest: bool,
}

/// Whether `password` is one an account can be created with.
pub fn is_valid_password(password: &str) -> bool {
    (MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&password.chars().count())
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
    /// Ranked is for players with an account: a guest asked to queue.
    AccountRequired,
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
    /// Playing without an account.
    pub guest: bool,
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
    /// A ranked match: one race for rating, among the players the matchmaker put in it. Nobody
    /// else can join, leaving counts as coming last, and the lobby closes after the results.
    pub ranked: bool,
    /// In the order they joined.
    pub players: Vec<LobbyPlayer>,
    /// The car tuning the lobby drives with, which the client predicts the player's car with.
    /// Sent again whenever the server reloads it.
    pub tuning: CarTuning,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum LobbyPhase {
    /// Fewer players than the lobby needs: everyone drives freely. A lobby opened by quick play
    /// does not wait forever: at `solo_tick` it races with whoever is there.
    Waiting { solo_tick: Option<u32> },
    /// The cars were lined up on the grid as soon as the lobby had enough players, and stand
    /// still until `start_tick`, the start. Players joining before it race too.
    Racing { start_tick: u32 },
    /// The race is over and everyone drives freely again until `end_tick`, when the lobby waits
    /// for its next race.
    Results {
        /// Best first. Players who left before finishing are not listed.
        results: Vec<RaceResult>,
        end_tick: u32,
        /// In a ranked match, what the race did to each rating. Empty otherwise.
        ratings: Vec<RatingChange>,
    },
}

/// What a ranked race did to a player's rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct RatingChange {
    pub car: CarId,
    pub before: i32,
    pub after: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LobbyPlayer {
    pub car: CarId,
    pub nickname: String,
    /// Playing without an account.
    pub guest: bool,
    /// How their car looks.
    pub look: CarLook,
    pub status: PlayerStatus,
    /// How well the player started the current or last race, once graded.
    pub start: Option<StartGrade>,
    /// Their rating, in a ranked match.
    pub rating: Option<i32>,
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
    /// [`Progress`]: star_racer_sim::race::Progress
    pub progress: f32,
    /// The boost letting go of the current drift would give, as a share of the longest boost a
    /// drift can give: 0 when not drifting or not yet, 1 at the most. The tuning stays on the
    /// server, which works these gauges out so the client can show them.
    pub drift_gauge: f32,
    /// Boost left, as a share of the longest boost a drift can give.
    pub boost_gauge: f32,
    /// What is left of the turbo being fired, as a share of what a charge gives. The meter itself
    /// is in the car, counted in charges.
    pub turbo_gauge: f32,
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
            turbo_gauge: if tuning.turbo_seconds > 0.0 {
                (car.turbo_boost / tuning.turbo_seconds).clamp(0.0, 1.0)
            } else {
                0.0
            },
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
    fn a_look_out_of_range_is_brought_back() {
        let look = CarLook {
            model: CAR_MODELS + 1,
            paint: 255,
            neon: CAR_NEONS,
        }
        .checked();
        assert!(look.model < CAR_MODELS && look.paint < CAR_PAINTS && look.neon < CAR_NEONS);
        assert_eq!(look.model, 1);
        assert_eq!(look.neon, 0);
    }

    #[test]
    fn message_round_trip() {
        for login in [
            Login::Guest {
                name: "Neon Falcon".into(),
            },
            Login::Device,
            Login::Password {
                name: "Primo".into(),
                password: "correct horse".into(),
                create: true,
            },
        ] {
            let hello = ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                public_key: [7; 32],
                login,
                look: CarLook {
                    model: 2,
                    paint: 7,
                    neon: 3,
                },
            };
            assert_eq!(decode::<ClientMessage>(&encode(&hello)).unwrap(), hello);
        }
    }

    #[test]
    fn race_messages_round_trip() {
        use glam::Vec2;

        use star_racer_sim::track::scenery::{Prop, Side};
        use star_racer_sim::track::{Elevation, Narrows, Pad, Segment};

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
                    elevation: vec![
                        Elevation {
                            at: 0.0,
                            height: 4.0,
                        },
                        Elevation {
                            at: 70.0,
                            height: 12.5,
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
                        Prop::Skyline {
                            at: 60.0,
                            side: Side::Right,
                            offset: 6.0,
                            length: 90.0,
                            depth: 50.0,
                            height: 35.0,
                        },
                        Prop::Tunnel {
                            at: 80.0,
                            length: 40.0,
                            clearance: 9.0,
                        },
                    ],
                    pads: vec![Pad {
                        at: 200.0,
                        lateral: -4.0,
                        length: 10.0,
                        width: 6.0,
                    }],
                },
            }],
            identity: Identity {
                name: "Zoë".into(),
                guest: false,
            },
        };
        let lobby = ServerMessage::Lobby(LobbyState {
            phase: LobbyPhase::Results {
                results: vec![RaceResult {
                    car: CarId(2),
                    nickname: "Zoë".into(),
                    time_ms: Some(61_250),
                }],
                end_tick: 9000,
                ratings: vec![RatingChange {
                    car: CarId(2),
                    before: 1000,
                    after: 1024,
                }],
            },
            ranked: true,
            players: vec![LobbyPlayer {
                car: CarId(2),
                nickname: "Zoë".into(),
                guest: true,
                look: CarLook {
                    model: 3,
                    paint: 1,
                    neon: 5,
                },
                status: PlayerStatus::Finished { time_ms: 61_250 },
                start: Some(StartGrade::A),
                rating: Some(1000),
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
                    body: 0.8,
                    body_rate: 1.5,
                    steering: -0.6,
                    yaw_rate: -0.5,
                    drift: -1,
                    drift_charge: 1.25,
                    drift_held: 0.75,
                    drift_button: true,
                    boost: 0.5,
                    drift_press: 0.1,
                    turbo: 1.75,
                    turbo_boost: 1.2,
                    turbo_button: true,
                },
                progress: 812.5,
                drift_gauge: 0.4,
                boost_gauge: 0.3,
                turbo_gauge: 0.6,
                input_seq: 41,
            }],
        });
        let input = ClientMessage::Input {
            input: CarInput::new(true, -0.5).with_drift(true).with_turbo(true),
            tick: 1231,
            seq: 42,
        };

        let profile = ServerMessage::Profile(RankedProfile {
            rating: 1024,
            games: 3,
            wins: 1,
            place: Some(12),
        });
        let leaderboard = ServerMessage::Leaderboard(vec![LeaderboardEntry {
            place: 1,
            nickname: "Zoë".into(),
            rating: 1650,
            games: 41,
        }]);
        let queue = ServerMessage::Queue(Some(QueueStatus { players: 3 }));

        for message in [
            welcome,
            lobby,
            snapshot,
            ServerMessage::Pong(7),
            ServerMessage::LobbyEnded,
            profile,
            leaderboard,
            queue,
        ] {
            assert_eq!(decode::<ServerMessage>(&encode(&message)).unwrap(), message);
        }
        for message in [
            input,
            ClientMessage::Ping(7),
            ClientMessage::QuickPlay,
            ClientMessage::QueueRanked,
            ClientMessage::LeaveQueue,
        ] {
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
            track: "skyway".into(),
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
