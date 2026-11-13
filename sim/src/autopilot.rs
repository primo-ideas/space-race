//! A driver that follows the road at full speed. It proves a circuit can be driven in tests, lets
//! the client drive on its own for unattended checks, and takes over a car once its player has
//! finished the race, while the others come in. It is not an opponent AI.
//!
//! It drives in one of two [`Style`]s: gripping all the way, or drifting through the turns, which
//! also measures what drifting gains on a track. And it drives with a [`Personality`]: a line of its
//! own across the road, weaving from one side toward the other, how far ahead it looks, how eager
//! it is to drift and to fire its turbo. Without one, the neutral driver the tests measure
//! circuits with follows the centerline; every car on a track gets one of its own, so a field of
//! them spreads out instead of following each other down the middle.

use std::f32::consts::TAU;

use std::collections::VecDeque;

use super::car::{Car, CarInput, CarTuning};
use super::race::wrapped_delta;
use super::track::Track;
use super::{TICK_RATE, TICK_SECONDS};

/// The target point is this far ahead along the centerline, plus the distance covered in
/// [`LOOK_AHEAD_TIME`]. Looking further ahead at speed smooths the line.
const LOOK_AHEAD_DISTANCE: f32 = 6.0;
const LOOK_AHEAD_TIME: f32 = 0.5;

/// Full lock for this many radians between the heading and the target.
const FULL_LOCK_ANGLE: f32 = 0.25;

/// A drifting autopilot looks at how much the centerline turns over the next
/// [`DRIFT_LOOK_AHEAD`] meters, and taps the drift button into a turn of more than
/// [`DRIFT_START_TURN`] radians.
const DRIFT_LOOK_AHEAD: f32 = 16.0;
const DRIFT_START_TURN: f32 = 0.25;

/// A press of the drift button needs the stick turned at least this far toward the turn, or it
/// starts nothing.
const DRIFT_PRESS_STEER: f32 = 0.5;

/// A drift aims closer than [`LOOK_AHEAD_TIME`]: at drifting speeds, a target that far away cuts
/// through the inside of a hairpin.
const DRIFT_LOOK_AHEAD_TIME: f32 = 0.3;

/// The drifting autopilot fires its turbo where the centerline turns by less than
/// [`TURBO_STRAIGHT`] radians all along the next stretch, looked at every
/// [`TURBO_LOOK_AHEAD_STEP`] meters.
const TURBO_LOOK_AHEAD_STEP: f32 = 15.0;
const TURBO_LOOK_AHEAD_STEPS: usize = 4;
const TURBO_STRAIGHT: f32 = 0.3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Style {
    /// Never touches the drift button. It still drifts when it turns hard enough to break away,
    /// but it never tightens a drift, and a drift ends as soon as it straightens.
    #[default]
    Grip,
    /// Taps the drift button into the turns, and holds it only where the stick alone cannot turn
    /// tightly enough, since holding costs speed. Straightening out of the turn ends the drift,
    /// which pays its boost. It fires the turbo its drifts fill where the road runs straight.
    Drift,
}

/// How one autopilot drives, as against another: what makes a field of them spread out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Personality {
    /// Where its line runs across the road, as a share of the room either side of the centerline
    /// it keeps clear of the walls: 0 down the middle, positive to the left.
    pub line: f32,
    /// How far its line weaves either side of that, as a share of the same room, and how long one
    /// weave is along the road, in meters, starting `phase` radians into it.
    pub weave: f32,
    pub weave_length: f32,
    pub phase: f32,
    /// How far ahead it looks, as a share of the neutral driver's.
    pub look_ahead: f32,
    /// How sharp a turn ahead must be for it to drift, as a share of the neutral driver's: below 1,
    /// it drifts into gentler turns.
    pub drift_threshold: f32,
    /// How straight the road ahead must be for it to fire its turbo, as a share of the neutral
    /// driver's: above 1, it fires on bends a careful driver would wait out.
    pub turbo_nerve: f32,
}

impl Default for Personality {
    /// The neutral driver: down the centerline, as the tests measure circuits.
    fn default() -> Self {
        Self {
            line: 0.0,
            weave: 0.0,
            weave_length: 400.0,
            phase: 0.0,
            look_ahead: 1.0,
            drift_threshold: 1.0,
            turbo_nerve: 1.0,
        }
    }
}

/// How much of the road's half width either side of the centerline a line may use, beyond the
/// car's own size: what keeps a weaving line off the walls.
const LINE_MARGIN: f32 = 4.5;

impl Personality {
    /// A personality of its own for `seed`, the same every time for the same seed: a car keeps
    /// its manner from lap to lap and from the server to a client's screen.
    pub fn of(seed: u32) -> Self {
        // Each number drawn from the seed by a different mixing, so they do not move together.
        let draw = |salt: u32| {
            let mut x = seed.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
            x ^= x >> 16;
            x = x.wrapping_mul(0x7FEB_352D);
            x ^= x >> 15;
            x = x.wrapping_mul(0x846C_A68B);
            x ^= x >> 16;
            x as f32 / u32::MAX as f32
        };
        let between = |salt: u32, low: f32, high: f32| low + (high - low) * draw(salt);
        Self {
            line: between(1, -0.4, 0.4),
            weave: between(2, 0.08, 0.28),
            weave_length: between(3, 220.0, 520.0),
            phase: between(4, 0.0, TAU),
            look_ahead: between(5, 0.85, 1.2),
            drift_threshold: between(6, 0.75, 1.35),
            turbo_nerve: between(7, 0.8, 1.6),
        }
    }

    /// How far left of the centerline this driver's line runs `distance` meters along the lap,
    /// in meters, on a road `half_width` meters from the centerline to either wall there.
    pub fn offset(&self, distance: f32, half_width: f32) -> f32 {
        let room = (half_width - LINE_MARGIN).max(0.0);
        let wave = (TAU * distance / self.weave_length + self.phase).sin();
        ((self.line + self.weave * wave) * room).clamp(-room, room)
    }
}

pub fn drive(car: &Car, track: &Track) -> CarInput {
    drive_as(&Personality::default(), car, track)
}

/// [`drive`], with a personality.
pub fn drive_as(personality: &Personality, car: &Car, track: &Track) -> CarInput {
    CarInput::new(true, pursue(personality, car, track, LOOK_AHEAD_TIME))
}

/// The stick that points the car at a spot of its line ahead, `look_ahead_time` seconds of travel
/// away: full lock for [`FULL_LOCK_ANGLE`] between the heading and the spot, and more than full
/// lock, beyond 1, when that is not enough. A drift steers its travel with the same stick as a
/// gripping car steers its nose, on a radius of the same kind, so the one pursuit drives both.
fn pursue(personality: &Personality, car: &Car, track: &Track, look_ahead_time: f32) -> f32 {
    let here = track.project(car.position);
    let look_ahead =
        (LOOK_AHEAD_DISTANCE + car.velocity.length() * look_ahead_time) * personality.look_ahead;
    let ahead = here.distance + look_ahead;
    let point = track.point_at(ahead);
    let offset = personality.offset(ahead, track.half_width_at(ahead.rem_euclid(track.length())));
    // The direction is a unit tangent, so its perpendicular is the left of the road.
    let target = point.position + point.direction.perp() * offset;
    // Positive when the target is on the left; steering is right positive.
    let angle = car.forward().angle_to(target - car.position);
    -angle / FULL_LOCK_ANGLE
}

/// [`drive`] in a given style.
///
/// Drifting decides from the road ahead: a tap of the button with the stick into a turn that is
/// coming throws the car into a drift, the pursuit then steers the drift's curve, and the button
/// is held only while the pursuit asks for more than full lock into the drift. Coming out of the
/// turn, the stick straightens, the body comes back into line and the drift pays its boost.
pub fn drive_in_style(style: Style, car: &Car, track: &Track) -> CarInput {
    drive_in_style_as(style, &Personality::default(), car, track)
}

/// [`drive_in_style`], with a personality.
pub fn drive_in_style_as(
    style: Style,
    personality: &Personality,
    car: &Car,
    track: &Track,
) -> CarInput {
    if style == Style::Grip {
        return drive_as(personality, car, track);
    }
    drift_through_turns(personality, car, track)
        .with_turbo(fires_turbo(personality, car, track))
}

/// Whether to press the turbo button now: with a charge to spend, out of a drift, and with the
/// road ahead straight enough to take the speed. A press, not a hold, like the drift's.
fn fires_turbo(personality: &Personality, car: &Car, track: &Track) -> bool {
    if car.turbo < 1.0 || car.turbo_button || car.is_boosting() || car.is_drifting() {
        return false;
    }
    let here = track.project(car.position).distance;
    let now = track.point_at(here).direction;
    (1..=TURBO_LOOK_AHEAD_STEPS).all(|step| {
        let ahead = track.point_at(here + step as f32 * TURBO_LOOK_AHEAD_STEP);
        now.angle_to(ahead.direction).abs() < TURBO_STRAIGHT * personality.turbo_nerve
    })
}

fn drift_through_turns(personality: &Personality, car: &Car, track: &Track) -> CarInput {
    if car.is_drifting() {
        let steer = pursue(personality, car, track, DRIFT_LOOK_AHEAD_TIME);
        // Into the drift is toward its side, which is the sign of `drift` for the stick.
        let holding = steer * f32::from(car.drift) > 1.0;
        return CarInput::new(true, steer).with_drift(holding);
    }
    let input = drive_as(personality, car, track);
    // Positive when the road turns left. A left turn is a drift to the left, side -1.
    let turn = turn_ahead(car, track);
    let side = if turn > 0.0 { -1.0 } else { 1.0 };
    if turn.abs() > DRIFT_START_TURN * personality.drift_threshold && input.steer() * side >= 0.0 {
        // A press, not a hold: the button goes down only if it was up, so a press that came too
        // early to start anything is let go of and made again.
        let steer = side * input.steer().abs().max(DRIFT_PRESS_STEER);
        CarInput::new(true, steer).with_drift(!car.drift_button)
    } else {
        input
    }
}

/// How much the centerline turns over the next [`DRIFT_LOOK_AHEAD`] meters, in radians, positive
/// to the left.
fn turn_ahead(car: &Car, track: &Track) -> f32 {
    let here = track.project(car.position).distance;
    let now = track.point_at(here).direction;
    let ahead = track.point_at(here + DRIFT_LOOK_AHEAD).direction;
    now.angle_to(ahead)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LapReport {
    /// Time to drive one full lap length from the first grid slot, if it did within the limit.
    pub lap_time: Option<f32>,
    /// Ticks spent touching a wall.
    pub wall_ticks: u32,
    /// Drifts started, the longest one in seconds, and ticks spent drifting and boosting.
    pub drifts: u32,
    pub longest_drift: f32,
    pub drift_ticks: u32,
    pub boost_ticks: u32,
    /// Charges of turbo fired.
    pub turbos: u32,
    pub top_speed: f32,
}

/// Lets the autopilot drive one lap from the first grid slot, in `style`.
///
/// The autopilot decides from the car as it was `delay_ticks` ticks earlier, like a client that
/// only sees the car once the server's snapshot has arrived and been interpolated.
pub fn drive_lap(
    track: &Track,
    tuning: &CarTuning,
    style: Style,
    time_limit: f32,
    delay_ticks: usize,
) -> LapReport {
    drive_lap_as(
        track,
        tuning,
        style,
        &Personality::default(),
        time_limit,
        delay_ticks,
    )
}

/// [`drive_lap`], with a personality.
pub fn drive_lap_as(
    track: &Track,
    tuning: &CarTuning,
    style: Style,
    personality: &Personality,
    time_limit: f32,
    delay_ticks: usize,
) -> LapReport {
    let mut car = Car::on_grid(track, 0);
    let mut seen = VecDeque::from(vec![car; delay_ticks + 1]);
    let mut distance = track.project(car.position).distance;
    let mut progress = 0.0;
    let mut report = LapReport {
        lap_time: None,
        wall_ticks: 0,
        drifts: 0,
        longest_drift: 0.0,
        drift_ticks: 0,
        boost_ticks: 0,
        turbos: 0,
        top_speed: 0.0,
    };
    let mut drift_start = None;

    for tick in 1..=(time_limit * TICK_RATE as f32) as u32 {
        let input = drive_in_style_as(style, personality, &seen[0], track);
        let firing = car.turbo_boost > 0.0;
        car.step(input, tuning, track);
        report.turbos += u32::from(car.turbo_boost > 0.0 && !firing);
        seen.pop_front();
        seen.push_back(car);
        let projection = track.project(car.position);
        progress += wrapped_delta(projection.distance - distance, track.length());
        distance = projection.distance;

        // The walls are where the road is narrowest around the car, so a bottleneck counts.
        let limit = track.half_width_at(projection.distance) - tuning.collision_radius;
        if projection.lateral.abs() >= limit - 1e-3 {
            report.wall_ticks += 1;
        }
        report.drift_ticks += u32::from(car.is_drifting());
        match (car.is_drifting(), drift_start) {
            (true, None) => {
                report.drifts += 1;
                drift_start = Some(tick);
            }
            (false, Some(start)) => {
                let seconds = (tick - start) as f32 * TICK_SECONDS;
                report.longest_drift = report.longest_drift.max(seconds);
                drift_start = None;
            }
            _ => {}
        }
        report.boost_ticks += u32::from(car.is_boosting());
        report.top_speed = report.top_speed.max(car.velocity.length());
        if progress >= track.length() {
            report.lap_time = Some(tick as f32 * TICK_SECONDS);
            break;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::test_support::{banked_oval, oval, tuning};

    #[test]
    fn autopilot_laps_an_oval_without_touching_the_walls() {
        let track = Track::build(&oval(120.0, 45.0, 25.0)).unwrap();
        let report = drive_lap(&track, &tuning(), Style::Grip, 60.0, 0);

        let lap_time = report.lap_time.expect("the lap was not completed");
        let average_speed = track.length() / lap_time;
        assert!(average_speed > 0.6 * tuning().top_speed, "{report:?}");
        assert_eq!(report.wall_ticks, 0, "{report:?}");
    }

    /// The client's autopilot sees the car through snapshots and interpolation, about five ticks
    /// late, and must still drive cleanly.
    #[test]
    fn autopilot_copes_with_the_client_delay() {
        let track = Track::build(&oval(120.0, 45.0, 25.0)).unwrap();
        for delay_ticks in [3, 6, 9] {
            let report = drive_lap(&track, &tuning(), Style::Grip, 60.0, delay_ticks);
            println!("delay {delay_ticks} ticks: {report:?}");
            let lap_time = report.lap_time.expect("the lap was not completed");
            let average_speed = track.length() / lap_time;
            assert!(average_speed > 0.6 * tuning().top_speed, "{report:?}");
            assert_eq!(report.wall_ticks, 0, "{report:?}");
        }
    }

    #[test]
    fn autopilot_laps_a_banked_oval_without_touching_the_walls() {
        let track = Track::build(&banked_oval(120.0, 45.0, 25.0, 30.0)).unwrap();
        let report = drive_lap(&track, &tuning(), Style::Grip, 60.0, 6);
        println!("banked: {report:?}");

        let lap_time = report.lap_time.expect("the lap was not completed");
        assert!(
            track.length() / lap_time > 0.6 * tuning().top_speed,
            "{report:?}"
        );
        assert_eq!(report.wall_ticks, 0, "{report:?}");
    }

    #[test]
    fn a_drifting_autopilot_laps_drifting_and_boosting() {
        let track = Track::build(&banked_oval(110.0, 38.0, 24.0, 30.0)).unwrap();
        let grip = drive_lap(&track, &tuning(), Style::Grip, 60.0, 0);
        let drift = drive_lap(&track, &tuning(), Style::Drift, 60.0, 0);
        println!(
            "grip:  {grip:?}
drift: {drift:?}"
        );

        assert!(drift.lap_time.is_some(), "{drift:?}");
        assert!(drift.drift_ticks > 0 && drift.boost_ticks > 0, "{drift:?}");
        assert_eq!(grip.drift_ticks, 0);
    }

    /// Personalities are each their own, the same for the same seed, and keep their line inside
    /// the road whatever it weaves.
    #[test]
    fn personalities_differ_and_keep_off_the_walls() {
        assert_eq!(Personality::of(7), Personality::of(7));
        let lines: Vec<f32> = (0..8).map(|seed| Personality::of(seed).line).collect();
        let spread = lines.iter().cloned().fold(f32::MIN, f32::max)
            - lines.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread > 0.4, "{lines:?}");
        for seed in 0..50 {
            let personality = Personality::of(seed);
            for step in 0..200 {
                let offset = personality.offset(step as f32 * 7.0, 17.0);
                assert!(offset.abs() <= 17.0 - LINE_MARGIN + 1e-4, "{seed} {offset}");
            }
        }
        assert_eq!(Personality::default().offset(123.0, 17.0), 0.0);
    }

    /// A field of autopilots, each with its own personality, laps an oval on lines of their own
    /// rather than all down the middle, and none spends the lap on the walls.
    #[test]
    fn a_field_of_personalities_spreads_out_and_laps() {
        let track = Track::build(&banked_oval(110.0, 45.0, 24.0, 30.0)).unwrap();
        let mut laterals = Vec::new();
        for seed in 0..6 {
            let personality = Personality::of(seed);
            let report = drive_lap_as(&track, &tuning(), Style::Drift, &personality, 60.0, 0);
            assert!(report.lap_time.is_some(), "{seed}: {report:?}");
            assert!(report.wall_ticks < 30, "{seed}: {report:?}");
            // Where it is half a lap in.
            let mut car = Car::on_grid(&track, 0);
            for _ in 0..(TICK_RATE * 4) {
                let input = drive_in_style_as(Style::Grip, &personality, &car, &track);
                car.step(input, &tuning(), &track);
            }
            laterals.push(track.project(car.position).lateral);
        }
        let spread = laterals.iter().cloned().fold(f32::MIN, f32::max)
            - laterals.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread > 1.5, "{laterals:?}");
    }

    #[test]
    fn autopilot_steers_toward_the_centerline() {
        let track = Track::build(&oval(120.0, 45.0, 25.0)).unwrap();
        let mut car = Car::on_grid(&track, 0);
        car.position += Vec2::Y * 4.0;
        // Left of the centerline: steer right.
        assert!(drive(&car, &track).steer > 0);
    }
}
