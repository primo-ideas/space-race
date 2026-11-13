//! The cars' weight, seen. The simulation moves a car over the road and nothing more; how its body
//! moves on top of that is drawn here, so that it looks as heavy as it drives (see
//! `docs/simulation.md#weight`).
//!
//! Every car's body rides on springs of its own. It pitches back as the car gathers speed, rolls to
//! the outside of a turn, sinks into a bowl and at the foot of a dive and lifts over a crest, then
//! settles slowly, overshooting a little, as something heavy does. Nothing is scripted: what pushes
//! the springs is the acceleration the car really has, read from how its displayed velocity changes
//! from one frame to the next, gravity included. A wall jolts the body because it changes the
//! velocity all at once.
//!
//! The camera chasing the player feels the same push (see `camera.rs`): it hangs back as the car
//! pulls away, closes in as it slows, dips as the road sags under it, and shakes when the car hits
//! something.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::race::{CarVisual, RaceUpdate, RaceView};
use crate::screen::Screen;
use crate::track::CurrentTrack;
use crate::world;

/// Earth's, in m/s²: what a car standing still feels, and what its springs carry without moving.
const GRAVITY: f32 = 9.81;

/// The body pitching about the car's width: nose up as it gathers speed, down as it loses it.
const PITCH: Spring = Spring {
    frequency: 1.2,
    damping: 0.35,
    gain: 0.12 * PI / 180.0,
    limit: 6.0 * PI / 180.0,
};
/// The body rolling about the car's length, to the outside of a turn. Heavier since v0.1.44, at
/// the user's word for more weight: slower to lean and to come back (1 Hz, 1.3 before), and
/// further, 9 degrees where 6 was already reached at half lock at 209 km/h.
const ROLL: Spring = Spring {
    frequency: 1.0,
    damping: 0.4,
    gain: 0.1 * PI / 180.0,
    limit: 9.0 * PI / 180.0,
};
/// The body sinking toward the road when it is pressed into it, and rising when it is lightened,
/// in meters. Stiffer than the rest: a banking transition taken off the centerline lifts the car
/// at four or five times gravity, and a softer body would bob through every one of them.
const HEAVE: Spring = Spring {
    frequency: 2.0,
    damping: 0.55,
    gain: 0.005,
    limit: 0.08,
};
/// The body turns about this point of the car, low and in the middle, where its wheels would be.
const PIVOT: Vec3 = Vec3::new(0.0, 0.25, 0.0);

/// A car whose displayed position jumps further than this in a frame, in meters, or whose speed
/// does by more than this, in m/s, was put somewhere else, lined up on a new grid most likely: its
/// body is not thrown about for it.
const TELEPORT_DISTANCE: f32 = 6.0;
const TELEPORT_SPEED: f32 = 45.0;
/// Driving never changes the velocity faster than this, in m/s²: a boost pushes at 35 and the
/// tightest drift turns the car at about 90. Beyond it, the car hit something.
const IMPACT_ACCELERATION: f32 = 150.0;
/// The springs are stepped this finely at most, in seconds, so a slow frame cannot make them blow
/// up.
const MAX_STEP: f32 = 1.0 / 120.0;

pub struct WeightPlugin;

impl Plugin for WeightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FollowedPush>().add_systems(
            Update,
            sway.after(RaceUpdate).run_if(in_state(Screen::Lobby)),
        );
    }
}

/// How the car the camera follows is pushed this frame, for the camera.
#[derive(Resource, Default)]
pub struct FollowedPush(pub Option<Push>);

/// How a car is pushed: its acceleration in the world, and how hard it hit something.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Push {
    /// In m/s², gravity left out.
    pub acceleration: Vec3,
    /// The speed the car lost at once, beyond anything driving does, in m/s. Zero but on a hit.
    pub impact: f32,
}

/// The part of a car that rides on the springs, under the car's place on the road.
#[derive(Component)]
pub struct CarBody;

/// A car's springs, and how it moved last frame.
#[derive(Component, Default)]
pub struct Sway {
    motion: Motion,
    pitch: Oscillator,
    roll: Oscillator,
    heave: Oscillator,
}

impl Sway {
    /// Brings the body to rest, its springs neither stretched nor moving.
    fn rest(&mut self) {
        self.pitch = Oscillator::default();
        self.roll = Oscillator::default();
        self.heave = Oscillator::default();
    }

    /// The car's body, `seconds` after the last frame, pushed by `push`, or by nothing but gravity
    /// when there is nothing to tell. `rotation` is how the car sits on the road.
    fn settle(&mut self, push: Option<Push>, rotation: Quat, seconds: f32) -> Transform {
        let acceleration = push.map_or(Vec3::ZERO, |push| push.acceleration);
        // What the springs feel, in the car's own frame: `+x` ahead, `+y` up from the road and
        // `+z` to the right. Gravity presses the car into the road, and pulls it sideways on a
        // bank and back or forth on a slope.
        let felt = rotation.inverse() * (acceleration + Vec3::Y * GRAVITY);
        // Pushed ahead, the body stays behind: its nose lifts. Pushed to the right, as in a right
        // turn, its top swings left, a negative turn about `+x`. Pressed into the road harder than
        // gravity alone, it sinks.
        self.pitch.step(&PITCH, felt.x, seconds);
        self.roll.step(&ROLL, -felt.z, seconds);
        self.heave.step(&HEAVE, -(felt.y - GRAVITY), seconds);

        let rotation = Quat::from_rotation_z(self.pitch.shown(&PITCH))
            * Quat::from_rotation_x(self.roll.shown(&ROLL));
        Transform::from_translation(PIVOT - rotation * PIVOT + Vec3::Y * self.heave.shown(&HEAVE))
            .with_rotation(rotation)
    }
}

/// How a car moved last frame, to tell how it is pushed in this one.
#[derive(Debug, Default, Clone, Copy)]
struct Motion {
    /// Its position and velocity, in the world.
    last: Option<(Vec3, Vec3)>,
}

impl Motion {
    /// The car is at `position` with `velocity`, both in the world, `seconds` after the last
    /// frame. `None` when there is nothing to tell: on its first frame, and on one that put it
    /// somewhere else.
    fn update(&mut self, position: Vec3, velocity: Vec3, seconds: f32) -> Option<Push> {
        let (last_position, last_velocity) = self.last.replace((position, velocity))?;
        let change = velocity - last_velocity;
        if seconds <= 0.0
            || position.distance(last_position) > TELEPORT_DISTANCE
            || change.length() > TELEPORT_SPEED
        {
            return None;
        }
        let acceleration = change / seconds;
        Some(Push {
            acceleration,
            impact: (acceleration.length() - IMPACT_ACCELERATION).max(0.0) * seconds,
        })
    }
}

/// A spring as heavy as a car's suspension, or a camera's mount: how quickly it swings, how much
/// it is damped, how far an acceleration holds it, and how far it can go.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    /// In hertz. Lower is heavier: slower to swing and to settle.
    pub frequency: f32,
    /// As a share of critical damping: below 1, it overshoots a little before it settles.
    pub damping: f32,
    /// Where a steady acceleration of 1 m/s² holds it, in its own unit.
    pub gain: f32,
    /// How far it can go, approached softly rather than hit.
    pub limit: f32,
}

/// Where a spring stands, and how fast it moves.
#[derive(Debug, Default, Clone, Copy)]
pub struct Oscillator {
    value: f32,
    rate: f32,
}

impl Oscillator {
    /// Lets `spring` swing for `seconds` under an acceleration of `acceleration` m/s². A short,
    /// hard push, a wall's, swings it as far as the speed it changes calls for, however short the
    /// frame that carried it.
    pub fn step(&mut self, spring: &Spring, acceleration: f32, seconds: f32) {
        let omega = TAU * spring.frequency;
        let steps = (seconds / MAX_STEP).ceil().max(1.0);
        let dt = seconds / steps;
        for _ in 0..steps as u32 {
            let pull = omega * omega * (spring.gain * acceleration - self.value)
                - 2.0 * spring.damping * omega * self.rate;
            self.rate += pull * dt;
            self.value += self.rate * dt;
        }
    }

    /// Where it shows: where it stands, eased into its limit.
    pub fn shown(&self, spring: &Spring) -> f32 {
        spring.limit * (self.value / spring.limit).tanh()
    }
}

/// Sways every car's body with how it is pushed, and tells the camera how its car is.
pub(crate) fn sway(
    time: Res<Time>,
    view: Res<RaceView>,
    track: Option<Res<CurrentTrack>>,
    mut followed: ResMut<FollowedPush>,
    mut cars: Query<(&CarVisual, &Transform, &mut Sway, &Children)>,
    mut bodies: Query<&mut Transform, (With<CarBody>, Without<CarVisual>)>,
) {
    let seconds = time.delta_secs();
    followed.0 = None;
    for (visual, place, mut sway, children) in &mut cars {
        let Some(car) = view
            .cars
            .iter()
            .find(|car| car.id == visual.0)
            .map(|snapshot| snapshot.car)
        else {
            continue;
        };
        // Climbing or diving, the car's velocity has a part up or down the road.
        let gradient = track
            .as_ref()
            .map_or(Vec2::ZERO, |track| track.0.surface(car.position).gradient);
        let velocity = world::direction(car.velocity) + Vec3::Y * gradient.dot(car.velocity);
        let push = sway.motion.update(place.translation, velocity, seconds);
        if push.is_none() {
            // A car seen for the first time, or put somewhere else: its body starts at rest.
            sway.rest();
        }
        let body = sway.settle(push, place.rotation, seconds);
        for child in children.iter() {
            if let Ok(mut transform) = bodies.get_mut(child) {
                *transform = body;
            }
        }
        if view.followed.is_some_and(|(id, _)| id == visual.0) {
            followed.0 = push;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    /// Drives a car along `+x` for `seconds` with a world acceleration of `acceleration`, starting
    /// at `speed`, and returns its body as it stands then.
    fn pushed(acceleration: Vec3, speed: f32, seconds: f32) -> (Sway, Transform) {
        let mut sway = Sway::default();
        let mut position = Vec3::ZERO;
        let mut velocity = Vec3::X * speed;
        let mut body = Transform::default();
        for _ in 0..(seconds / FRAME) as usize {
            let push = sway.motion.update(position, velocity, FRAME);
            body = sway.settle(push, Quat::IDENTITY, FRAME);
            velocity += acceleration * FRAME;
            position += velocity * FRAME;
        }
        (sway, body)
    }

    #[test]
    fn gathering_speed_lifts_the_nose_and_it_settles() {
        let (_, body) = pushed(Vec3::X * 20.0, 10.0, 3.0);
        let nose = body.rotation * Vec3::X;
        let settled = PITCH.limit * (PITCH.gain * 20.0 / PITCH.limit).tanh();
        assert!(nose.y > 0.0, "{nose}");
        assert!((nose.y.asin() - settled).abs() < 0.002, "{nose} {settled}");
    }

    /// Turning right, the car is pushed to its right, `+z`, and its body leans out to the left.
    #[test]
    fn a_turn_rolls_the_body_to_the_outside() {
        let (_, body) = pushed(Vec3::Z * 30.0, 30.0, 2.0);
        let top = body.rotation * Vec3::Y;
        assert!(top.z < -0.02, "{top}");
    }

    /// Pressed into the road, as in a banked bowl or at the foot of a dive, the body sinks; standing
    /// still, it stays where it is.
    #[test]
    fn pressed_into_the_road_the_body_sinks() {
        let (_, pressed) = pushed(Vec3::Y * 8.0, 0.0, 2.0);
        assert!(pressed.translation.y < -0.02, "{pressed:?}");
        let (_, still) = pushed(Vec3::ZERO, 0.0, 2.0);
        assert!(still.translation.length() < 1e-4, "{still:?}");
        assert!(still.rotation.angle_between(Quat::IDENTITY) < 1e-4);
    }

    /// The springs never go past their limits, and stay calm at a low frame rate.
    #[test]
    fn the_springs_stay_within_their_limits_at_any_frame_rate() {
        for frame in [1.0 / 300.0, 1.0 / 15.0] {
            let mut oscillator = Oscillator::default();
            for _ in 0..(5.0 / frame) as usize {
                oscillator.step(&ROLL, 500.0, frame);
                assert!(oscillator.shown(&ROLL).abs() <= ROLL.limit);
            }
            oscillator.step(&ROLL, 0.0, 10.0);
            assert!(oscillator.shown(&ROLL).abs() < 1e-3, "{oscillator:?}");
        }
    }

    /// A wall changes the velocity at once, which counts as a hit; driving never does.
    #[test]
    fn a_hit_is_told_apart_from_driving() {
        let mut motion = Motion::default();
        assert_eq!(motion.update(Vec3::ZERO, Vec3::X * 40.0, FRAME), None);
        // A drift as tight as they come, a full second of it.
        let drifting = motion
            .update(Vec3::X * 0.67, Vec3::new(40.0, 0.0, 1.5), FRAME)
            .unwrap();
        assert_eq!(drifting.impact, 0.0, "{drifting:?}");
        // Into a wall at 15 degrees: the speed into it gone in a tick.
        let hit = motion
            .update(Vec3::X * 1.34, Vec3::new(39.0, 0.0, -9.0), FRAME)
            .unwrap();
        assert!(hit.impact > 5.0, "{hit:?}");
    }

    /// A car put on a new grid stands there; it did not crash into it.
    #[test]
    fn a_car_put_somewhere_else_is_not_thrown_about() {
        let mut motion = Motion::default();
        motion.update(Vec3::ZERO, Vec3::X * 40.0, FRAME);
        assert_eq!(motion.update(Vec3::X * 50.0, Vec3::ZERO, FRAME), None);
        assert!(motion.update(Vec3::X * 50.0, Vec3::ZERO, FRAME).is_some());
    }
}
