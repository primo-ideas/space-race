//! Fixtures for the simulation tests.

use super::car::CarTuning;
use super::track::{Segment, TrackDescription};

/// A counter-clockwise oval of width 16 m. The start line is in the middle of a straight, so
/// `straight` is split around it.
pub fn oval(straight: f32, radius: f32, transition: f32) -> TrackDescription {
    banked_oval(straight, radius, transition, 0.0)
}

/// [`oval`] with turns banked by `banking` degrees.
pub fn banked_oval(straight: f32, radius: f32, transition: f32, banking: f32) -> TrackDescription {
    oval_of_width(16.0, straight, radius, transition, banking)
}

/// An oval 300 m wide with 4 km straights: a car can accelerate, drift and circle around the start
/// line without ever meeting a wall.
pub fn open_track() -> TrackDescription {
    oval_of_width(300.0, 4000.0, 200.0, 0.0, 0.0)
}

fn oval_of_width(
    width: f32,
    straight: f32,
    radius: f32,
    transition: f32,
    banking: f32,
) -> TrackDescription {
    let turn = Segment::Turn {
        angle: 180.0,
        radius,
        transition,
        banking,
    };
    TrackDescription {
        name: "Test oval".into(),
        width,
        segments: vec![
            Segment::Straight {
                length: straight / 2.0,
            },
            turn,
            Segment::Straight { length: straight },
            turn,
            Segment::Straight {
                length: straight / 2.0,
            },
        ],
        narrows: Vec::new(),
        scenery: Vec::new(),
    }
}

pub fn tuning() -> CarTuning {
    CarTuning {
        top_speed: 35.0,
        acceleration: 20.0,
        coasting: 4.0,
        turn_radius_slow: 6.0,
        turn_radius_fast: 30.0,
        min_steering_speed: 4.0,
        steering_response: 12.0,
        grip: 12.0,
        gravity: 9.81,
        drift_min_speed: 12.0,
        drift_entry_angle: 4.0,
        drift_exit_angle: 2.0,
        drift_radius_tight: 14.0,
        drift_grip: 4.0,
        drift_steering_response: 10.0,
        drift_button_tighten: 0.7,
        drift_button_drag: 3.0,
        drift_drag: 2.0,
        drift_drag_ramp: 2.0,
        drift_min_charge: 0.3,
        drift_boost_rate: 0.8,
        drift_max_boost: 1.5,
        boost_top_speed: 48.0,
        boost_acceleration: 30.0,
        start_boost: 1.5,
        collision_radius: 1.2,
        wall_bounce: 0.2,
        wall_friction: 1.5,
    }
}
