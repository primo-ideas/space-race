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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Style {
    /// Never touches the drift button. It still drifts when it turns hard enough to break away,
    /// but it never tightens a drift, and a drift ends as soon as it straightens.
    #[default]
    Grip,
    /// Taps the drift button into the turns, and holds it only where the stick alone cannot turn
    /// tightly enough, since holding costs speed. Straightening out of the turn ends the drift,
    /// which pays its boost.
    Drift,
}

pub fn drive(car: &Car, track: &Track) -> CarInput {
    CarInput::new(true, pursue(car, track, LOOK_AHEAD_TIME))
}

/// The stick that points the car at a spot of the centerline ahead, `look_ahead_time` seconds of
/// travel away: full lock for [`FULL_LOCK_ANGLE`] between the heading and the spot, and more than
/// full lock, beyond 1, when that is not enough. A drift steers its travel with the same stick as
/// a gripping car steers its nose, on a radius of the same kind, so the one pursuit drives both.
fn pursue(car: &Car, track: &Track, look_ahead_time: f32) -> f32 {
    let here = track.project(car.position);
    let look_ahead = LOOK_AHEAD_DISTANCE + car.velocity.length() * look_ahead_time;
    let target = track.point_at(here.distance + look_ahead).position;
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
    if style == Style::Grip {
        return drive(car, track);
    }

    if car.is_drifting() {
        let steer = pursue(car, track, DRIFT_LOOK_AHEAD_TIME);
        // Into the drift is toward its side, which is the sign of `drift` for the stick.
        let holding = steer * f32::from(car.drift) > 1.0;
        return CarInput::new(true, steer).with_drift(holding);
    }
    let input = drive(car, track);
    // Positive when the road turns left. A left turn is a drift to the left, side -1.
    let turn = turn_ahead(car, track);
    let side = if turn > 0.0 { -1.0 } else { 1.0 };
    if turn.abs() > DRIFT_START_TURN && input.steer() * side >= 0.0 {
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
