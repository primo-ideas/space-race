//! A driver that follows the centerline at full speed. It proves a circuit can be driven in tests,
//! lets the client drive on its own for unattended checks, and takes over a car once its player has
//! finished the race, while the others come in. It is not an opponent AI.
//!
//! It drives in one of two [`Style`]s: gripping all the way, or drifting through the turns, which
//! also measures what drifting gains on a track.

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
/// [`DRIFT_LOOK_AHEAD`] meters: it starts a drift beyond [`DRIFT_START_TURN`] radians, and holds it
/// while the road still turns the same way by more than [`DRIFT_HOLD_TURN`].
const DRIFT_LOOK_AHEAD: f32 = 16.0;
const DRIFT_START_TURN: f32 = 0.25;
const DRIFT_HOLD_TURN: f32 = 0.02;

/// Stick a drifting autopilot turns at least, into the turn, to start a drift. Full lock: nothing
/// less breaks the car off its axis, which is the only way into a drift now.
const DRIFT_START_STEER: f32 = 1.0;

/// How hard a drifting autopilot pushes the stick for a given relative error between the
/// curvature it needs and the curvature it drives.
const DRIFT_CURVATURE_GAIN: f32 = 4.0;

/// A drift aims closer than [`LOOK_AHEAD_TIME`]: at drifting speeds, a target that far away cuts
/// through the inside of a hairpin.
const DRIFT_LOOK_AHEAD_TIME: f32 = 0.3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Style {
    /// Never holds the drift button. It still slides when it turns hard, since going off the
    /// axis is what a drift is now, but it never tightens one or carries one past a turn.
    #[default]
    Grip,
    /// Holds the drift button through the turns, tightening the slide, and lets go on the way out
    /// so the drift pays its boost.
    Drift,
}

pub fn drive(car: &Car, track: &Track) -> CarInput {
    let here = track.project(car.position);
    let look_ahead = LOOK_AHEAD_DISTANCE + car.velocity.length() * LOOK_AHEAD_TIME;
    let target = track.point_at(here.distance + look_ahead).position;
    // Positive when the target is on the left; steering is right positive.
    let angle = car.forward().angle_to(target - car.position);
    CarInput::new(true, -angle / FULL_LOCK_ANGLE)
}

/// [`drive`] in a given style.
///
/// Drifting decides from the road ahead, not from the steering, which swings as soon as a drift
/// turns the car harder: it turns hard into a turn that is coming, which is what breaks the car
/// off its axis, and holds the button while the road keeps turning that way. Once a drift is on,
/// its side is locked and the stick only tightens or widens it, which [`drift_steer`] works out.
pub fn drive_in_style(style: Style, car: &Car, track: &Track) -> CarInput {
    let input = drive(car, track);
    if style == Style::Grip {
        return input;
    }

    // Positive when the road turns left. A left turn is a drift to the left, side -1.
    let turn = turn_ahead(car, track);
    let side = if turn > 0.0 { -1.0 } else { 1.0 };
    if car.is_drifting() {
        // Tighten while the road still bends this way; let go on the way out, which lets the car
        // come back into line and pays the boost.
        let holding = turn * -f32::from(car.drift) > DRIFT_HOLD_TURN;
        CarInput::new(true, drift_steer(car, track)).with_drift(holding)
    } else if turn.abs() > DRIFT_START_TURN && input.steer() * side >= 0.0 {
        // Nothing but the stick starts a drift now, so turn into it hard enough to break away.
        let steer = side * input.steer().abs().max(DRIFT_START_STEER);
        CarInput::new(true, steer)
    } else {
        input
    }
}

/// The stick that keeps a drift on the target point, by comparing curvatures.
///
/// Steering from the angle to the target, as [`drive`] does, fails in a drift: the stick then sets
/// a radius rather than a rotation, a neutral stick still turns, and the same gain either
/// oscillates between the tightest and widest drifts or lets the car slide into the inside wall.
/// Instead, the curvature of the arc through the target, tangent to the heading, is compared with
/// the curvature the car drives now, its yaw rate over its speed: the stick goes into the drift
/// when the target needs a tighter arc, out of it when it needs a wider one. It needs no tuning,
/// which the client does not have.
fn drift_steer(car: &Car, track: &Track) -> f32 {
    let here = track.project(car.position);
    let look_ahead = LOOK_AHEAD_DISTANCE + car.velocity.length() * DRIFT_LOOK_AHEAD_TIME;
    let to_target = track.point_at(here.distance + look_ahead).position - car.position;
    // Curvatures toward the side of the drift: positive when turning that way.
    let toward = -f32::from(car.drift);
    let angle = car.forward().angle_to(to_target);
    let wanted = 2.0 * angle.sin() / to_target.length().max(1.0) * toward;
    let current = (car.yaw_rate * toward / car.velocity.length().max(1.0)).max(1e-3);
    let into_drift = DRIFT_CURVATURE_GAIN * (wanted / current - 1.0);
    // Into the drift is toward its side, which is the sign of `drift` for the stick.
    f32::from(car.drift) * into_drift
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
        top_speed: 0.0,
    };
    let mut drift_start = None;

    for tick in 1..=(time_limit * TICK_RATE as f32) as u32 {
        let input = drive_in_style(style, &seen[0], track);
        car.step(input, tuning, track);
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
        report.boost_ticks += u32::from(car.boost > 0.0);
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

    #[test]
    fn autopilot_steers_toward_the_centerline() {
        let track = Track::build(&oval(120.0, 45.0, 25.0)).unwrap();
        let mut car = Car::on_grid(&track, 0);
        car.position += Vec2::Y * 4.0;
        // Left of the centerline: steer right.
        assert!(drive(&car, &track).steer > 0);
    }
}
