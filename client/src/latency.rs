//! How late the game is, measured rather than guessed.
//!
//! Every delay is timed from the moment something happens on the player's side, since that is
//! where the player starts waiting:
//!
//! - **ping**: a round trip to the server and back. It counts from the system that sent it, as an
//!   input does, so it compares with the two below; on the web that includes the wait for the
//!   frame to end before the message leaves.
//! - **input → server**: from the input changing to the arrival of the first snapshot that
//!   includes it: the way out, the wait for the server's next tick, and the way back.
//! - **input → screen**: from the same moment to the first frame that shows the player's car with
//!   it. While the car is predicted, that is as soon as the prediction reaches the tick the input
//!   was placed on; otherwise, the first frame that displays the snapshot including it, three
//!   ticks in the past. It is what the player feels, short of the frame that sampled the input
//!   and the one that reaches the glass.
//! - **correction**: how far the prediction was from the server when a snapshot put it right, in
//!   centimeters. Zero while every input arrives in time.
//!
//! Beside them, the **lead** the prediction runs ahead of the server with, the **buffer** -- how
//! far the displayed moment trails the newest snapshot, the interpolation delay and whatever the
//! clock estimate adds to it -- and the **frame**.
//!
//! What ties a snapshot to an input is `CarSnapshot::input_seq`: the server echoes, for every
//! car, the number of the last input in effect when it took the snapshot (see `docs/latency.md`).

use std::collections::VecDeque;
use std::time::Duration;

use bevy::platform::time::Instant;
use bevy::prelude::*;
use space_race_protocol::ClientMessage;
use space_race_sim::TICK_SECONDS;

use crate::lobby::CurrentLobby;
use crate::net::{Connection, NetEvent, Network};
use crate::prediction::Prediction;
use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;

/// How often the server is pinged while connected.
const PING_INTERVAL: Duration = Duration::from_millis(500);
/// How many measurements each figure is worked out from: about half a second of frames, sixteen
/// seconds of pings, the last 32 inputs.
const WINDOW: usize = 32;
/// Pings and inputs still waiting beyond this many are forgotten, oldest first.
const PENDING: usize = 64;
/// How often the figures go to the log, when its level lets them through (`--log info`).
const LOG_INTERVAL: Duration = Duration::from_secs(5);

pub struct LatencyPlugin;

impl Plugin for LatencyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Latency>()
            .init_resource::<LatencyOverlay>()
            .add_systems(OnEnter(Screen::Lobby), forget_the_last_lobby)
            .add_systems(Update, (ping.run_if(online), receive, toggle_overlay))
            .add_systems(
                Update,
                (displayed, log_figures)
                    .chain()
                    .after(receive)
                    .after(RaceUpdate)
                    .run_if(in_state(Screen::Lobby)),
            );
    }
}

/// Whether the figures are shown over the race. F3 toggles it; `--latency`, or `?latency` on the
/// web, starts with it on, which is how a phone gets to see them.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LatencyOverlay(pub bool);

/// The figures, and what is still waiting to be timed.
#[derive(Resource, Default)]
pub struct Latency {
    pub ping: Samples,
    pub to_server: Samples,
    pub to_screen: Samples,
    /// In centimeters.
    pub corrections: Samples,
    pub lead: Samples,
    pub buffer: Samples,
    pub frame: Samples,
    next_ping: u32,
    last_ping: Option<Instant>,
    /// Pings on their way, by number: the server answers them in order.
    pings: VecDeque<(u32, Instant)>,
    /// Inputs sent and not on screen yet, in the order they were sent.
    inputs: VecDeque<Pending>,
    /// The newest snapshot received in this lobby, by tick.
    newest_tick: Option<u32>,
}

/// An input sent and not on screen yet.
struct Pending {
    seq: u32,
    sent: Instant,
    /// The tick it takes effect on, on the player's timeline.
    placed: u32,
    /// The first snapshot that included it: its tick, and when it arrived.
    included: Option<(u32, Instant)>,
}

impl Latency {
    /// The input numbered `seq` left at `sent`, placed on tick `placed`.
    pub fn input_sent(&mut self, seq: u32, sent: Instant, placed: u32) {
        if self.inputs.len() == PENDING {
            self.inputs.pop_front();
        }
        self.inputs.push_back(Pending {
            seq,
            sent,
            placed,
            included: None,
        });
    }

    /// A snapshot put the prediction right, which was `meters` off.
    pub fn corrected(&mut self, meters: f32) {
        self.corrections.push(f64::from(meters) * 100.0);
    }

    /// The prediction runs `millis` ahead of the server.
    pub fn lead(&mut self, millis: f64) {
        self.lead.push(millis);
    }

    /// The figures, one line each: what they measure, and what they are now.
    pub fn lines(&self) -> [(&'static str, String); 7] {
        [
            ("PING", self.ping.spread()),
            ("INPUT \u{2192} SERVER", self.to_server.worst()),
            ("INPUT \u{2192} SCREEN", self.to_screen.worst()),
            ("CORRECTION", self.corrections.worst_in("cm", 1)),
            ("LEAD", self.lead.mean_only(0)),
            ("BUFFER", self.buffer.mean_only(0)),
            ("FRAME", self.frame.mean_only(1)),
        ]
    }

    /// The ping numbered `number` came back at `received`. Anything sent before it that has not
    /// come back never will: the connection is ordered.
    fn pong(&mut self, number: u32, received: Instant) {
        while let Some((sent_number, sent)) = self.pings.pop_front() {
            if sent_number == number {
                self.ping.push(millis(received, sent));
                break;
            }
        }
    }

    /// A snapshot of tick `tick` arrived at `received`, including the player's inputs up to
    /// `own_seq`, or without the player's car at all.
    fn snapshot(&mut self, tick: u32, own_seq: Option<u32>, received: Instant) {
        self.newest_tick = Some(self.newest_tick.map_or(tick, |newest| newest.max(tick)));
        let Some(own_seq) = own_seq else {
            // Spectating: the inputs go nowhere, and would be timed from here until the next race
            // gives the player a car.
            self.inputs.clear();
            return;
        };
        for pending in &mut self.inputs {
            if pending.included.is_none() && pending.seq <= own_seq {
                pending.included = Some((tick, received));
                self.to_server.push(millis(received, pending.sent));
            }
        }
    }

    /// The race is displayed at `tick`, and the player's predicted car at `own_tick` if it is
    /// predicted, at `now`. An input is on screen once the predicted car reaches the tick it was
    /// placed on, or, without a prediction, once the displayed moment reaches the first snapshot
    /// that included it.
    fn displayed(&mut self, tick: f64, own_tick: Option<f64>, now: Instant) {
        while let Some(pending) = self.inputs.front() {
            let shown = match (own_tick, pending.included) {
                (Some(own_tick), _) => own_tick >= f64::from(pending.placed),
                (None, Some((included, _))) => tick >= f64::from(included),
                (None, None) => false,
            };
            if !shown {
                break;
            }
            let waited = millis(now, pending.sent);
            self.to_screen.push(waited);
            self.inputs.pop_front();
        }
        if let Some(newest) = self.newest_tick {
            self.buffer
                .push((f64::from(newest) - tick) * f64::from(TICK_SECONDS) * 1000.0);
        }
    }
}

/// The last few measurements of one delay, in milliseconds.
#[derive(Default)]
pub struct Samples(VecDeque<f64>);

impl Samples {
    fn push(&mut self, millis: f64) {
        if self.0.len() == WINDOW {
            self.0.pop_front();
        }
        self.0.push_back(millis);
    }

    pub fn mean(&self) -> Option<f64> {
        (!self.0.is_empty()).then(|| self.0.iter().sum::<f64>() / self.0.len() as f64)
    }

    pub fn min(&self) -> Option<f64> {
        self.0.iter().copied().reduce(f64::min)
    }

    pub fn max(&self) -> Option<f64> {
        self.0.iter().copied().reduce(f64::max)
    }

    /// The mean, and how far the measurements spread around it.
    fn spread(&self) -> String {
        match (self.mean(), self.min(), self.max()) {
            (Some(mean), Some(min), Some(max)) => {
                format!("{mean:.0} ms  ({min:.0}\u{2013}{max:.0})")
            }
            _ => NOTHING_YET.into(),
        }
    }

    /// The mean, and the worst of them: a delay is felt at its worst.
    fn worst(&self) -> String {
        match (self.mean(), self.max()) {
            (Some(mean), Some(max)) => format!("{mean:.0} ms  (max {max:.0})"),
            _ => NOTHING_YET.into(),
        }
    }

    /// The same, in another unit than milliseconds.
    fn worst_in(&self, unit: &str, decimals: usize) -> String {
        match (self.mean(), self.max()) {
            (Some(mean), Some(max)) => {
                format!("{mean:.decimals$} {unit}  (max {max:.decimals$})")
            }
            _ => NOTHING_YET.into(),
        }
    }

    fn mean_only(&self, decimals: usize) -> String {
        self.mean()
            .map_or(NOTHING_YET.into(), |mean| format!("{mean:.decimals$} ms"))
    }
}

const NOTHING_YET: &str = "\u{2014}";

fn millis(later: Instant, earlier: Instant) -> f64 {
    later.saturating_duration_since(earlier).as_secs_f64() * 1000.0
}

fn online(connection: Res<Connection>) -> bool {
    matches!(*connection, Connection::Online { .. })
}

fn ping(network: Res<Network>, mut latency: ResMut<Latency>) {
    let now = Instant::now();
    if latency
        .last_ping
        .is_some_and(|last| now.saturating_duration_since(last) < PING_INTERVAL)
    {
        return;
    }
    latency.last_ping = Some(now);
    let number = latency.next_ping;
    latency.next_ping = number.wrapping_add(1);
    if latency.pings.len() == PENDING {
        latency.pings.pop_front();
    }
    latency.pings.push_back((number, now));
    network.send(ClientMessage::Ping(number));
}

fn receive(
    mut events: MessageReader<NetEvent>,
    current: Res<CurrentLobby>,
    mut latency: ResMut<Latency>,
) {
    let own = current.0.as_ref().map(|membership| membership.car);
    for event in events.read() {
        match event {
            NetEvent::Pong { number, received } => latency.pong(*number, *received),
            // Snapshots still on their way after leaving a lobby are not for this one.
            NetEvent::Snapshot { snapshot, received } if own.is_some() => {
                let own_seq = snapshot
                    .cars
                    .iter()
                    .find(|car| Some(car.id) == own)
                    .map(|car| car.input_seq);
                latency.snapshot(snapshot.tick, own_seq, *received);
            }
            _ => {}
        }
    }
}

/// A new lobby counts its ticks and the player's inputs from scratch. The pings and the figures
/// carry on: it is the same connection.
fn forget_the_last_lobby(mut latency: ResMut<Latency>) {
    latency.inputs.clear();
    latency.newest_tick = None;
}

fn displayed(
    view: Res<RaceView>,
    prediction: Res<Prediction>,
    time: Res<Time<Real>>,
    mut latency: ResMut<Latency>,
) {
    latency.frame.push(time.delta_secs_f64() * 1000.0);
    if let Some(tick) = view.tick {
        latency.displayed(tick, prediction.displayed_tick(), Instant::now());
    }
}

fn log_figures(latency: Res<Latency>, mut last: Local<Option<Instant>>) {
    let now = Instant::now();
    let last = last.get_or_insert(now);
    if now.saturating_duration_since(*last) < LOG_INTERVAL {
        return;
    }
    *last = now;
    let figures: Vec<String> = latency
        .lines()
        .iter()
        .map(|(what, value)| format!("{} {value}", what.to_lowercase()))
        .collect();
    info!("latency: {}", figures.join(" \u{b7} "));
}

fn toggle_overlay(keyboard: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<LatencyOverlay>) {
    if keyboard.just_pressed(KeyCode::F3) {
        overlay.0 = !overlay.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn samples_keep_the_last_window() {
        let mut samples = Samples::default();
        assert_eq!(samples.mean(), None);
        for millis in 0..(WINDOW as u32 + 8) {
            samples.push(f64::from(millis));
        }
        // The first eight are gone.
        assert_eq!(samples.min(), Some(8.0));
        assert_eq!(samples.max(), Some(f64::from(WINDOW as u32 + 7)));
        assert_eq!(samples.mean(), Some(8.0 + (WINDOW as f64 - 1.0) / 2.0));
    }

    /// An input is timed twice: to the snapshot that includes it, and to the frame showing it.
    #[test]
    fn an_input_is_timed_to_the_server_and_to_the_screen() {
        let start = Instant::now();
        let mut latency = Latency::default();
        latency.input_sent(1, start, 0);

        // A snapshot taken before the server had it changes nothing.
        latency.snapshot(99, Some(0), after(start, 25));
        assert_eq!(latency.to_server.mean(), None);
        // The first one including it arrives 40 ms after it left.
        latency.snapshot(100, Some(1), after(start, 40));
        assert_eq!(latency.to_server.mean(), Some(40.0));

        // Displayed three ticks behind: not there yet halfway to it, then there.
        latency.displayed(99.5, None, after(start, 70));
        assert_eq!(latency.to_screen.mean(), None);
        latency.displayed(100.0, None, after(start, 95));
        assert_eq!(latency.to_screen.mean(), Some(95.0));
        assert!(latency.inputs.is_empty());
    }

    /// While the car is predicted, an input is on screen as soon as the prediction reaches its
    /// tick, long before the server has even seen it.
    #[test]
    fn a_predicted_input_is_on_screen_at_its_tick() {
        let start = Instant::now();
        let mut latency = Latency::default();
        latency.input_sent(1, start, 50);
        latency.displayed(10.0, Some(49.5), after(start, 8));
        assert_eq!(latency.to_screen.mean(), None);
        latency.displayed(10.0, Some(50.0), after(start, 20));
        assert_eq!(latency.to_screen.mean(), Some(20.0));
    }

    /// One snapshot can include several inputs, if they changed faster than the server ticks.
    #[test]
    fn a_snapshot_includes_every_input_up_to_its_number() {
        let start = Instant::now();
        let mut latency = Latency::default();
        latency.input_sent(1, start, 0);
        latency.input_sent(2, after(start, 5), 0);
        latency.snapshot(10, Some(2), after(start, 30));
        assert_eq!(latency.to_server.min(), Some(25.0));
        assert_eq!(latency.to_server.max(), Some(30.0));
    }

    /// Inputs sent while spectating would be timed from there until the next race: they are not
    /// timed at all.
    #[test]
    fn inputs_without_a_car_are_not_timed() {
        let start = Instant::now();
        let mut latency = Latency::default();
        latency.input_sent(1, start, 0);
        latency.snapshot(10, None, after(start, 30));
        assert!(latency.inputs.is_empty());
        latency.snapshot(11, Some(1), after(start, 30_000));
        assert_eq!(latency.to_server.mean(), None);
    }

    #[test]
    fn a_pong_times_its_own_ping_and_drops_older_ones() {
        let start = Instant::now();
        let mut latency = Latency::default();
        latency
            .pings
            .extend([(0, start), (1, after(start, 500)), (2, after(start, 1000))]);
        latency.pong(1, after(start, 523));
        assert_eq!(latency.ping.mean(), Some(23.0));
        assert_eq!(latency.pings.len(), 1);
    }
}
