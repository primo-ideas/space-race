//! The player's own car, predicted: driven on the client the moment the player acts, instead of a
//! round trip and the interpolation delay later, when the server's snapshot of it is displayed.
//!
//! The client keeps a timeline of its own, ahead of the server by a round trip and a margin, so
//! that an input it applies on tick `n` reaches the server before the server steps tick `n`. Each
//! input goes out stamped with that tick, and the server holds it until then: both sides step the
//! car with the same inputs on the same ticks, through the same code (`Car::step` and
//! `race::racing_tick`), so they agree. When a snapshot of the car arrives, its state goes in
//! place of the prediction for that tick and the ticks since are replayed: reconciliation. While
//! every input arrives in time there is nothing to correct; when one does not, the difference is
//! eased out over a few frames rather than shown as a jump, unless it is a teleport.
//!
//! Only the player's own car is predicted, and only while they drive it: not once they have
//! crossed the line and the server drives it, nor while they watch. The other cars stay in the
//! interpolated past, which is what makes them move smoothly; they are shown a little behind where
//! they are, as every online racing game shows its opponents. See docs/latency.md.

use std::collections::VecDeque;
use std::f32::consts::PI;

use bevy::prelude::*;
use space_race_protocol::{LobbyPhase, MAX_INPUT_LEAD, PlayerStatus};
use space_race_sim::TICK_SECONDS;
use space_race_sim::car::{Car, CarInput, CarTuning};
use space_race_sim::race::{self, StartGrade, StartTiming};
use space_race_sim::track::Track;

use crate::latency::Latency;
use crate::lobby::{CurrentLobby, Membership};
use crate::net::NetEvent;
use crate::race::{SnapshotBuffer, interpolate, seconds_since_startup, shortest_angle};
use crate::track::CurrentTrack;

/// Kept on top of the round trip, in ticks, so an input still arrives before its tick when the
/// connection hiccups.
const MARGIN_TICKS: f64 = 2.0;
/// The round trip assumed before the first ping comes back: generous, since a lead too short
/// means inputs that arrive late and corrections.
const DEFAULT_ROUND_TRIP_MS: f64 = 100.0;
/// How quickly the lead follows the measured round trip, per second. Slowly: a lead that changes
/// is a timeline that stretches, and that is felt.
const LEAD_FOLLOW: f64 = 0.5;
/// Predicted ticks kept to put a snapshot's state in place of: two seconds, longer than any round
/// trip the lead allows.
const HISTORY: usize = 120;
/// Past this distance a correction is a teleport -- a new race lining the cars up, a car put back
/// on the road -- and is shown as one.
const SNAP_DISTANCE: f32 = 6.0;
/// How long a correction takes to fade out, in seconds: its time constant.
const CORRECTION_FADE: f32 = 0.08;

/// What the rules do to the player's car, as far as the client knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Driving freely: waiting for players, or between races.
    Free,
    /// In the race that starts on `start_tick`.
    Racing { start_tick: u32 },
}

impl Mode {
    /// The rules for the player's car, or `None` when it is not theirs to drive: once they have
    /// crossed the line, while they spectate, or before the lobby has said.
    fn of(membership: &Membership) -> Option<Self> {
        let state = membership.state.as_ref()?;
        match (membership.own_status()?, &state.phase) {
            (PlayerStatus::Driving, _) => Some(Self::Free),
            (PlayerStatus::Racing, LobbyPhase::Racing { start_tick }) => Some(Self::Racing {
                start_tick: *start_tick,
            }),
            _ => None,
        }
    }
}

/// The car as predicted at the end of one tick, with what the rules remember of it.
#[derive(Debug, Clone, Copy)]
struct Predicted {
    tick: u32,
    car: Car,
    start: StartTiming,
    /// The race `start` was last reset for, by its start tick: a new race grades a new start.
    race: Option<u32>,
    /// The input in effect on this tick.
    input: CarInput,
}

/// The prediction itself, apart from Bevy, so it can be tested tick by tick.
#[derive(Default)]
pub struct Predictor {
    /// Consecutive ticks, the newest last.
    ticks: VecDeque<Predicted>,
    /// Inputs by the tick they take effect on, in the order they were placed, until the oldest
    /// tick kept is past them.
    changes: VecDeque<(u32, CarInput)>,
    /// The input in effect before the oldest change kept.
    settled: CarInput,
    /// The tick the last input was placed on: none is placed before it.
    last_placed: u32,
}

impl Predictor {
    /// Places `input` on tick `tick`, or on the last input's tick if that is later, and returns the
    /// tick it went on: the one the server is asked to apply it on.
    pub fn place(&mut self, input: CarInput, tick: u32) -> u32 {
        let tick = tick.max(self.last_placed);
        self.last_placed = tick;
        self.changes.push_back((tick, input));
        tick
    }

    /// The newest tick predicted.
    pub fn newest(&self) -> Option<u32> {
        self.ticks.back().map(|predicted| predicted.tick)
    }

    /// Predicts every tick up to `tick`.
    pub fn advance(&mut self, tick: u32, mode: Mode, tuning: &CarTuning, track: &Track) {
        while let Some(&last) = self.ticks.back()
            && last.tick < tick
        {
            let next = self.step(last, mode, tuning, track);
            self.ticks.push_back(next);
            if self.ticks.len() > HISTORY {
                self.ticks.pop_front();
            }
        }
        self.forget_settled_changes();
    }

    /// The server's state of the car on `tick`, put in place of the prediction for that tick, and
    /// the ticks since replayed on top of it. Returns how far off the prediction was, in meters,
    /// if it had that tick. A state newer than anything predicted starts the prediction over from
    /// it; one older than the history is too late to matter.
    pub fn reconcile(
        &mut self,
        tick: u32,
        car: Car,
        mode: Mode,
        own_grade: Option<StartGrade>,
        tuning: &CarTuning,
        track: &Track,
    ) -> Option<f32> {
        let Some(index) = self
            .ticks
            .iter()
            .position(|predicted| predicted.tick == tick)
        else {
            if self.newest().is_none_or(|newest| tick > newest) {
                self.start_over(tick, car, mode, own_grade);
            }
            return None;
        };
        let target = self.newest().unwrap_or(tick);
        let error = self.ticks[index].car.position.distance(car.position);
        // Snapshots come in order: nothing before this tick will be put right again.
        self.ticks.drain(..index);
        self.ticks.truncate(1);
        self.ticks[0].car = car;
        self.advance(target, mode, tuning, track);
        Some(error)
    }

    /// The car at fractional tick `tick`, between the two predicted ticks around it.
    pub fn sample(&self, tick: f64) -> Option<Car> {
        let first = self.ticks.front()?;
        let whole = tick.floor();
        let from = whole as i64 - i64::from(first.tick);
        let at = |index: i64| {
            usize::try_from(index)
                .ok()
                .and_then(|index| self.ticks.get(index))
        };
        match (at(from), at(from + 1)) {
            (Some(from), Some(to)) => Some(interpolate(&from.car, &to.car, (tick - whole) as f32)),
            // Before the oldest tick, or on the newest: the nearest there is.
            _ if from < 0 => Some(first.car),
            _ => self.ticks.back().map(|predicted| predicted.car),
        }
    }

    /// Forgets everything: the car is not the player's to drive any more.
    pub fn clear(&mut self) {
        self.ticks.clear();
    }

    fn step(&self, from: Predicted, mode: Mode, tuning: &CarTuning, track: &Track) -> Predicted {
        let tick = from.tick + 1;
        let mut next = Predicted { tick, ..from };
        for &(at, input) in &self.changes {
            if at == tick {
                if input.accelerate != next.input.accelerate {
                    next.start.accelerator(input.accelerate, f64::from(tick));
                }
                next.input = input;
            }
        }
        match mode {
            Mode::Free => next.car.step(next.input, tuning, track),
            Mode::Racing { start_tick } => {
                if next.race != Some(start_tick) {
                    next.start.reset();
                    next.race = Some(start_tick);
                }
                race::racing_tick(
                    &mut next.car,
                    &mut next.start,
                    next.input,
                    tick,
                    start_tick,
                    tuning,
                    track,
                );
            }
        }
        next
    }

    /// Starts predicting from the server's state on `tick`, with nothing predicted to go on.
    fn start_over(&mut self, tick: u32, car: Car, mode: Mode, own_grade: Option<StartGrade>) {
        let input = self
            .changes
            .iter()
            .rev()
            .find(|(at, _)| *at <= tick)
            .map_or(self.settled, |(_, input)| *input);
        let (start, race) = match mode {
            // A start graded already must not be graded again, and pay its boost twice.
            Mode::Racing { start_tick } => match own_grade {
                Some(grade) => (StartTiming::graded(grade), Some(start_tick)),
                None => (Self::held(input, tick), Some(start_tick)),
            },
            Mode::Free => (Self::held(input, tick), None),
        };
        self.ticks.clear();
        self.ticks.push_back(Predicted {
            tick,
            car,
            start,
            race,
            input,
        });
    }

    /// A start with the accelerator held from `tick` on, if it is.
    fn held(input: CarInput, tick: u32) -> StartTiming {
        let mut start = StartTiming::default();
        if input.accelerate {
            start.accelerator(true, f64::from(tick));
        }
        start
    }

    /// Changes that took effect on or before the oldest tick kept are in that tick's input.
    fn forget_settled_changes(&mut self) {
        let Some(oldest) = self.ticks.front().map(|predicted| predicted.tick) else {
            return;
        };
        while let Some(&(at, input)) = self.changes.front()
            && at <= oldest
        {
            self.settled = input;
            self.changes.pop_front();
        }
    }
}

/// How far the displayed car still is from the predicted one after a correction: shown as a
/// glide back rather than a jump.
#[derive(Debug, Default, Clone, Copy)]
struct Glide {
    offset: Vec2,
    heading: f32,
}

impl Glide {
    /// The car shown as `before` is now predicted as `after`: the display keeps where it was and
    /// glides from there, unless the jump is a teleport.
    fn corrected(&mut self, before: &Car, after: &Car) {
        let offset = self.offset + (before.position - after.position);
        if offset.length() > SNAP_DISTANCE {
            *self = Self::default();
            return;
        }
        self.offset = offset;
        self.heading =
            (self.heading + shortest_angle(after.heading, before.heading)).clamp(-PI, PI);
    }

    fn fade(&mut self, seconds: f32) {
        let keep = (-seconds / CORRECTION_FADE).exp();
        self.offset *= keep;
        self.heading *= keep;
    }

    fn apply(&self, car: Car) -> Car {
        Car {
            position: car.position + self.offset,
            heading: car.heading + self.heading,
            ..car
        }
    }
}

/// The prediction, its timeline and what is displayed of it.
#[derive(Resource, Default)]
pub struct Prediction {
    predictor: Predictor,
    /// The player's timeline, fractional: the server's time as the snapshots tell it, plus the
    /// lead. `None` before the first snapshot.
    timeline: Option<f64>,
    /// How far ahead of the server the timeline runs, in ticks.
    lead: Option<f64>,
    glide: Glide,
    /// The player's car as displayed this frame, while it is predicted.
    displayed: Option<Car>,
}

impl Prediction {
    /// The tick an input made now takes effect on, placed there: the next one on the player's
    /// timeline. Before any snapshot there is no timeline, and the server puts it on its next.
    pub fn place(&mut self, input: CarInput) -> u32 {
        let next = self
            .timeline
            .map_or(0, |timeline| timeline.floor() as u32 + 1);
        self.predictor.place(input, next)
    }

    /// The player's car as displayed this frame, while it is predicted.
    pub fn car(&self) -> Option<Car> {
        self.displayed
    }

    /// The player's timeline, while their car is predicted.
    pub fn own_tick(&self) -> Option<f64> {
        self.displayed.and(self.timeline)
    }

    /// The tick the player's car is displayed at: a tick behind the timeline, so there are always
    /// two predicted ticks to show it between.
    pub fn displayed_tick(&self) -> Option<f64> {
        self.own_tick().map(|timeline| timeline - 1.0)
    }

    /// The lead the timeline runs with, in milliseconds.
    pub fn lead_ms(&self) -> Option<f64> {
        self.lead
            .map(|lead| lead * f64::from(TICK_SECONDS) * 1000.0)
    }

    /// Moves the timeline to the server's time `server_seconds`, ahead by a lead that follows the
    /// measured round trip. It never goes back.
    fn follow_clock(&mut self, server_seconds: f64, round_trip_ms: Option<f64>, elapsed: f64) {
        let tick_ms = f64::from(TICK_SECONDS) * 1000.0;
        let wanted = (round_trip_ms.unwrap_or(DEFAULT_ROUND_TRIP_MS) / tick_ms + MARGIN_TICKS)
            .clamp(1.0, f64::from(MAX_INPUT_LEAD) - MARGIN_TICKS);
        let lead = match self.lead {
            Some(lead) => lead + (wanted - lead) * (1.0 - (-LEAD_FOLLOW * elapsed).exp()),
            None => wanted,
        };
        self.lead = Some(lead);
        let timeline = server_seconds / f64::from(TICK_SECONDS) + lead;
        self.timeline = Some(
            self.timeline
                .map_or(timeline, |before| before.max(timeline)),
        );
    }

    fn stop(&mut self) {
        self.predictor.clear();
        self.glide = Glide::default();
        self.displayed = None;
    }
}

/// Runs the prediction for this frame: the clock, the newest snapshot of the player's car put in
/// place, the ticks up to the timeline, and what is displayed.
#[allow(clippy::too_many_arguments)]
pub fn predict(
    mut events: MessageReader<NetEvent>,
    buffer: Res<SnapshotBuffer>,
    current: Res<CurrentLobby>,
    track: Option<Res<CurrentTrack>>,
    time: Res<Time<Real>>,
    mut latency: ResMut<Latency>,
    mut prediction: ResMut<Prediction>,
) {
    let Some(membership) = &current.0 else {
        *prediction = Prediction::default();
        return;
    };
    // The timeline runs whatever the player does: an input needs a tick even while spectating.
    let now = time
        .last_update()
        .map(|frame| seconds_since_startup(&time, frame));
    if let Some(server) = now.and_then(|now| buffer.server_time(now)) {
        prediction.follow_clock(server, latency.ping.mean(), time.delta_secs_f64());
    }
    if let Some(lead) = prediction.lead_ms() {
        latency.lead(lead);
    }

    let own = membership.car;
    let newest = events
        .read()
        .filter_map(|event| match event {
            NetEvent::Snapshot { snapshot, .. } => snapshot
                .cars
                .iter()
                .find(|car| car.id == own)
                .map(|car| (snapshot.tick, car.car)),
            _ => None,
        })
        .max_by_key(|(tick, _)| *tick);

    let (Some(mode), Some(state), Some(track), Some(timeline)) = (
        Mode::of(membership),
        membership.state.as_ref(),
        track,
        prediction.timeline,
    ) else {
        prediction.stop();
        return;
    };
    let tuning = &state.tuning;
    let own_grade = membership.player(own).and_then(|player| player.start);
    let shown_at = timeline - 1.0;

    prediction.glide.fade(time.delta_secs());
    let Prediction {
        predictor, glide, ..
    } = &mut *prediction;
    if let Some((tick, car)) = newest {
        let before = predictor.sample(shown_at);
        if let Some(error) = predictor.reconcile(tick, car, mode, own_grade, tuning, &track.0) {
            latency.corrected(error);
        }
        predictor.advance(timeline.floor() as u32, mode, tuning, &track.0);
        if let (Some(before), Some(after)) = (before, predictor.sample(shown_at)) {
            glide.corrected(&before, &after);
        }
    }
    predictor.advance(timeline.floor() as u32, mode, tuning, &track.0);
    prediction.displayed = prediction
        .predictor
        .sample(shown_at)
        .map(|car| prediction.glide.apply(car));
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::TrackDescription;

    use super::*;

    /// The server's own track and tuning: what the prediction really runs with.
    fn esplanade() -> (Track, CarTuning) {
        let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../server/data");
        let read = |path: &str| std::fs::read_to_string(format!("{data}/{path}")).unwrap();
        let description: TrackDescription = ron::from_str(&read("tracks/esplanade.ron")).unwrap();
        let tuning: CarTuning = ron::from_str(&read("car.ron")).unwrap();
        (Track::build(&description).unwrap(), tuning)
    }

    /// What the server does with a player's car: each input put into effect on the tick it was
    /// placed on (or `late` ticks after it), then the car stepped by the same rules.
    struct Server {
        car: Car,
        start: StartTiming,
        input: CarInput,
        inputs: Vec<(u32, CarInput)>,
    }

    impl Server {
        fn new(car: Car) -> Self {
            Self {
                car,
                start: StartTiming::default(),
                input: CarInput::default(),
                inputs: Vec::new(),
            }
        }

        fn tick(&mut self, tick: u32, mode: Mode, tuning: &CarTuning, track: &Track) -> Car {
            for &(at, input) in &self.inputs {
                if at == tick {
                    if input.accelerate != self.input.accelerate {
                        self.start.accelerator(input.accelerate, f64::from(tick));
                    }
                    self.input = input;
                }
            }
            match mode {
                Mode::Free => self.car.step(self.input, tuning, track),
                Mode::Racing { start_tick } => {
                    race::racing_tick(
                        &mut self.car,
                        &mut self.start,
                        self.input,
                        tick,
                        start_tick,
                        tuning,
                        track,
                    );
                }
            }
            self.car
        }
    }

    /// A lap's worth of driving, with the inputs changing every half second: accelerate, then
    /// steer both ways, drift, let go.
    fn inputs_every_half_second(first: u32) -> Vec<(u32, CarInput)> {
        [
            CarInput::new(true, 0.0),
            CarInput::new(true, -0.6),
            CarInput::new(true, 1.0).with_drift(true),
            CarInput::new(true, 0.3),
            CarInput::new(false, -1.0),
            CarInput::new(true, 0.0).with_drift(true),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, input)| (first + 30 * index as u32, input))
        .collect()
    }

    /// Runs a server and a predictor side by side, the prediction `lead` ticks ahead and each
    /// snapshot arriving `delay` ticks after it was taken, the server applying every input `late`
    /// ticks after the tick it was placed on. Returns the error of every reconciliation.
    fn race_against_the_server(mode: Mode, late: u32, lead: u32, delay: u32) -> Vec<f32> {
        let (track, tuning) = esplanade();
        let first = 10;
        let grid = Car::on_grid(&track, 0);
        let mut predictor = Predictor::default();
        predictor.reconcile(first, grid, mode, None, &tuning, &track);
        let mut server = Server::new(grid);
        let inputs = inputs_every_half_second(first + 20);
        for &(at, input) in &inputs {
            predictor.place(input, at);
            server.inputs.push((at + late, input));
        }

        let mut snapshots = VecDeque::new();
        let mut errors = Vec::new();
        for tick in first + 1..first + 240 {
            snapshots.push_back((tick, server.tick(tick, mode, &tuning, &track)));
            predictor.advance(tick + lead, mode, &tuning, &track);
            while let Some(&(taken, car)) = snapshots.front()
                && taken + delay <= tick
            {
                snapshots.pop_front();
                errors.extend(predictor.reconcile(taken, car, mode, None, &tuning, &track));
            }
        }
        errors
    }

    /// The whole point: when every input arrives in time, the server's state is exactly what was
    /// predicted for its tick, and there is nothing to correct.
    #[test]
    fn a_prediction_the_server_agrees_with_needs_no_correction() {
        let errors = race_against_the_server(Mode::Free, 0, 8, 5);
        assert!(errors.len() > 200, "{}", errors.len());
        assert!(errors.iter().all(|&error| error == 0.0), "{errors:?}");
    }

    /// An input the server could only apply late puts the car somewhere else: the prediction is
    /// corrected, and agrees again once the replay has caught up.
    #[test]
    fn a_late_input_is_corrected_and_the_prediction_agrees_again() {
        let errors = race_against_the_server(Mode::Free, 2, 8, 5);
        assert!(errors.iter().any(|&error| error > 0.0));
        // Each late input costs a correction or two, while the server has not applied it yet;
        // the rest of the time the prediction agrees exactly.
        let exact = errors.iter().filter(|&&error| error == 0.0).count();
        assert!(exact > errors.len() / 2, "{exact} of {}", errors.len());
    }

    /// On the grid, then off it, graded and boosted: the same rules on both sides, so the same
    /// start.
    #[test]
    fn a_start_is_predicted_exactly_as_the_server_grades_it() {
        let errors = race_against_the_server(Mode::Racing { start_tick: 40 }, 0, 8, 5);
        assert!(errors.iter().all(|&error| error == 0.0), "{errors:?}");
    }

    #[test]
    fn inputs_are_never_placed_before_an_earlier_one() {
        let mut predictor = Predictor::default();
        assert_eq!(predictor.place(CarInput::new(true, 0.0), 20), 20);
        assert_eq!(predictor.place(CarInput::new(false, 0.0), 18), 20);
        assert_eq!(predictor.place(CarInput::new(true, 0.0), 25), 25);
    }

    #[test]
    fn a_correction_glides_back_unless_it_is_a_teleport() {
        let at = |x: f32| Car {
            position: Vec2::new(x, 0.0),
            ..Car::default()
        };
        let mut glide = Glide::default();
        glide.corrected(&at(10.0), &at(10.5));
        assert_eq!(glide.apply(at(10.5)).position.x, 10.0);
        glide.fade(CORRECTION_FADE * 5.0);
        assert!(glide.offset.length() < 0.01);

        glide.corrected(&at(10.0), &at(50.0));
        assert_eq!(glide.offset, Vec2::ZERO);
    }
}
