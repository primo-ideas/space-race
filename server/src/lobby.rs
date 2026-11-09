//! A lobby: players racing together on one track, race after race.
//!
//! A lobby cycles through phases. It **waits**, everyone driving freely, until it holds
//! `min_players`. The **race** then lines the cars up on the grid at once, holds them there for a
//! few seconds, and lasts until every racer has driven the laps, or until a timeout after the first
//! one finished. The **results** show for a few seconds: whoever crossed the line watches their car
//! drive itself, everyone else drives freely again, then the cycle begins anew. Players who join on
//! the grid race; players who join after the start spectate the race and race in the next one.
//!
//! One task owns the whole lobby and advances it at `TICK_RATE`. Sessions send it commands, and
//! receive snapshots through a channel of their own and the lobby state through a watch channel,
//! which always holds the latest state. No locks: the lobby state is never shared. The task ends
//! when the last player leaves.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use space_race_protocol::{
    self as protocol, CarId, CarSnapshot, JoinError, LobbyId, LobbyPhase, LobbyPlayer,
    LobbySettings, LobbyState, LobbyStatus, LobbySummary, MAX_INPUT_LEAD, PlayerStatus, RaceResult,
    RatingChange, ServerMessage, Snapshot,
};
use space_race_sim::autopilot;
use space_race_sim::car::{Car, CarInput, CarTuning};
use space_race_sim::race::{self, Progress, RaceStanding, StartTiming};
use space_race_sim::track::Track;
use space_race_sim::{TICK_DURATION, TICK_RATE, TICK_SECONDS};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{self, MissedTickBehavior};
use tracing::{debug, info};

use crate::content::LoadedTrack;
use crate::ranking::{Placed, rating_changes};
use crate::session::{Player, PlayerId};

/// Snapshots waiting to be sent to one player. A player whose connection cannot keep up skips
/// snapshots rather than slowing the lobby down: the next one supersedes them anyway.
const SNAPSHOT_BUFFER: usize = 8;

const COMMAND_BUFFER: usize = 256;

/// Most inputs a player may have waiting for their tick at once. A client leads the server by at
/// most `MAX_INPUT_LEAD`, half a second, in which even a stick read at 144 frames a second
/// changes fewer times than this.
const MAX_SCHEDULED: usize = 256;

/// How long each timed phase lasts.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    /// Standing on the grid before the start: time to look at the screen, then the start lights.
    pub grid: Duration,
    /// Once the first car has finished, how long the others have to finish too.
    pub finish_timeout: Duration,
    /// Showing the results.
    pub results: Duration,
    /// How long a lobby opened by quick play waits for a second player before racing anyway.
    pub quick_solo: Duration,
    /// Standing on the grid before a ranked race. Longer than `grid`: nobody chose the moment a
    /// match was found, and a browser that has not raced yet can hold its page for many seconds
    /// compiling the race's shaders, which must not cost a start that counts.
    pub ranked_grid: Duration,
    /// How long the matchmaker lets a match smaller than full wait for others to turn up.
    pub gather: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            grid: Duration::from_secs(3 + u64::from(race::START_LIGHTS_SECONDS)),
            finish_timeout: Duration::from_secs(30),
            results: Duration::from_secs(8),
            quick_solo: Duration::from_secs(15),
            ranked_grid: Duration::from_secs(12),
            gather: Duration::from_secs(15),
        }
    }
}

fn ticks(duration: Duration) -> u32 {
    (duration.as_secs_f64() * f64::from(TICK_RATE)).round() as u32
}

/// What a car that has crossed the line is driven with: the drifting autopilot, the style worth
/// watching.
fn self_driven(car: &Car, track: &Track) -> CarInput {
    autopilot::drive_in_style(autopilot::Style::Drift, car, track)
}

/// The lobby as the directory lists it. Unlike a [`LobbySummary`], it holds deadlines rather than
/// durations, so it stays right however long it waits before being sent.
#[derive(Debug, Clone)]
pub struct LobbyInfo {
    pub id: LobbyId,
    pub settings: LobbySettings,
    pub players: Vec<String>,
    pub status: InfoStatus,
    /// A ranked match, which nobody can join and which is not listed.
    pub ranked: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum InfoStatus {
    Waiting,
    Starting { at: Instant },
    Racing { lap: u8 },
    Results { until: Instant },
}

impl LobbyInfo {
    pub fn summary(&self, now: Instant) -> LobbySummary {
        let remaining_ms = |deadline: Instant| {
            u32::try_from(deadline.saturating_duration_since(now).as_millis()).unwrap_or(u32::MAX)
        };
        LobbySummary {
            id: self.id,
            settings: self.settings.clone(),
            players: self.players.clone(),
            status: match self.status {
                InfoStatus::Waiting => LobbyStatus::Waiting,
                InfoStatus::Starting { at } => LobbyStatus::Starting {
                    remaining_ms: remaining_ms(at),
                },
                InfoStatus::Racing { lap } => LobbyStatus::Racing { lap },
                InfoStatus::Results { until } => LobbyStatus::Results {
                    remaining_ms: remaining_ms(until),
                },
            },
        }
    }
}

/// What a lobby tells the directory.
pub enum LobbyEvent {
    Changed(LobbyInfo),
    /// The last player left, and the lobby's task has ended.
    Closed(LobbyId),
    /// A ranked race is over: what it did to everyone's rating.
    MatchFinished(Vec<Placed>),
}

/// How a lobby came to be, which decides how long it waits and how it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyKind {
    /// Opened by a player, with their settings.
    Custom,
    /// Opened by quick play: it waits for a second player, but not forever.
    QuickPlay,
}

/// A player the matchmaker put in a ranked match, and where they stood when it started.
#[derive(Debug, Clone)]
pub struct Entrant {
    pub player: Player,
    pub rating: i32,
    pub games: u32,
}

/// A running lobby, as seen by the directory and the sessions. Cheap to clone.
#[derive(Clone)]
pub struct LobbyHandle {
    id: LobbyId,
    commands: mpsc::Sender<Command>,
}

/// A player's place in a lobby.
pub struct Membership {
    pub lobby: LobbyHandle,
    pub car: CarId,
    pub settings: LobbySettings,
    /// Encoded [`ServerMessage::Snapshot`] messages, ready to send.
    pub snapshots: mpsc::Receiver<Bytes>,
    /// The encoded [`ServerMessage::Lobby`] message, marked as changed so it is sent at once.
    pub state: watch::Receiver<Bytes>,
}

enum Command {
    Join {
        player: Player,
        reply: oneshot::Sender<Result<Joined, JoinError>>,
    },
    Input {
        car: CarId,
        input: CarInput,
        /// The tick it takes effect on, as the client asks.
        tick: u32,
        /// The input's number, echoed in the snapshots once it is driving the car.
        seq: u32,
    },
    Leave {
        car: CarId,
    },
}

/// A membership, before the handle is attached to it.
struct Joined {
    car: CarId,
    settings: LobbySettings,
    snapshots: mpsc::Receiver<Bytes>,
    state: watch::Receiver<Bytes>,
}

impl LobbyHandle {
    pub fn id(&self) -> LobbyId {
        self.id
    }

    pub async fn join(&self, player: Player) -> Result<Membership, JoinError> {
        let (reply, joined) = oneshot::channel();
        // A lobby that stopped taking commands has closed since it was found: it is gone.
        self.commands
            .send(Command::Join { player, reply })
            .await
            .map_err(|_| JoinError::NoSuchLobby)?;
        let joined = joined.await.map_err(|_| JoinError::NoSuchLobby)??;
        Ok(self.membership(joined))
    }

    pub async fn input(&self, car: CarId, input: CarInput, tick: u32, seq: u32) {
        let _ = self
            .commands
            .send(Command::Input {
                car,
                input,
                tick,
                seq,
            })
            .await;
    }

    pub async fn leave(&self, car: CarId) {
        let _ = self.commands.send(Command::Leave { car }).await;
    }

    fn membership(&self, joined: Joined) -> Membership {
        Membership {
            lobby: self.clone(),
            car: joined.car,
            settings: joined.settings,
            snapshots: joined.snapshots,
            state: joined.state,
        }
    }
}

/// Starts a lobby on its own task, with its creator already in it, and returns the creator's
/// membership along with the lobby as the directory should list it.
///
/// `settings` must be valid: the directory checks them.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    id: LobbyId,
    settings: LobbySettings,
    track: Arc<LoadedTrack>,
    tuning: watch::Receiver<CarTuning>,
    timings: Timings,
    kind: LobbyKind,
    creator: Player,
    events: mpsc::UnboundedSender<LobbyEvent>,
) -> (Membership, LobbyInfo) {
    let mut lobby = Lobby::new(id, settings, track, tuning.borrow().clone(), timings, kind);
    let joined = lobby
        .add_member(creator)
        .expect("a new lobby has room for its creator");
    let (mut memberships, info) = launch(lobby, vec![joined], tuning, events);
    (memberships.remove(0), info)
}

/// Starts a ranked match on its own task, with all its players in it and the race already lining
/// up, and returns each one's membership, in the order given.
pub fn spawn_match(
    id: LobbyId,
    settings: LobbySettings,
    track: Arc<LoadedTrack>,
    tuning: watch::Receiver<CarTuning>,
    timings: Timings,
    entrants: Vec<Entrant>,
    events: mpsc::UnboundedSender<LobbyEvent>,
) -> (Vec<Membership>, LobbyInfo) {
    let kind = LobbyKind::Custom;
    let mut lobby = Lobby::new(id, settings, track, tuning.borrow().clone(), timings, kind);
    let joined = lobby.begin_match(entrants);
    launch(lobby, joined, tuning, events)
}

/// Hands a lobby holding its first players to a task of its own.
fn launch(
    mut lobby: Lobby,
    joined: Vec<(CarId, mpsc::Receiver<Bytes>)>,
    tuning: watch::Receiver<CarTuning>,
    events: mpsc::UnboundedSender<LobbyEvent>,
) -> (Vec<Membership>, LobbyInfo) {
    lobby.changed = false;
    let id = lobby.id;
    let (state_sender, state) = watch::channel(lobby.encoded_state());
    let (commands, receiver) = mpsc::channel(COMMAND_BUFFER);
    let handle = LobbyHandle { id, commands };
    let memberships = joined
        .into_iter()
        .map(|(car, snapshots)| {
            let mut state = state.clone();
            state.mark_changed();
            handle.membership(Joined {
                car,
                settings: lobby.settings.clone(),
                snapshots,
                state,
            })
        })
        .collect();
    let info = lobby.info(Instant::now());

    info!(lobby = %id, name = %lobby.settings.name, track = %lobby.settings.track, "lobby opened");
    tokio::spawn(lobby.run(receiver, tuning, state_sender, events));
    (memberships, info)
}

/// The lobby state and rules, apart from the task and its timing, so tests can step it tick by
/// tick.
pub struct Lobby {
    id: LobbyId,
    settings: LobbySettings,
    track: Arc<LoadedTrack>,
    tuning: CarTuning,
    timings: Timings,
    tick: u32,
    phase: Phase,
    /// In the order they joined.
    members: Vec<Member>,
    next_car: u16,
    /// Whether the lobby state or its listing changed since they were last published.
    changed: bool,
    /// How many ticks a lobby opened by quick play waits for a second player.
    solo_after: Option<u32>,
    /// When it stops waiting.
    solo_tick: Option<u32>,
    /// What makes this a ranked match, if it is one.
    ranked: Option<RankedMatch>,
    /// What the race just did to the ratings, for the task to tell the directory.
    outcome: Option<Vec<Placed>>,
}

/// The players of a ranked match, whether or not they are still in it.
struct RankedMatch {
    roster: Vec<Rostered>,
    /// Those who left before finishing, in the order they left.
    forfeited: Vec<PlayerId>,
}

struct Rostered {
    entrant: Entrant,
    car: CarId,
}

enum Phase {
    Waiting,
    Racing(Race),
    Results {
        results: Vec<RaceResult>,
        end_tick: u32,
        ratings: Vec<RatingChange>,
    },
}

struct Race {
    start_tick: u32,
    /// In finishing order, including players who left after finishing.
    finishers: Vec<RaceResult>,
    first_finish_tick: Option<u32>,
    /// The leader's lap, for the lobby listing.
    leader_lap: u8,
}

struct Member {
    player: Player,
    car_id: CarId,
    status: PlayerStatus,
    /// Meaningless while spectating.
    car: Car,
    input: CarInput,
    /// The number of `input`, which the snapshots carry back.
    input_seq: u32,
    /// Inputs that have arrived and wait for the tick they take effect on, in the order sent.
    scheduled: VecDeque<Scheduled>,
    /// When the accelerator was pressed, and how the last race started.
    start: StartTiming,
    /// Meaningful while racing.
    progress: Progress,
    snapshots: mpsc::Sender<Bytes>,
}

/// An input waiting for the tick it takes effect on.
struct Scheduled {
    tick: u32,
    input: CarInput,
    seq: u32,
}

impl Lobby {
    fn new(
        id: LobbyId,
        settings: LobbySettings,
        track: Arc<LoadedTrack>,
        tuning: CarTuning,
        timings: Timings,
        kind: LobbyKind,
    ) -> Self {
        let solo_after = (kind == LobbyKind::QuickPlay).then(|| ticks(timings.quick_solo));
        Self {
            id,
            settings,
            track,
            tuning,
            timings,
            tick: 0,
            phase: Phase::Waiting,
            members: Vec::new(),
            next_car: 0,
            changed: true,
            solo_after,
            solo_tick: solo_after,
            ranked: None,
            outcome: None,
        }
    }

    /// Makes this a ranked match between `entrants`, and lines them up on the grid.
    fn begin_match(&mut self, entrants: Vec<Entrant>) -> Vec<(CarId, mpsc::Receiver<Bytes>)> {
        let mut joined = Vec::with_capacity(entrants.len());
        let mut roster = Vec::with_capacity(entrants.len());
        for entrant in entrants {
            let (car, snapshots) = self
                .add_member(entrant.player.clone())
                .expect("a match has room for its players");
            roster.push(Rostered { entrant, car });
            joined.push((car, snapshots));
        }
        self.ranked = Some(RankedMatch {
            roster,
            forfeited: Vec::new(),
        });
        self.start_race();
        joined
    }

    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut tuning: watch::Receiver<CarTuning>,
        state: watch::Sender<Bytes>,
        events: mpsc::UnboundedSender<LobbyEvent>,
    ) {
        let mut ticks = time::interval(TICK_DURATION);
        // After a stall, catch up on the missed ticks so race time keeps pace with real time.
        ticks.set_missed_tick_behavior(MissedTickBehavior::Burst);
        loop {
            tokio::select! {
                _ = ticks.tick() => {
                    // An error means the server is shutting down: keep the current tuning.
                    if tuning.has_changed().unwrap_or(false) {
                        self.tuning = tuning.borrow_and_update().clone();
                        // The players predict their cars with it: they hear of it at once.
                        self.changed = true;
                    }
                    self.tick();
                }
                command = commands.recv() => {
                    // The lobby holds no handle to itself, but members always do until they leave.
                    let Some(command) = command else { break };
                    self.handle(command, &state);
                }
            }
            if let Some(placed) = self.outcome.take() {
                let _ = events.send(LobbyEvent::MatchFinished(placed));
            }
            // Everyone has left, or a ranked match has shown its results.
            if self.members.is_empty() {
                break;
            }
            if std::mem::take(&mut self.changed) {
                state.send_replace(self.encoded_state());
                let _ = events.send(LobbyEvent::Changed(self.info(Instant::now())));
            }
        }
        info!(lobby = %self.id, name = %self.settings.name, "lobby closed");
        let _ = events.send(LobbyEvent::Closed(self.id));
    }

    fn handle(&mut self, command: Command, state: &watch::Sender<Bytes>) {
        match command {
            Command::Join { player, reply } => {
                let joined = self.join(player).map(|(car, snapshots)| {
                    let mut state = state.subscribe();
                    // The value is published after this command, but the new member must get it
                    // even if it was already published.
                    state.mark_changed();
                    Joined {
                        car,
                        settings: self.settings.clone(),
                        snapshots,
                        state,
                    }
                });
                if let Err(Ok(joined)) = reply.send(joined) {
                    // The session stopped waiting for the answer and will never leave: remove the
                    // player now.
                    self.leave(joined.car);
                }
            }
            Command::Input {
                car,
                input,
                tick,
                seq,
            } => self.input(car, input, tick, seq),
            Command::Leave { car } => self.leave(car),
        }
    }

    fn join(&mut self, player: Player) -> Result<(CarId, mpsc::Receiver<Bytes>), JoinError> {
        // A ranked match is between the players the matchmaker put in it.
        if self.ranked.is_some() {
            return Err(JoinError::LobbyFull);
        }
        self.add_member(player)
    }

    fn add_member(&mut self, player: Player) -> Result<(CarId, mpsc::Receiver<Bytes>), JoinError> {
        if self.members.len() >= usize::from(self.settings.max_players) {
            return Err(JoinError::LobbyFull);
        }

        let car_id = self.allocate_car_id();
        let status = match &self.phase {
            Phase::Racing(race) if self.tick < race.start_tick => PlayerStatus::Racing,
            Phase::Racing(_) => PlayerStatus::Spectating,
            _ => PlayerStatus::Driving,
        };
        let slot = match status {
            PlayerStatus::Spectating => 0,
            _ => self.free_grid_slot(),
        };
        let car = Car::on_grid(&self.track.track, slot);
        let (snapshots, receiver) = mpsc::channel(SNAPSHOT_BUFFER);
        info!(
            lobby = %self.id, player = %player.id, nickname = %player.nickname, car = %car_id,
            ?status, "player joined the lobby"
        );
        self.members.push(Member {
            player,
            car_id,
            status,
            car,
            input: CarInput::default(),
            input_seq: 0,
            scheduled: VecDeque::new(),
            start: StartTiming::default(),
            progress: Progress::new(&self.track.track, car.position),
            snapshots,
        });
        self.changed = true;
        Ok((car_id, receiver))
    }

    /// A new input, the player's `seq`-th in this lobby, to take effect on `tick`: the tick the
    /// player's own client applied it on as it predicted their car. It waits for that tick here,
    /// so both sides step the car with the same inputs on the same ticks, and the start is graded
    /// on the tick the input took effect. See docs/latency.md.
    fn input(&mut self, car: CarId, input: CarInput, tick: u32, seq: u32) {
        let next_tick = self.tick + 1;
        let Some(member) = self.members.iter_mut().find(|member| member.car_id == car) else {
            return;
        };
        // A client sending more than this within half a second is broken or hostile; its input
        // stays what it was rather than growing the queue.
        if member.scheduled.len() >= MAX_SCHEDULED {
            return;
        }
        // Never before the next tick -- the past is not rewritten -- nor before an input sent
        // earlier, nor further ahead than a client may lead.
        let earliest = member
            .scheduled
            .back()
            .map_or(next_tick, |last| last.tick.max(next_tick));
        let latest = (self.tick + MAX_INPUT_LEAD).max(earliest);
        member.scheduled.push_back(Scheduled {
            tick: tick.clamp(earliest, latest),
            input,
            seq,
        });
    }

    fn leave(&mut self, car: CarId) {
        let Some(index) = self.members.iter().position(|member| member.car_id == car) else {
            return;
        };
        let member = self.members.remove(index);
        info!(
            lobby = %self.id, player = %member.player.id, nickname = %member.player.nickname,
            car = %car, "player left the lobby"
        );
        self.changed = true;

        // Leaving a ranked race comes last, however it was left: a lost connection counts too,
        // or disconnecting would be the way out of a race going badly.
        let in_play = matches!(self.phase, Phase::Racing(_))
            && !matches!(member.status, PlayerStatus::Finished { .. });
        if in_play && let Some(ranked) = &mut self.ranked {
            ranked.forfeited.push(member.player.id);
        }
        // With nobody left to finish it, the race is settled now or never.
        if self.members.is_empty() && self.ranked.is_some() {
            self.end_race();
        }
    }

    fn allocate_car_id(&mut self) -> CarId {
        loop {
            let id = CarId(self.next_car);
            self.next_car = self.next_car.wrapping_add(1);
            if self.members.iter().all(|member| member.car_id != id) {
                return id;
            }
        }
    }

    /// The first grid slot no car stands on, so a car never appears inside another one.
    fn free_grid_slot(&self) -> usize {
        (0..)
            .find(|slot| {
                let (position, _) = self.track.track.grid_slot(*slot);
                self.members
                    .iter()
                    .filter(|member| member.status != PlayerStatus::Spectating)
                    .all(|member| member.car.position.distance(position) > 1.0)
            })
            .expect("there are infinitely many slots")
    }

    /// Advances the lobby by one tick, and sends the snapshot to every member.
    fn tick(&mut self) {
        self.tick += 1;
        self.update_phase();
        self.apply_inputs();
        self.drive();
        self.send_snapshot();
    }

    /// Puts into effect every input whose tick has come, in the order they were sent. After the
    /// phase, which may reset a start, and before the cars move.
    fn apply_inputs(&mut self) {
        let tick = self.tick;
        for member in &mut self.members {
            while let Some(next) = member.scheduled.pop_front() {
                if next.tick > tick {
                    member.scheduled.push_front(next);
                    break;
                }
                if next.input.accelerate != member.input.accelerate {
                    member
                        .start
                        .accelerator(next.input.accelerate, f64::from(tick));
                }
                member.input = next.input;
                member.input_seq = next.seq;
            }
        }
    }

    fn update_phase(&mut self) {
        let enough_players = self.members.len() >= usize::from(self.settings.min_players);
        let waited_enough = self.solo_tick.is_some_and(|tick| self.tick >= tick);
        match &self.phase {
            Phase::Waiting if enough_players || waited_enough => self.start_race(),
            Phase::Racing(race) => {
                if self.tick == race.start_tick {
                    // The listing tells joining from spectating apart.
                    self.changed = true;
                }
                let racers = self
                    .members
                    .iter()
                    .filter(|member| member.status == PlayerStatus::Racing)
                    .count();
                let timed_out = race
                    .first_finish_tick
                    .is_some_and(|first| self.tick >= first + ticks(self.timings.finish_timeout));
                // Everyone else has walked out of a ranked race: the last one standing has won it,
                // and need not drive three laps alone to be told so.
                let walkover = self.ranked.is_some() && racers <= 1 && race.finishers.is_empty();
                if racers == 0 || timed_out || walkover {
                    self.end_race();
                }
            }
            // A ranked match is one race: once its results have shown, it is over.
            Phase::Results { end_tick, .. } if self.tick >= *end_tick && self.ranked.is_some() => {
                self.members.clear();
                self.changed = true;
            }
            Phase::Results { end_tick, .. } if self.tick >= *end_tick => {
                if enough_players {
                    self.start_race();
                } else {
                    self.hand_cars_back();
                    self.solo_tick = self.solo_after.map(|after| self.tick + after);
                    self.set_phase(Phase::Waiting);
                }
            }
            _ => {}
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.changed = true;
    }

    /// Lines every player up on the grid, in the order they joined.
    fn start_race(&mut self) {
        let track = &self.track.track;
        for (slot, member) in self.members.iter_mut().enumerate() {
            member.status = PlayerStatus::Racing;
            member.start.reset();
            member.car = Car::on_grid(track, slot);
            member.progress = Progress::new(track, member.car.position);
        }
        info!(
            lobby = %self.id, players = self.members.len(), laps = self.settings.laps,
            "race started"
        );
        let grid = if self.ranked.is_some() {
            self.timings.ranked_grid
        } else {
            self.timings.grid
        };
        self.set_phase(Phase::Racing(Race {
            start_tick: self.tick + ticks(grid),
            finishers: Vec::new(),
            first_finish_tick: None,
            leader_lap: 0,
        }));
    }

    fn end_race(&mut self) {
        let Phase::Racing(race) = std::mem::replace(&mut self.phase, Phase::Waiting) else {
            return;
        };

        let mut unfinished: Vec<&Member> = self
            .members
            .iter()
            .filter(|member| member.status == PlayerStatus::Racing)
            .collect();
        unfinished.sort_by(|a, b| {
            RaceStanding::Racing {
                progress: a.progress.total(),
            }
            .rank(&RaceStanding::Racing {
                progress: b.progress.total(),
            })
        });
        let mut results = race.finishers;
        results.sort_by_key(|result| result.time_ms);
        results.extend(unfinished.into_iter().map(|member| RaceResult {
            car: member.car_id,
            nickname: member.player.nickname.clone(),
            time_ms: None,
        }));
        info!(lobby = %self.id, ?results, "race finished");
        let ratings = self.settle_ratings(&results);

        // Everyone who did not cross the line drives freely again; those who did keep watching
        // their car drive itself, through the results and until the next race lines up. A solo
        // race ends on the tick its only player finishes, so this is the whole of their finish.
        // Spectators get a car, once the racers are counted as standing where they are.
        for member in &mut self.members {
            if member.status == PlayerStatus::Racing {
                member.status = PlayerStatus::Driving;
            }
        }
        for index in 0..self.members.len() {
            if self.members[index].status == PlayerStatus::Spectating {
                let slot = self.free_grid_slot();
                let member = &mut self.members[index];
                member.status = PlayerStatus::Driving;
                member.car = Car::on_grid(&self.track.track, slot);
            }
        }

        self.set_phase(Phase::Results {
            results,
            end_tick: self.tick + ticks(self.timings.results),
            ratings,
        });
    }

    /// In a ranked match, works out what the race did to every rating, for the players to see and
    /// the directory to record: those listed in `results` in that order, then whoever left, the
    /// last to leave ahead of the first.
    fn settle_ratings(&mut self, results: &[RaceResult]) -> Vec<RatingChange> {
        let Some(ranked) = &self.ranked else {
            return Vec::new();
        };
        let by_car = |car: CarId| ranked.roster.iter().find(|rostered| rostered.car == car);
        let by_player = |id: PlayerId| {
            ranked
                .roster
                .iter()
                .find(|rostered| rostered.entrant.player.id == id)
        };
        let order: Vec<&Rostered> = results
            .iter()
            .filter_map(|result| by_car(result.car))
            .chain(
                ranked
                    .forfeited
                    .iter()
                    .rev()
                    .filter_map(|id| by_player(*id)),
            )
            .collect();
        let standings: Vec<(i32, u32)> = order
            .iter()
            .map(|rostered| (rostered.entrant.rating, rostered.entrant.games))
            .collect();
        let changes = rating_changes(&standings);

        self.outcome = Some(
            order
                .iter()
                .zip(&changes)
                .enumerate()
                .map(|(place, (rostered, change))| Placed {
                    id: rostered.entrant.player.id,
                    nickname: rostered.entrant.player.nickname.clone(),
                    change: *change,
                    won: place == 0,
                })
                .collect(),
        );
        order
            .iter()
            .zip(&changes)
            .map(|(rostered, change)| RatingChange {
                car: rostered.car,
                before: rostered.entrant.rating,
                after: (rostered.entrant.rating + change).max(0),
            })
            .collect()
    }

    /// Gives the players who finished the last race their car back, once there is nothing left
    /// to watch.
    fn hand_cars_back(&mut self) {
        let mut handed = false;
        for member in &mut self.members {
            if matches!(member.status, PlayerStatus::Finished { .. }) {
                member.status = PlayerStatus::Driving;
                handed = true;
            }
        }
        self.changed |= handed;
    }

    /// Moves every car that may move this tick, and records laps and finishes.
    fn drive(&mut self) {
        let track = &self.track.track;
        let Phase::Racing(race) = &mut self.phase else {
            for member in &mut self.members {
                match member.status {
                    PlayerStatus::Driving => member.car.step(member.input, &self.tuning, track),
                    // The race is over, and they crossed the line: their car carries on by
                    // itself while the results show.
                    PlayerStatus::Finished { .. } => {
                        let input = self_driven(&member.car, track);
                        member.car.step(input, &self.tuning, track);
                    }
                    PlayerStatus::Racing | PlayerStatus::Spectating => {}
                }
            }
            return;
        };
        let finish = f32::from(self.settings.laps) * track.length();
        for member in &mut self.members {
            let before = member.progress.total();
            match member.status {
                // The rules both sides share, so the player's own client predicts exactly this:
                // graded on the first tick the start can be, held on the grid until then, driven
                // after it.
                PlayerStatus::Racing => {
                    if let Some(grade) = race::racing_tick(
                        &mut member.car,
                        &mut member.start,
                        member.input,
                        self.tick,
                        race.start_tick,
                        &self.tuning,
                        track,
                    ) {
                        self.changed = true;
                        debug!(
                            lobby = %self.id, nickname = %member.player.nickname, ?grade,
                            "start graded"
                        );
                    }
                }
                // A player who has finished watches their car drive itself home while the others
                // come in, the way a kart carries on over the line: their last input never leaves
                // a car parked across the road.
                PlayerStatus::Finished { .. } => {
                    let input = self_driven(&member.car, track);
                    member.car.step(input, &self.tuning, track);
                }
                PlayerStatus::Driving | PlayerStatus::Spectating => continue,
            }
            // On the grid, waiting for the start: nothing has moved.
            if self.tick <= race.start_tick {
                continue;
            }
            member.progress.update(track, member.car.position);
            let after = member.progress.total();

            if member.status == PlayerStatus::Racing && after >= finish {
                // Where the line was crossed within the tick, so ties within a tick are settled.
                let within_tick = ((finish - before) / (after - before)).clamp(0.0, 1.0);
                let racing_ticks = (self.tick - race.start_tick - 1) as f32 + within_tick;
                let time_ms = (racing_ticks * TICK_SECONDS * 1000.0).round() as u32;
                member.status = PlayerStatus::Finished { time_ms };
                race.finishers.push(RaceResult {
                    car: member.car_id,
                    nickname: member.player.nickname.clone(),
                    time_ms: Some(time_ms),
                });
                race.first_finish_tick.get_or_insert(self.tick);
                self.changed = true;
                info!(
                    lobby = %self.id, nickname = %member.player.nickname, time_ms,
                    "player finished"
                );
            }
        }

        let leader_lap = self
            .members
            .iter()
            .filter(|member| member.status != PlayerStatus::Spectating)
            .map(|member| race::lap(member.progress.total(), track.length()))
            .max()
            .unwrap_or(0)
            .min(u32::from(self.settings.laps)) as u8;
        if leader_lap != race.leader_lap {
            race.leader_lap = leader_lap;
            self.changed = true;
        }
    }

    fn send_snapshot(&self) {
        let racing = matches!(self.phase, Phase::Racing(_));
        let snapshot = ServerMessage::Snapshot(Snapshot {
            tick: self.tick,
            cars: self
                .members
                .iter()
                .filter(|member| member.status != PlayerStatus::Spectating)
                .map(|member| {
                    let progress = if racing { member.progress.total() } else { 0.0 };
                    CarSnapshot::new(
                        member.car_id,
                        member.car,
                        progress,
                        member.input_seq,
                        &self.tuning,
                    )
                })
                .collect(),
        });
        if self.tick % TICK_RATE == 0 {
            for member in &self.members {
                debug!(
                    lobby = %self.id,
                    car = %member.car_id,
                    status = ?member.status,
                    progress = member.progress.total(),
                    speed = member.car.velocity.length(),
                    heading = member.car.heading,
                    slip = member.car.slip,
                    body = member.car.body,
                    drift = member.car.drift,
                    drift_charge = member.car.drift_charge,
                    boost = member.car.boost,
                    input = ?member.input,
                    "car state"
                );
            }
        }

        let bytes = Bytes::from(protocol::encode(&snapshot));
        for member in &self.members {
            // Full: this player is lagging and skips a snapshot. Closed: their session is ending
            // and will send `Leave`.
            let _ = member.snapshots.try_send(bytes.clone());
        }
    }

    fn state(&self) -> LobbyState {
        LobbyState {
            phase: match &self.phase {
                Phase::Waiting => LobbyPhase::Waiting {
                    solo_tick: self.solo_tick,
                },
                Phase::Racing(race) => LobbyPhase::Racing {
                    start_tick: race.start_tick,
                },
                Phase::Results {
                    results,
                    end_tick,
                    ratings,
                } => LobbyPhase::Results {
                    results: results.clone(),
                    end_tick: *end_tick,
                    ratings: ratings.clone(),
                },
            },
            ranked: self.ranked.is_some(),
            players: self
                .members
                .iter()
                .map(|member| LobbyPlayer {
                    car: member.car_id,
                    nickname: member.player.nickname.clone(),
                    status: member.status,
                    start: member.start.grade(),
                    rating: self.ranked.as_ref().and_then(|ranked| {
                        ranked
                            .roster
                            .iter()
                            .find(|rostered| rostered.car == member.car_id)
                            .map(|rostered| rostered.entrant.rating)
                    }),
                })
                .collect(),
            tuning: self.tuning.clone(),
        }
    }

    fn encoded_state(&self) -> Bytes {
        Bytes::from(protocol::encode(&ServerMessage::Lobby(self.state())))
    }

    fn info(&self, now: Instant) -> LobbyInfo {
        let deadline = |tick: u32| now + TICK_DURATION * tick.saturating_sub(self.tick);
        LobbyInfo {
            id: self.id,
            settings: self.settings.clone(),
            players: self
                .members
                .iter()
                .map(|member| member.player.nickname.clone())
                .collect(),
            status: match &self.phase {
                Phase::Waiting => InfoStatus::Waiting,
                Phase::Racing(race) if self.tick < race.start_tick => InfoStatus::Starting {
                    at: deadline(race.start_tick),
                },
                Phase::Racing(race) => InfoStatus::Racing {
                    lap: race.leader_lap,
                },
                Phase::Results { end_tick, .. } => InfoStatus::Results {
                    until: deadline(*end_tick),
                },
            },
            ranked: self.ranked.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use space_race_sim::race::StartGrade;

    use super::*;
    use crate::content;
    use crate::session::PlayerId;

    fn skyway() -> (Arc<LoadedTrack>, CarTuning) {
        let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let tracks = content::load_tracks(&data_dir).unwrap();
        let tuning =
            content::load_car_tuning(&data_dir.join(content::CAR_TUNING_FILE), &tracks).unwrap();
        (Arc::clone(&tracks["skyway"]), tuning)
    }

    fn settings(laps: u8, min_players: u8, max_players: u8) -> LobbySettings {
        LobbySettings {
            name: "Test".into(),
            track: "skyway".into(),
            laps,
            min_players,
            max_players,
        }
    }

    fn quick_timings() -> Timings {
        Timings {
            grid: Duration::from_millis(500),
            finish_timeout: Duration::from_secs(2),
            results: Duration::from_millis(500),
            quick_solo: Duration::from_secs(1),
            ranked_grid: Duration::from_millis(500),
            gather: Duration::from_millis(100),
        }
    }

    fn new_lobby(settings: LobbySettings, timings: Timings) -> Lobby {
        let (track, tuning) = skyway();
        Lobby::new(
            LobbyId(0),
            settings,
            track,
            tuning,
            timings,
            LobbyKind::Custom,
        )
    }

    fn player(number: u8) -> Player {
        Player {
            id: PlayerId([number; 32]),
            nickname: format!("Player {number}"),
        }
    }

    fn join(lobby: &mut Lobby, number: u8) -> CarId {
        // The snapshot receiver is dropped: the lobby ignores closed channels.
        lobby.join(player(number)).unwrap().0
    }

    impl Member {
        /// The rating the lobby shows for this player.
        fn rating_shown(&self, lobby: &Lobby) -> Option<i32> {
            lobby
                .state()
                .players
                .iter()
                .find(|player| player.car == self.car_id)
                .and_then(|player| player.rating)
        }
    }

    fn member(lobby: &Lobby, car: CarId) -> &Member {
        lobby
            .members
            .iter()
            .find(|member| member.car_id == car)
            .unwrap()
    }

    /// Lets the autopilot drive every car, from the live state.
    fn autopilot_inputs(lobby: &mut Lobby) {
        let track = Arc::clone(&lobby.track);
        for member in &mut lobby.members {
            member.input = autopilot::drive(&member.car, &track.track);
        }
    }

    /// Ticks until `done` holds, and fails after `seconds` of lobby time.
    fn tick_until(lobby: &mut Lobby, seconds: f32, mut done: impl FnMut(&mut Lobby) -> bool) {
        for _ in 0..(seconds * TICK_RATE as f32) as u32 {
            lobby.tick();
            if done(lobby) {
                return;
            }
        }
        panic!("condition not reached within {seconds} s");
    }

    #[test]
    fn cars_line_up_as_soon_as_there_are_enough_players() {
        let mut lobby = new_lobby(settings(3, 2, 8), Timings::default());
        let first = join(&mut lobby, 1);
        lobby.tick();
        assert!(matches!(lobby.state().phase, LobbyPhase::Waiting { .. }));

        let second = join(&mut lobby, 2);
        lobby.tick();
        let LobbyPhase::Racing { start_tick } = lobby.state().phase else {
            panic!("expected the grid, got {:?}", lobby.state().phase);
        };
        assert_eq!(start_tick, lobby.tick + 6 * TICK_RATE);
        for car in [first, second] {
            assert_eq!(member(&lobby, car).status, PlayerStatus::Racing);
        }
    }

    #[test]
    fn cars_stand_on_the_grid_until_the_start() {
        let mut lobby = new_lobby(settings(3, 2, 8), quick_timings());
        let car = join(&mut lobby, 1);
        lobby.input(car, CarInput::new(true, 0.0), 0, 0);

        // Driving freely while waiting for players.
        let spawn = member(&lobby, car).car.position;
        lobby.tick();
        lobby.tick();
        assert!(member(&lobby, car).car.velocity.length() > 0.0);

        join(&mut lobby, 2);
        lobby.tick();
        let LobbyPhase::Racing { start_tick } = lobby.state().phase else {
            panic!("expected the grid, got {:?}", lobby.state().phase);
        };
        assert_eq!(member(&lobby, car).status, PlayerStatus::Racing);
        assert_eq!(member(&lobby, car).car.position, spawn);

        // Held on the grid, accelerator down, until the start.
        while lobby.tick < start_tick {
            lobby.tick();
            assert_eq!(member(&lobby, car).car.position, spawn);
        }
        lobby.tick();
        assert_ne!(member(&lobby, car).car.position, spawn);
    }

    /// An input takes effect on the tick its client placed it on, and the start is graded on that
    /// tick: a press placed on the start is perfect however early it arrived, one that arrives
    /// after its tick counts from the tick it could take effect on, and none can be placed
    /// further ahead than a client may lead.
    #[test]
    fn inputs_take_effect_on_their_tick_and_starts_are_graded_there() {
        let timings = Timings {
            grid: Duration::from_secs(2),
            ..quick_timings()
        };
        let mut lobby = new_lobby(settings(3, 4, 8), timings);
        let early = join(&mut lobby, 1);
        let perfect = join(&mut lobby, 2);
        let late = join(&mut lobby, 3);
        let ahead = join(&mut lobby, 4);
        tick_until(&mut lobby, 1.0, |lobby| {
            matches!(lobby.phase, Phase::Racing(_))
        });
        let LobbyPhase::Racing { start_tick } = lobby.state().phase else {
            unreachable!();
        };
        let accelerate = CarInput::new(true, 0.0);
        let grade = |lobby: &Lobby, car: CarId| member(lobby, car).start.grade();
        let tick_to = |lobby: &mut Lobby, tick: u32| {
            while lobby.tick < tick {
                lobby.tick();
            }
        };

        // Pressed as the cars lined up, and held.
        lobby.input(early, accelerate, lobby.tick + 1, 1);
        // Placed on the start a whole second before it, as no client leading by a round trip
        // would: it takes effect as far ahead as a client may lead, and no further.
        tick_to(&mut lobby, start_tick - 60);
        lobby.input(ahead, accelerate, start_tick, 1);
        // Placed on the start by a client leading the server by five ticks: it waits for it.
        tick_to(&mut lobby, start_tick - 5);
        lobby.input(perfect, accelerate, start_tick, 1);
        lobby.tick();
        assert!(!member(&lobby, perfect).input.accelerate);
        // Placed on the start, but arriving ten ticks after it: from the next tick.
        tick_to(&mut lobby, start_tick + 10);
        lobby.input(late, accelerate, start_tick, 1);
        tick_to(&mut lobby, start_tick + 30);

        assert_eq!(grade(&lobby, early), Some(StartGrade::E));
        assert_eq!(grade(&lobby, ahead), Some(StartGrade::E));
        assert_eq!(grade(&lobby, perfect), Some(StartGrade::S));
        assert_eq!(grade(&lobby, late), Some(StartGrade::C));
        assert_eq!(member(&lobby, early).car.boost, 0.0);
        assert!(member(&lobby, late).car.boost > 0.0);
        assert!(member(&lobby, perfect).car.boost > member(&lobby, late).car.boost);
        let players = lobby.state().players;
        assert!(
            players
                .iter()
                .any(|player| player.car == perfect && player.start == Some(StartGrade::S))
        );
    }

    #[test]
    fn a_race_ends_with_results_and_the_next_one_begins() {
        let mut lobby = new_lobby(settings(1, 1, 8), quick_timings());
        let car = join(&mut lobby, 1);

        tick_until(&mut lobby, 120.0, |lobby| {
            autopilot_inputs(lobby);
            matches!(lobby.phase, Phase::Results { .. })
        });
        let LobbyPhase::Results { results, .. } = lobby.state().phase else {
            unreachable!();
        };
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].car, car);
        // A sanity check rather than a benchmark: one lap of the skyway from the grid is about
        // a minute of driving, and what matters here is that the lap was timed at all. What the
        // autopilot is really worth on a circuit is measured in `content::tests`.
        let time_ms = results[0].time_ms.expect("the race was finished");
        assert!((30_000..100_000).contains(&time_ms), "{time_ms} ms");

        // Alone, the race ends on the tick its only player crosses the line, so the results are
        // the whole of their finish: the car keeps driving itself through them, hands off.
        assert!(matches!(
            member(&lobby, car).status,
            PlayerStatus::Finished { .. }
        ));
        let crossed = member(&lobby, car).car.position;
        for _ in 0..TICK_RATE / 4 {
            lobby.input(car, CarInput::default(), 0, 0);
            lobby.tick();
        }
        let watching = member(&lobby, car);
        assert!(watching.car.velocity.length() > 10.0, "{:?}", watching.car);
        assert!(watching.car.position.distance(crossed) > 5.0);

        tick_until(&mut lobby, 1.0, |lobby| {
            matches!(lobby.phase, Phase::Racing(_))
        });
        assert_eq!(member(&lobby, car).status, PlayerStatus::Racing);
    }

    /// Once the results are over and there are not enough players for another race, whoever
    /// finished takes their car back.
    #[test]
    fn a_finished_player_drives_again_when_the_lobby_waits() {
        let mut lobby = new_lobby(settings(1, 2, 8), quick_timings());
        let first = join(&mut lobby, 1);
        let second = join(&mut lobby, 2);

        tick_until(&mut lobby, 120.0, |lobby| {
            autopilot_inputs(lobby);
            matches!(lobby.phase, Phase::Results { .. })
        });
        assert!(matches!(
            member(&lobby, first).status,
            PlayerStatus::Finished { .. }
        ));

        lobby.leave(second);
        tick_until(&mut lobby, 2.0, |lobby| {
            matches!(lobby.phase, Phase::Waiting)
        });
        assert_eq!(member(&lobby, first).status, PlayerStatus::Driving);
    }

    /// A player who has finished stops driving: their car carries on by itself, so they have
    /// something to watch while the others come in, and never sits parked across the road.
    #[test]
    fn a_finished_car_drives_itself_while_the_others_come_in() {
        let mut lobby = new_lobby(settings(1, 2, 8), quick_timings());
        let winner = join(&mut lobby, 1);
        let straggler = join(&mut lobby, 2);

        // The winner laps; the straggler never moves, so the race is still on at the finish.
        tick_until(&mut lobby, 120.0, |lobby| {
            autopilot_inputs(lobby);
            lobby.input(straggler, CarInput::default(), 0, 0);
            matches!(member(lobby, winner).status, PlayerStatus::Finished { .. })
        });

        // Both players now let go of everything. Only the one still racing comes to a stop.
        let crossed = member(&lobby, winner).progress.total();
        for _ in 0..TICK_RATE {
            lobby.input(winner, CarInput::default(), 0, 0);
            lobby.input(straggler, CarInput::default(), 0, 0);
            lobby.tick();
        }

        let carrying_on = member(&lobby, winner);
        assert!(
            carrying_on.car.velocity.length() > 10.0,
            "{:?}",
            carrying_on.car.velocity
        );
        assert!(carrying_on.progress.total() > crossed + 10.0);
        assert!(member(&lobby, straggler).car.velocity.length() < 1.0);
    }

    #[test]
    fn stragglers_do_not_finish_after_the_timeout() {
        let mut lobby = new_lobby(settings(1, 2, 8), quick_timings());
        let winner = join(&mut lobby, 1);
        let straggler = join(&mut lobby, 2);

        tick_until(&mut lobby, 120.0, |lobby| {
            autopilot_inputs(lobby);
            // The straggler never moves.
            lobby.input(straggler, CarInput::default(), 0, 0);
            matches!(lobby.phase, Phase::Results { .. })
        });
        let LobbyPhase::Results { results, .. } = lobby.state().phase else {
            unreachable!();
        };
        assert_eq!(results.len(), 2);
        assert_eq!((results[0].car, results[1].car), (winner, straggler));
        assert!(results[0].time_ms.is_some());
        assert_eq!(results[1].time_ms, None);
    }

    #[test]
    fn players_joining_on_the_grid_race() {
        let mut lobby = new_lobby(settings(3, 2, 8), quick_timings());
        let first = join(&mut lobby, 1);
        join(&mut lobby, 2);
        lobby.tick();
        // The first player leaves a free slot among the others.
        lobby.leave(first);
        lobby.tick();

        let late = join(&mut lobby, 3);
        assert_eq!(member(&lobby, late).status, PlayerStatus::Racing);
        let positions: Vec<_> = lobby
            .members
            .iter()
            .map(|member| member.car.position)
            .collect();
        assert!(positions[0].distance(positions[1]) > 1.0);
    }

    #[test]
    fn players_joining_during_a_race_spectate_it_and_drive_after() {
        let mut lobby = new_lobby(settings(3, 1, 8), quick_timings());
        let racer = join(&mut lobby, 1);
        tick_until(
            &mut lobby,
            1.0,
            |lobby| matches!(&lobby.phase, Phase::Racing(race) if lobby.tick >= race.start_tick),
        );

        let (spectator, mut snapshots) = lobby.join(player(2)).unwrap();
        assert_eq!(member(&lobby, spectator).status, PlayerStatus::Spectating);
        lobby.tick();
        let ServerMessage::Snapshot(snapshot) =
            protocol::decode::<ServerMessage>(&snapshots.try_recv().unwrap()).unwrap()
        else {
            panic!("expected a snapshot");
        };
        let cars: Vec<_> = snapshot.cars.iter().map(|car| car.id).collect();
        assert_eq!(cars, [racer]);

        // Without anyone racing, the race ends at once, and the spectator gets a car.
        lobby.leave(racer);
        lobby.tick();
        assert!(matches!(lobby.phase, Phase::Results { .. }));
        assert_eq!(member(&lobby, spectator).status, PlayerStatus::Driving);
    }

    fn entrant(number: u8, rating: i32) -> Entrant {
        Entrant {
            player: player(number),
            rating,
            games: 10,
        }
    }

    /// A ranked match of one lap, between players numbered from 1, all rated the same.
    fn ranked_match(players: u8) -> (Lobby, Vec<CarId>) {
        let mut lobby = new_lobby(settings(1, players, players), quick_timings());
        let entrants = (1..=players).map(|number| entrant(number, 1000)).collect();
        let cars = lobby
            .begin_match(entrants)
            .into_iter()
            .map(|(car, _)| car)
            .collect();
        (lobby, cars)
    }

    fn rating_of(lobby: &Lobby, car: CarId) -> (i32, i32) {
        let LobbyPhase::Results { ratings, .. } = lobby.state().phase else {
            panic!("expected the results");
        };
        let change = ratings
            .iter()
            .find(|change| change.car == car)
            .expect("a rating change for this car");
        (change.before, change.after)
    }

    #[test]
    fn nobody_joins_a_ranked_match_and_a_custom_lobby_is_not_one() {
        let (mut lobby, _) = ranked_match(2);
        assert!(lobby.state().ranked);
        assert!(matches!(lobby.join(player(9)), Err(JoinError::LobbyFull)));

        let custom = new_lobby(settings(3, 2, 8), quick_timings());
        assert!(!custom.state().ranked);
    }

    /// The race runs on the grid at once, the winner gains what the loser gives up, the directory
    /// is told, and the match ends once the results have shown.
    #[test]
    fn a_ranked_race_moves_the_ratings_and_ends_the_match() {
        let (mut lobby, cars) = ranked_match(2);
        assert_eq!(lobby.members[0].rating_shown(&lobby), Some(1000));
        tick_until(&mut lobby, 150.0, |lobby| {
            autopilot_inputs(lobby);
            matches!(lobby.phase, Phase::Results { .. })
        });

        let placed = lobby.outcome.take().expect("the directory is told");
        assert_eq!(placed.len(), 2);
        assert!(placed[0].won && !placed[1].won);
        assert_eq!(placed[0].change, 12);
        assert_eq!(placed[1].change, -12);
        let winner = cars
            .iter()
            .copied()
            .find(|car| rating_of(&lobby, *car).1 > 1000)
            .expect("someone won");
        assert_eq!(rating_of(&lobby, winner), (1000, 1012));

        tick_until(&mut lobby, 2.0, |lobby| lobby.members.is_empty());
    }

    /// Walking out of a race comes last, after those still in it, and the last to leave ranks
    /// ahead of the first.
    #[test]
    fn leaving_a_ranked_race_comes_last() {
        let (mut lobby, cars) = ranked_match(4);
        lobby.leave(cars[0]);
        lobby.leave(cars[1]);
        tick_until(&mut lobby, 150.0, |lobby| {
            autopilot_inputs(lobby);
            matches!(lobby.phase, Phase::Results { .. })
        });
        let placed = lobby.outcome.take().unwrap();
        assert_eq!(placed.len(), 4);
        assert_eq!(placed[2].id, player(2).id);
        assert_eq!(placed[3].id, player(1).id);
        assert!(placed[3].change < placed[2].change);
        assert!(placed[2].change < 0);
        // Those who left are not in the results the others read.
        let LobbyPhase::Results { results, .. } = lobby.state().phase else {
            panic!("expected the results");
        };
        assert_eq!(results.len(), 2);
    }

    /// With only one player left the race is theirs, without three laps alone to prove it.
    #[test]
    fn the_last_player_standing_wins_a_ranked_race_at_once() {
        let (mut lobby, cars) = ranked_match(2);
        lobby.leave(cars[0]);
        lobby.tick();
        assert!(matches!(lobby.phase, Phase::Results { .. }));
        let placed = lobby.outcome.take().unwrap();
        assert_eq!(placed[0].id, player(2).id);
        assert!(placed[0].won);
        assert_eq!(placed[1].id, player(1).id);
    }

    /// Everyone walking out still counts: the rating is settled, and the lobby goes.
    #[test]
    fn a_ranked_race_everyone_left_is_still_settled() {
        let (mut lobby, cars) = ranked_match(2);
        lobby.leave(cars[0]);
        lobby.leave(cars[1]);
        assert!(lobby.members.is_empty());
        let placed = lobby.outcome.take().expect("settled");
        assert_eq!(placed[0].id, player(2).id);
    }

    /// Having crossed the line, leaving costs nothing.
    #[test]
    fn leaving_after_finishing_keeps_the_place() {
        let (mut lobby, cars) = ranked_match(2);
        tick_until(&mut lobby, 150.0, |lobby| {
            autopilot_inputs(lobby);
            lobby
                .members
                .iter()
                .any(|member| matches!(member.status, PlayerStatus::Finished { .. }))
        });
        let finisher = lobby
            .members
            .iter()
            .find(|member| matches!(member.status, PlayerStatus::Finished { .. }))
            .map(|member| (member.car_id, member.player.id))
            .unwrap();
        lobby.leave(finisher.0);
        tick_until(&mut lobby, 150.0, |lobby| {
            autopilot_inputs(lobby);
            matches!(lobby.phase, Phase::Results { .. })
        });
        let placed = lobby.outcome.take().unwrap();
        assert_eq!(placed[0].id, finisher.1);
        assert!(placed[0].won);
        let _ = cars;
    }

    fn quick_lobby() -> Lobby {
        let (track, tuning) = skyway();
        Lobby::new(
            LobbyId(0),
            settings(1, 2, 8),
            track,
            tuning,
            quick_timings(),
            LobbyKind::QuickPlay,
        )
    }

    /// A lobby opened by quick play gives a second player time to arrive, then races anyway.
    #[test]
    fn a_quick_play_lobby_does_not_wait_forever() {
        let mut lobby = quick_lobby();
        join(&mut lobby, 1);
        let solo_tick = ticks(quick_timings().quick_solo);
        assert_eq!(
            lobby.state().phase,
            LobbyPhase::Waiting {
                solo_tick: Some(solo_tick)
            }
        );
        for _ in 0..solo_tick - 1 {
            lobby.tick();
            assert!(matches!(lobby.phase, Phase::Waiting));
        }
        lobby.tick();
        assert!(matches!(lobby.phase, Phase::Racing(_)));
    }

    #[test]
    fn a_second_player_starts_a_quick_play_lobby_at_once() {
        let mut lobby = quick_lobby();
        join(&mut lobby, 1);
        lobby.tick();
        assert!(matches!(lobby.phase, Phase::Waiting));
        join(&mut lobby, 2);
        lobby.tick();
        assert!(matches!(lobby.phase, Phase::Racing(_)));
    }

    /// Alone after a race, the player gets the same wait again rather than waiting forever.
    #[test]
    fn a_quick_play_lobby_waits_again_after_a_race() {
        let mut lobby = quick_lobby();
        join(&mut lobby, 1);
        tick_until(&mut lobby, 150.0, |lobby| {
            autopilot_inputs(lobby);
            matches!(lobby.phase, Phase::Results { .. })
        });
        tick_until(&mut lobby, 2.0, |lobby| {
            matches!(lobby.phase, Phase::Waiting)
        });
        tick_until(&mut lobby, 2.0, |lobby| {
            matches!(lobby.phase, Phase::Racing(_))
        });
    }

    #[test]
    fn a_custom_lobby_waits_for_its_players() {
        let mut lobby = new_lobby(settings(1, 2, 8), quick_timings());
        join(&mut lobby, 1);
        assert_eq!(lobby.state().phase, LobbyPhase::Waiting { solo_tick: None });
        for _ in 0..600 {
            lobby.tick();
        }
        assert!(matches!(lobby.phase, Phase::Waiting));
    }

    #[test]
    fn a_full_lobby_refuses_players() {
        let mut lobby = new_lobby(settings(3, 2, 2), Timings::default());
        join(&mut lobby, 1);
        join(&mut lobby, 2);
        assert!(matches!(lobby.join(player(3)), Err(JoinError::LobbyFull)));
    }

    #[test]
    fn listed_countdowns_are_deadlines() {
        let mut lobby = new_lobby(settings(3, 1, 8), Timings::default());
        join(&mut lobby, 1);
        lobby.tick();
        let now = Instant::now();
        let info = lobby.info(now);
        let later = info.summary(now + Duration::from_secs(4));
        let LobbyStatus::Starting { remaining_ms } = later.status else {
            panic!("expected the grid, got {:?}", later.status);
        };
        assert!((1_900..=2_000).contains(&remaining_ms), "{remaining_ms}");
    }
}
