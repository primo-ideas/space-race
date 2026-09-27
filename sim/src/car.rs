//! One car: its state, what the player presses, the tuning, and one simulation step.
//!
//! An arcade model, built for feel rather than realism. There is no brake: the game is full speed
//! all the time, and letting go of the accelerator only coasts. The velocity is kept apart from the
//! heading, so the car can slide: when it grips, sideways speed dies out at a rate set by the grip.
//!
//! **Drift** is the heart of the gameplay, in the spirit of Rocket Racing, and a driving mode of its
//! own: the car skates. A tap of the drift button with the stick turned throws the car into a drift
//! toward that side, and so does turning hard enough to break away. While it lasts, the stick sets
//! the curve the car travels on, and the body swings far across it: that angle is drawn, and it is
//! what ends the drift, but it never carries the car off its line. Nothing has to be held and the
//! drift costs no speed; holding the button tightens the curve, at a cost that grows the longer it
//! is held. Bringing the body back into line ends the drift and pays out what it built up as a
//! boost, unless the button is held. Touching a wall loses it.

use std::f32::consts::{PI, TAU};

use bitcode::{Decode, Encode};
use glam::Vec2;
use serde::Deserialize;

use super::TICK_SECONDS;
use super::track::Track;

#[derive(Debug, Clone, Copy, Default, PartialEq, Encode, Decode)]
pub struct Car {
    pub position: Vec2,
    /// Meters per second. Not necessarily along the heading: the car can slide.
    pub velocity: Vec2,
    /// Radians counter-clockwise from `+x`, in `[-π, π)`. What the car steers: while it grips,
    /// the nose, which the travel follows at the rate of the grip; while it drifts, the direction
    /// of travel itself, which the stick turns, the body being swung off it by [`Car::body`].
    pub heading: f32,
    /// The angle between where the car travels and its heading, in radians, positive when the
    /// heading is to the left of the travel: the slide of a car that grips, which the rotation puts
    /// ahead of the velocity and the grip pulls back. Turning hard enough to take it past
    /// `drift_entry_angle` starts a drift. Nothing while the car drifts, its heading being its
    /// travel.
    pub slip: f32,
    /// How far the body is swung off the heading, in radians, positive to the left: the drift
    /// angle. It is what is drawn and what ends a drift, but it moves the car nowhere. It settles
    /// back to nothing once a drift is over.
    pub body: f32,
    /// Radians per second, positive to the left.
    pub yaw_rate: f32,
    /// The side the car drifts toward: -1 left, 1 right, 0 when not drifting.
    pub drift: i8,
    /// What the current drift has built up: seconds of drift, counted faster in tight drifts.
    pub drift_charge: f32,
    /// Seconds the drift button has been held without a break during this drift: holding it costs
    /// more the longer it lasts.
    pub drift_held: f32,
    /// Whether the drift button was down on the last tick, so a press is told apart from a hold.
    pub drift_button: bool,
    /// Seconds of boost left.
    pub boost: f32,
}

/// A press of the drift button starts nothing unless the stick points at least this far to one
/// side: a drift needs a side.
const DRIFT_PRESS_STICK: f32 = 0.2;

/// With the drift button held, the stick pointing this far against the drift carries it over to
/// the other side, charge and all: how a drift is chained through an S.
const DRIFT_SWITCH_STICK: f32 = 0.5;

/// What a player presses, quantized: compact on the wire, and the same number on every machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Encode, Decode)]
pub struct CarInput {
    pub accelerate: bool,
    /// Held to drift, with the stick turned toward the side to drift to.
    pub drift: bool,
    /// From -127 (full left) to 127 (full right).
    pub steer: i8,
}

impl CarInput {
    /// `steer` in `[-1, 1]`, right positive. The drift button is not pressed.
    pub fn new(accelerate: bool, steer: f32) -> Self {
        Self {
            accelerate,
            drift: false,
            steer: (steer.clamp(-1.0, 1.0) * 127.0).round() as i8,
        }
    }

    pub fn with_drift(self, drift: bool) -> Self {
        Self { drift, ..self }
    }

    /// In `[-1, 1]`, right positive. A peer may send -128, which counts as full left.
    pub fn steer(self) -> f32 {
        (f32::from(self.steer) / 127.0).max(-1.0)
    }
}

/// How a car drives. Loaded from a file by the server, so the feel can be tuned without
/// recompiling.
#[derive(Debug, Clone, PartialEq, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub struct CarTuning {
    /// Speed reached while accelerating, in m/s.
    pub top_speed: f32,
    /// Acceleration from standstill, in m/s². It fades linearly to zero at top speed.
    pub acceleration: f32,
    /// Deceleration when not accelerating, and back down to top speed after a boost, in m/s².
    pub coasting: f32,
    /// Turning radius at full lock when slow, in meters.
    pub turn_radius_slow: f32,
    /// Turning radius at full lock at top speed. In between, the radius follows the speed
    /// linearly. Wider at speed keeps the car controllable.
    pub turn_radius_fast: f32,
    /// The car steers at least as if it were going this fast, in m/s. Without a brake there is no
    /// reverse, so this is what lets a car stopped against a wall turn away from it.
    pub min_steering_speed: f32,
    /// How quickly the rotation follows the steering, per second. Higher is snappier.
    pub steering_response: f32,
    /// How quickly sliding sideways dies out, per second, when not drifting. Higher feels like
    /// rails.
    pub grip: f32,
    /// Pull down the slopes of the road, in m/s² (9.81 is Earth's). It slows a car climbing, speeds
    /// one diving and draws one toward the inside of a banked turn; zero makes the relief and the
    /// banking purely visual.
    pub gravity: f32,

    /// Slowest speed a drift starts at or keeps going at, in m/s.
    pub drift_min_speed: f32,
    /// How far off its axis of travel a gripping car must come, in degrees, for its slide to become
    /// a drift: turning hard enough breaks the car away without the button. A tap of the button
    /// does it at once.
    pub drift_entry_angle: f32,
    /// How far back into line the body must come, in degrees, with the stick keeping it there, for
    /// a drift to end. Holding the drift button keeps a drift alive however straight the car runs.
    pub drift_exit_angle: f32,
    /// How far the body swings off the travel in a drift, in degrees, with the stick fully into the
    /// drift. Most of it comes with the first touch of the stick: the angle follows the square
    /// root of how far the stick points into the drift, and there is none with the stick centered
    /// or against the drift.
    pub drift_angle: f32,
    /// How quickly the body swings toward that angle, and back into line, per second.
    pub drift_angle_response: f32,
    /// Radius of the curve the car travels on while drifting with the stick fully into the drift,
    /// in meters. The stick sets the curve directly: the car does not run wide.
    pub drift_radius_tight: f32,
    /// How quickly the curve follows the stick while drifting, per second. Higher than
    /// `steering_response`, so a drift answers the stick at once.
    pub drift_steering_response: f32,
    /// What holding the drift button multiplies the drift radius by, from 0 to 1: the button
    /// tightens the turn beyond what the stick alone gives.
    pub drift_button_tighten: f32,
    /// Deceleration while the drift button is held in a drift, in m/s², from the moment it is
    /// pressed. Tightening costs speed; a drift that holds nothing costs none.
    pub drift_button_drag: f32,
    /// How much more holding the button drags for every second it is held, in m/s² per second: a
    /// drift tightened for long brakes harder and harder, so the button has to be let go of.
    pub drift_button_drag_ramp: f32,
    /// Charge a drift needs before it gives any boost, in seconds.
    pub drift_min_charge: f32,
    /// Seconds of boost per second of charge beyond the minimum.
    pub drift_boost_rate: f32,
    /// Longest boost a single drift can give, in seconds.
    pub drift_max_boost: f32,
    /// Top speed while boosting, in m/s. Above `top_speed`.
    pub boost_top_speed: f32,
    /// Acceleration while boosting, in m/s². It pushes in full up to `top_speed`, then fades to zero
    /// at `boost_top_speed`, so a boost is felt even at top speed. A boost pushes whether the
    /// accelerator is pressed or not.
    pub boost_acceleration: f32,
    /// Seconds of boost for a perfect start: pressing the accelerator right at the start. Lesser
    /// starts get a share of it (see [`StartGrade`](super::race::StartGrade)).
    pub start_boost: f32,

    /// Radius of the circle the car collides as, in meters.
    pub collision_radius: f32,
    /// Share of the speed into a wall that bounces back, from 0 to 1.
    pub wall_bounce: f32,
    /// How quickly scraping along a wall slows the car, per second.
    pub wall_friction: f32,
}

impl CarTuning {
    /// Rejects values that would break the simulation, such as a zero radius dividing by zero.
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            ("top_speed", self.top_speed),
            ("acceleration", self.acceleration),
            ("turn_radius_slow", self.turn_radius_slow),
            ("turn_radius_fast", self.turn_radius_fast),
            ("steering_response", self.steering_response),
            ("drift_entry_angle", self.drift_entry_angle),
            ("drift_exit_angle", self.drift_exit_angle),
            ("drift_angle", self.drift_angle),
            ("drift_angle_response", self.drift_angle_response),
            ("drift_button_tighten", self.drift_button_tighten),
            ("drift_radius_tight", self.drift_radius_tight),
            ("drift_steering_response", self.drift_steering_response),
            ("boost_top_speed", self.boost_top_speed),
            ("boost_acceleration", self.boost_acceleration),
            ("collision_radius", self.collision_radius),
        ];
        let non_negative = [
            ("coasting", self.coasting),
            ("min_steering_speed", self.min_steering_speed),
            ("grip", self.grip),
            ("gravity", self.gravity),
            ("drift_min_speed", self.drift_min_speed),
            ("drift_button_drag", self.drift_button_drag),
            ("drift_button_drag_ramp", self.drift_button_drag_ramp),
            ("drift_min_charge", self.drift_min_charge),
            ("drift_boost_rate", self.drift_boost_rate),
            ("drift_max_boost", self.drift_max_boost),
            ("start_boost", self.start_boost),
            ("wall_friction", self.wall_friction),
        ];
        for (name, value) in positive {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!("{name} must be a positive number, not {value}"));
            }
        }
        for (name, value) in non_negative {
            if !(value.is_finite() && value >= 0.0) {
                return Err(format!(
                    "{name} must be zero or a positive number, not {value}"
                ));
            }
        }
        if self.drift_button_tighten > 1.0 {
            return Err(format!(
                "drift_button_tighten must be at most 1, not {}",
                self.drift_button_tighten
            ));
        }
        // A body that never swung past the exit angle would end every drift as it began.
        if self.drift_exit_angle >= self.drift_angle {
            return Err(format!(
                "drift_exit_angle must be below drift_angle, not {} against {}",
                self.drift_exit_angle, self.drift_angle
            ));
        }
        for (name, value) in [
            ("drift_entry_angle", self.drift_entry_angle),
            ("drift_angle", self.drift_angle),
        ] {
            if value >= 90.0 {
                return Err(format!("{name} must be below 90 degrees, not {value}"));
            }
        }
        if self.boost_top_speed <= self.top_speed {
            return Err(format!(
                "boost_top_speed must be above top_speed, not {}",
                self.boost_top_speed
            ));
        }
        if !(0.0..=1.0).contains(&self.wall_bounce) {
            return Err(format!(
                "wall_bounce must be between 0 and 1, not {}",
                self.wall_bounce
            ));
        }
        Ok(())
    }
}

impl Car {
    /// A car standing still.
    pub fn new(position: Vec2, heading: f32) -> Self {
        Self {
            position,
            heading,
            ..Self::default()
        }
    }

    /// A car standing still on a starting grid slot.
    pub fn on_grid(track: &Track, slot: usize) -> Self {
        let (position, heading) = track.grid_slot(slot);
        Self::new(position, heading)
    }

    pub fn forward(&self) -> Vec2 {
        Vec2::from_angle(self.heading)
    }

    /// Speed along the heading. Negative only briefly, after bouncing off a wall.
    pub fn forward_speed(&self) -> f32 {
        self.velocity.dot(self.forward())
    }

    pub fn is_drifting(&self) -> bool {
        self.drift != 0
    }

    /// Where the body points: the heading, swung across by the drift angle (see [`Car::body`]).
    pub fn body_heading(&self) -> f32 {
        wrap_angle(self.heading + self.body)
    }

    /// Seconds of boost that letting go of the drift would give now.
    pub fn pending_boost(&self, tuning: &CarTuning) -> f32 {
        if !self.is_drifting() {
            return 0.0;
        }
        ((self.drift_charge - tuning.drift_min_charge) * tuning.drift_boost_rate)
            .clamp(0.0, tuning.drift_max_boost)
    }

    /// Advances the car by one tick.
    pub fn step(&mut self, input: CarInput, tuning: &CarTuning, track: &Track) {
        let dt = TICK_SECONDS;
        self.update_drift(input, tuning, dt);
        self.steer(input, tuning, dt);

        // On a slope, gravity pulls downhill. For a body resting on a height field, the horizontal
        // part of that pull is `g × gradient / (1 + |gradient|²)`. It comes before the grip, which
        // then holds against it the way tires would.
        let surface = track.surface(self.position);
        let pull = tuning.gravity / (1.0 + surface.gradient.length_squared());
        self.velocity -= surface.gradient * pull * dt;

        if self.is_drifting() {
            self.skate(input, tuning, dt);
        } else {
            self.grip(input, tuning, dt);
        }
        self.position += self.velocity * dt;

        let hit_wall = self.collide_with_walls(tuning, track, dt);
        if hit_wall && self.is_drifting() {
            // A drift into a wall is lost, and pays nothing. The body settles back into line on
            // its own, as after any drift.
            self.drift = 0;
            self.drift_charge = 0.0;
            self.drift_held = 0.0;
        }
        let moving = self.velocity.length_squared() > 1e-6;
        if self.is_drifting() && moving {
            // A drift steers the travel itself, walls included.
            self.heading = self.velocity.to_angle();
        }
        // The slide is read back from the velocity this tick leaves behind, so a drift always
        // starts from what the car actually did.
        self.slip = if moving {
            self.velocity.angle_to(self.forward())
        } else {
            0.0
        };
        let swing = 1.0 - decay(tuning.drift_angle_response, dt);
        self.body += (self.body_target(input, tuning) - self.body) * swing;
        self.drift_button = input.drift;
    }

    /// Starts, holds, carries over or ends the drift.
    ///
    /// A **press** of the drift button with the stick turned starts one toward the stick, and so
    /// does turning hard enough to take the slide past `drift_entry_angle` without it. Nothing has
    /// to be held afterward. The drift ends when the body is back in line with the stick keeping
    /// it there, centered or against the drift, unless the button is held at that moment: held,
    /// it carries the drift down a straight, and with the stick well against the drift it carries
    /// it over to the other side, charge and all.
    fn update_drift(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        self.boost = (self.boost - dt).max(0.0);

        let speed = self.velocity.length();
        if self.is_drifting() {
            if input.drift && self.drift_stick(input) < -DRIFT_SWITCH_STICK {
                self.drift = -self.drift;
            }
            let exit = tuning.drift_exit_angle.to_radians();
            let in_line = self.body.abs() < exit && self.body_target(input, tuning).abs() < exit;
            if speed < tuning.drift_min_speed || (in_line && !input.drift) {
                self.end_drift(tuning);
            } else {
                // Tighter drifts build up faster: from half to one and a half times real time.
                // Steering out of the drift charges at the slow end, never backwards.
                self.drift_charge += dt * (0.5 + self.drift_stick(input).max(0.0));
                self.drift_held = if input.drift {
                    self.drift_held + dt
                } else {
                    0.0
                };
            }
        } else if speed >= tuning.drift_min_speed {
            let pressed = input.drift && !self.drift_button;
            let side = if pressed && input.steer().abs() >= DRIFT_PRESS_STICK {
                // Steering right is a drift to the right, side 1.
                input.steer().signum() as i8
            } else if self.slip.abs() >= tuning.drift_entry_angle.to_radians() {
                // A nose to the left of the travel is a car coming round to the left, side -1.
                if self.slip > 0.0 { -1 } else { 1 }
            } else {
                0
            };
            if side != 0 {
                self.start_drift(side);
            }
        }
    }

    /// Starts a drift toward `side`. The heading becomes the travel, and the body takes over
    /// whatever slide the car had, so nothing on screen jumps.
    fn start_drift(&mut self, side: i8) {
        self.drift = side;
        self.drift_charge = 0.0;
        self.drift_held = 0.0;
        if self.velocity.length_squared() > 1e-6 {
            self.body += self.slip;
            self.heading = self.velocity.to_angle();
            self.slip = 0.0;
        }
    }

    /// Ends the drift and pays out what it built up.
    fn end_drift(&mut self, tuning: &CarTuning) {
        self.boost = self.boost.max(self.pending_boost(tuning));
        self.drift = 0;
        self.drift_charge = 0.0;
        self.drift_held = 0.0;
    }

    /// How far the stick points into the drift, from -1 fully against it to 1 fully into it, and
    /// 0 centered. The sign matters: centered means straight ahead, and against the drift means
    /// steering out of it, which is how a slide is caught.
    fn drift_stick(&self, input: CarInput) -> f32 {
        input.steer() * f32::from(self.drift)
    }

    /// The angle the body swings toward: across the travel, into the drift, as far as the stick
    /// points into it, most of it with the first touch; back in line otherwise.
    fn body_target(&self, input: CarInput, tuning: &CarTuning) -> f32 {
        if !self.is_drifting() {
            return 0.0;
        }
        let into = self.drift_stick(input).max(0.0);
        // A drift to the left, side -1, swings the nose to the left: a positive angle.
        -f32::from(self.drift) * tuning.drift_angle.to_radians() * into.sqrt()
    }

    fn steer(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        let steering_speed = self.forward_speed().max(tuning.min_steering_speed);
        // Steering right turns clockwise, a negative yaw rate.
        let target_yaw_rate = if self.is_drifting() {
            // The stick sets the curve the car travels on, and it is signed: centered runs
            // straight, fully to one side turns on `drift_radius_tight`, whichever side the drift
            // is on. The body swinging across it changes nothing about where the car goes.
            let mut curvature = input.steer() / tuning.drift_radius_tight;
            if input.drift {
                // Holding the button pulls the drift tighter than the stick alone can.
                curvature /= tuning.drift_button_tighten;
            }
            -steering_speed * curvature
        } else {
            let speed_share = (steering_speed / tuning.top_speed).min(1.0);
            let turn_radius = tuning.turn_radius_slow
                + (tuning.turn_radius_fast - tuning.turn_radius_slow) * speed_share;
            -input.steer() * steering_speed / turn_radius
        };
        let response = if self.is_drifting() {
            tuning.drift_steering_response
        } else {
            tuning.steering_response
        };
        self.yaw_rate += (target_yaw_rate - self.yaw_rate) * (1.0 - decay(response, dt));
    }

    /// A tick of gripping: the rotation turns the nose, the velocity is split along it, and
    /// whatever the rotation left sideways slides and dies out at the rate of the grip.
    fn grip(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        self.heading = wrap_angle(self.heading + self.yaw_rate * dt);
        let forward = self.forward();
        let right = -forward.perp();
        let speed = self.velocity.length();
        let forward_speed = self.drive(self.velocity.dot(forward), speed, input, tuning, dt);
        let sideways_speed = self.velocity.dot(right) * decay(tuning.grip, dt);
        self.velocity = forward * forward_speed + right * sideways_speed;
    }

    /// A tick of drifting: the rotation turns the travel itself, which keeps all of its speed, so
    /// the car goes where the stick says whatever the body does.
    fn skate(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        let speed = self.velocity.length();
        let travel = if speed > 1e-3 {
            self.velocity.to_angle()
        } else {
            self.heading
        };
        self.heading = wrap_angle(travel + self.yaw_rate * dt);
        let speed = self.drive(speed, speed, input, tuning, dt);
        self.velocity = self.forward() * speed;
    }

    /// New forward speed after one tick of boost, accelerator or coasting, and of holding the
    /// drift button. `speed` is the whole speed, sliding included, which is what the top speeds
    /// limit.
    fn drive(
        &self,
        forward_speed: f32,
        speed: f32,
        input: CarInput,
        tuning: &CarTuning,
        dt: f32,
    ) -> f32 {
        let forward_speed = if self.boost > 0.0 {
            // Full push up to top speed, fading out toward the boost top speed.
            let fade = ((tuning.boost_top_speed - speed)
                / (tuning.boost_top_speed - tuning.top_speed))
                .clamp(0.0, 1.0);
            forward_speed + tuning.boost_acceleration * fade * dt
        } else if input.accelerate && speed > tuning.top_speed {
            // Faster than the engine alone goes, after a boost: settle back to top speed.
            forward_speed - (speed - tuning.top_speed).min(tuning.coasting * dt)
        } else if input.accelerate {
            let fade = (1.0 - speed / tuning.top_speed).max(0.0);
            forward_speed + tuning.acceleration * fade * dt
        } else {
            move_towards(forward_speed, 0.0, tuning.coasting * dt)
        };
        if self.is_drifting() && input.drift {
            // A drift costs nothing; tightening it with the button does, and the longer the
            // button is held, the harder it brakes.
            let drag = tuning.drift_button_drag + tuning.drift_button_drag_ramp * self.drift_held;
            move_towards(forward_speed, 0.0, drag * dt)
        } else {
            forward_speed
        }
    }

    /// Pushes the car back onto the road. Returns whether it touched a wall.
    ///
    /// The walls are read where the car is, not from the track as a whole: through a bottleneck
    /// they stand closer in, and the road itself squeezes the car toward the centerline.
    fn collide_with_walls(&mut self, tuning: &CarTuning, track: &Track, dt: f32) -> bool {
        let projection = track.project(self.position);
        let limit = track.half_width_at(projection.distance) - tuning.collision_radius;
        let overlap = projection.lateral.abs() - limit;
        if overlap <= 0.0 {
            return false;
        }

        let outward = (self.position - projection.closest).normalize_or_zero();
        self.position -= outward * overlap;

        let into_wall = self.velocity.dot(outward);
        if into_wall > 0.0 {
            self.velocity -= outward * into_wall * (1.0 + tuning.wall_bounce);
        }
        let along_wall = self.velocity - outward * self.velocity.dot(outward);
        self.velocity -= along_wall * (1.0 - decay(tuning.wall_friction, dt));
        true
    }
}

/// Factor that an exponential decay at `rate` per second leaves after `dt` seconds.
fn decay(rate: f32, dt: f32) -> f32 {
    (-rate * dt).exp()
}

fn move_towards(value: f32, target: f32, max_delta: f32) -> f32 {
    if (target - value).abs() <= max_delta {
        target
    } else {
        value + (target - value).signum() * max_delta
    }
}

fn wrap_angle(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TICK_RATE;
    use crate::test_support::{banked_oval, open_track, oval, tuning};
    use crate::track::Elevation;

    /// A car standing still in the middle of the first banked turn's arc, on the centerline, drifts
    /// down toward the inside; on flat road it stays put.
    #[test]
    fn a_banked_turn_pulls_a_standing_car_toward_the_inside() {
        for (banking, should_move) in [(30.0, true), (0.0, false)] {
            let track = Track::build(&banked_oval(100.0, 40.0, 20.0, banking)).unwrap();
            let arc_middle = 50.0 + 20.0 + (std::f32::consts::PI * 40.0 - 20.0) / 2.0;
            let point = track.point_at(arc_middle);
            let mut car = Car::new(point.position, point.direction.to_angle());
            run(&mut car, CarInput::default(), 3.0, &track);

            // The turn goes left, so the inside is on the left: positive lateral.
            let lateral = track.project(car.position).lateral;
            if should_move {
                assert!(lateral > 0.3, "{banking}: {lateral}");
            } else {
                assert!(lateral.abs() < 1e-4, "{banking}: {lateral}");
            }
        }
    }

    /// A long straight, so a car can accelerate without reaching a turn.
    fn long_track() -> Track {
        Track::build(&oval(2000.0, 60.0, 0.0)).unwrap()
    }

    /// Climbing costs speed and diving gives it: gravity pulls along the road as well as across it.
    #[test]
    fn a_climb_slows_the_car_and_a_descent_speeds_it_up() {
        // The first kilometer after the start line climbs `rise` meters, and the rest of the lap
        // comes back down.
        let sloped = |rise: f32| {
            let mut description = oval(2000.0, 60.0, 0.0);
            description.elevation = vec![
                Elevation {
                    at: 0.0,
                    height: 0.0,
                },
                Elevation {
                    at: 1000.0,
                    height: rise,
                },
            ];
            Track::build(&description).unwrap()
        };
        let speed_after = |track: &Track| {
            let mut car = Car::new(Vec2::ZERO, 0.0);
            run(&mut car, ACCELERATE, 3.0, track);
            car.forward_speed()
        };
        let flat = speed_after(&long_track());
        let climbing = speed_after(&sloped(60.0));
        let diving = speed_after(&sloped(-60.0));
        assert!(
            climbing < flat - 0.5 && diving > flat + 0.5,
            "{climbing} {flat} {diving}"
        );
    }

    fn run(car: &mut Car, input: CarInput, seconds: f32, track: &Track) {
        for _ in 0..ticks(seconds) {
            car.step(input, &tuning(), track);
        }
    }

    fn ticks(seconds: f32) -> usize {
        (seconds * TICK_RATE as f32) as usize
    }

    const ACCELERATE: CarInput = CarInput {
        accelerate: true,
        drift: false,
        steer: 0,
    };

    fn drifting(steer: f32) -> CarInput {
        CarInput::new(true, steer).with_drift(true)
    }

    /// A car at top speed in the middle of a track too wide to meet a wall.
    fn at_top_speed(track: &Track) -> Car {
        let mut car = Car::new(Vec2::ZERO, 0.0);
        run(&mut car, ACCELERATE, 12.0, track);
        car
    }

    /// Angle between where the body points and where the car goes: the drift angle as it is seen.
    fn body_angle(car: &Car) -> f32 {
        car.velocity
            .angle_to(Vec2::from_angle(car.body_heading()))
            .abs()
    }

    #[test]
    fn accelerating_approaches_top_speed() {
        let track = long_track();
        let mut car = Car::on_grid(&track, 0);
        run(&mut car, ACCELERATE, 1.0, &track);
        let after_one_second = car.forward_speed();
        run(&mut car, ACCELERATE, 14.0, &track);

        assert!(after_one_second > 5.0, "{after_one_second}");
        let top_speed = tuning().top_speed;
        assert!(car.forward_speed() > 0.95 * top_speed && car.forward_speed() <= top_speed);
        assert!(
            car.velocity.y.abs() < 1e-3,
            "drove straight: {:?}",
            car.velocity
        );
    }

    #[test]
    fn letting_go_coasts_to_a_stop() {
        let track = long_track();
        let mut car = Car::on_grid(&track, 0);
        run(&mut car, ACCELERATE, 2.0, &track);
        run(&mut car, CarInput::default(), 30.0, &track);
        assert!(car.velocity.length() < 1e-3, "{:?}", car.velocity);
    }

    #[test]
    fn steering_right_turns_clockwise() {
        let track = long_track();
        let right = CarInput {
            steer: 127,
            ..ACCELERATE
        };
        let mut car = Car::on_grid(&track, 0);
        run(&mut car, ACCELERATE, 1.0, &track);
        run(&mut car, right, 0.3, &track);
        assert!(car.heading < -0.05, "{}", car.heading);
    }

    /// Without a brake there is no reverse: a car stopped facing a wall must still turn away.
    #[test]
    fn a_car_stopped_against_a_wall_can_turn_away() {
        let track = long_track();
        let mut car = Car::on_grid(&track, 0);
        // Facing the left wall, touching it.
        car.heading = std::f32::consts::FRAC_PI_2;
        car.position.y = track.half_width() - tuning().collision_radius;
        run(&mut car, ACCELERATE, 1.0, &track);
        assert!(car.forward_speed() < 1.0, "pushed into the wall");

        let right = CarInput {
            steer: 127,
            ..ACCELERATE
        };
        run(&mut car, right, 3.0, &track);
        assert!(car.heading < 1.0, "{}", car.heading);
        assert!(car.forward_speed() > 5.0, "{}", car.forward_speed());
    }

    /// A car running along the wall is squeezed toward the middle of the road by a bottleneck: the
    /// walls it meets are the ones beside it, not the width of the track as a whole.
    #[test]
    fn a_bottleneck_squeezes_the_car_toward_the_middle() {
        let mut description = oval(2000.0, 60.0, 0.0);
        description.narrows = vec![crate::track::Narrows {
            at: 300.0,
            length: 60.0,
            width: 9.0,
            blend: 40.0,
        }];
        let track = Track::build(&description).unwrap();

        // Along the left wall, on the straight that leads into the bottleneck.
        let entry = track.point_at(150.0);
        let lateral = track.half_width() - tuning().collision_radius;
        let mut car = Car::new(
            entry.position + entry.direction.perp() * lateral,
            entry.direction.to_angle(),
        );
        for _ in 0..ticks(12.0) {
            car.step(ACCELERATE, &tuning(), &track);
            let here = track.project(car.position);
            let room = track.half_width_at(here.distance) - tuning().collision_radius;
            assert!(here.lateral.abs() <= room + 1e-3, "{here:?}");
        }
        let through = track.project(car.position);
        assert!(
            through.distance > 330.0,
            "the car never reached it: {through:?}"
        );
        assert!(through.lateral < 4.5, "still out wide: {through:?}");
    }

    #[test]
    fn walls_keep_the_car_on_the_road() {
        let track = long_track();
        let mut car = Car::on_grid(&track, 0);
        let hard_left = CarInput {
            steer: -127,
            ..ACCELERATE
        };
        let limit = track.half_width() - tuning().collision_radius;
        for _ in 0..(10 * TICK_RATE) {
            car.step(hard_left, &tuning(), &track);
            let lateral = track.project(car.position).lateral;
            assert!(lateral.abs() <= limit + 1e-3, "{lateral}");
        }
    }

    /// One tick with the drift button pressed and the stick at `steer`, which must start a drift.
    fn press_into_a_drift(car: &mut Car, steer: f32, track: &Track) {
        car.step(drifting(steer), &tuning(), track);
        assert!(car.is_drifting(), "the press started nothing");
    }

    /// A tap of the button with the stick turned throws the car into a drift at once, toward the
    /// stick. A press with the stick centered has no side to drift to, and a car too slow does not
    /// drift at all.
    #[test]
    fn a_press_of_the_button_throws_the_car_into_a_drift() {
        let track = Track::build(&open_track()).unwrap();
        for (steer, side) in [(0.6, 1), (-0.6, -1)] {
            let mut car = at_top_speed(&track);
            car.step(drifting(steer), &tuning(), &track);
            assert_eq!(car.drift, side);
        }

        let mut car = at_top_speed(&track);
        car.step(drifting(0.0), &tuning(), &track);
        assert!(!car.is_drifting(), "no side");

        let mut car = Car::new(Vec2::ZERO, 0.0);
        run(&mut car, ACCELERATE, 0.3, &track);
        car.step(drifting(1.0), &tuning(), &track);
        assert!(!car.is_drifting(), "too slow");
    }

    /// Turning hard enough breaks the car into a drift without the button; gentle steering does
    /// not, and the side is the way the car came round.
    #[test]
    fn turning_hard_breaks_into_a_drift_without_the_button() {
        let track = Track::build(&open_track()).unwrap();

        let mut car = at_top_speed(&track);
        run(&mut car, CarInput::new(true, 0.3), 1.5, &track);
        assert!(!car.is_drifting(), "gentle, {} rad off axis", car.slip);

        for (steer, side) in [(1.0, 1), (-1.0, -1)] {
            let mut car = at_top_speed(&track);
            run(&mut car, CarInput::new(true, steer), 1.5, &track);
            assert_eq!(car.drift, side, "full lock, {} rad off axis", car.slip);
        }
    }

    /// The body swings far across the travel, most of the way in a third of a second, and more
    /// with the stick further into the drift.
    #[test]
    fn the_body_swings_far_across_the_travel_at_once() {
        let track = Track::build(&open_track()).unwrap();
        let full = tuning().drift_angle.to_radians();
        let mut angles = Vec::new();
        for steer in [1.0, 0.5] {
            let mut car = at_top_speed(&track);
            press_into_a_drift(&mut car, steer, &track);
            run(&mut car, CarInput::new(true, steer), 0.35, &track);
            assert!(car.is_drifting());
            angles.push(body_angle(&car));
        }
        assert!(
            angles[0] > 0.8 * full && angles[0] <= full,
            "{} rad",
            angles[0]
        );
        assert!(
            angles[1] < angles[0] && angles[1] > 0.5 * full,
            "{angles:?}"
        );
    }

    /// However far the body swings, the car travels on the curve the stick asks for: a quarter
    /// turn takes it no further ahead than the radius and the time the curve takes to tighten.
    #[test]
    fn a_drift_goes_where_the_stick_says_without_running_wide() {
        let track = Track::build(&open_track()).unwrap();
        let start = at_top_speed(&track);
        let mut car = start;
        press_into_a_drift(&mut car, 1.0, &track);
        let mut ticks_left = ticks(6.0);
        // Until the direction of travel has turned a quarter turn to the right.
        while start.velocity.angle_to(car.velocity) > -std::f32::consts::FRAC_PI_2 {
            car.step(CarInput::new(true, 1.0), &tuning(), &track);
            ticks_left -= 1;
            assert!(ticks_left > 0, "never turned");
        }
        let ahead = (car.position - start.position).dot(start.forward());
        let radius = tuning().drift_radius_tight;
        println!("a quarter turn {ahead} m ahead, on a {radius} m radius");
        assert!(ahead < radius + 6.0, "{ahead}");
        assert!(body_angle(&car) > 0.8 * tuning().drift_angle.to_radians());
    }

    /// A drift costs nothing: the car keeps its speed however long it lasts. Holding the button
    /// tightens it and brakes, harder and harder, so the button has to be let go of.
    #[test]
    fn a_drift_keeps_its_speed_and_only_the_button_brakes_it() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        let top = car.velocity.length();
        press_into_a_drift(&mut car, 1.0, &track);
        run(&mut car, CarInput::new(true, 1.0), 2.0, &track);
        assert!(car.is_drifting());
        assert!(
            car.velocity.length() > top - 0.1,
            "{}",
            car.velocity.length()
        );

        let mut loose = car;
        let mut tight = car;
        run(&mut loose, CarInput::new(true, 1.0), 0.4, &track);
        run(&mut tight, drifting(1.0), 0.4, &track);
        // The same stick turns the travel further with the button held, and slower.
        let turned = |after: &Car| car.velocity.angle_to(after.velocity).abs();
        assert!(
            turned(&tight) > turned(&loose),
            "{} {}",
            turned(&tight),
            turned(&loose)
        );
        assert!(tight.velocity.length() < loose.velocity.length() - 0.5);

        let start = tight.velocity.length();
        run(&mut tight, drifting(1.0), 1.0, &track);
        let after_one = tight.velocity.length();
        run(&mut tight, drifting(1.0), 1.0, &track);
        let after_two = tight.velocity.length();
        assert!(tight.is_drifting(), "still drifting");
        assert!(
            after_one - after_two > start - after_one,
            "{start} {after_one} {after_two}"
        );
    }

    /// Straightening the stick brings the body back into line, which ends the drift and pays out
    /// its charge. Held through that moment, the button keeps the drift alive, so it can be carried
    /// down a straight and into the next turn.
    #[test]
    fn coming_back_into_line_ends_a_drift_unless_the_button_is_held() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        press_into_a_drift(&mut car, 1.0, &track);
        // Held into the turn for a while, so there is a charge worth paying out.
        run(&mut car, CarInput::new(true, 1.0), 1.5, &track);

        // The boost is checked on the tick the drift ends, since it runs down from that moment.
        let mut released = car;
        let mut left = ticks(0.5);
        while released.is_drifting() {
            released.step(ACCELERATE, &tuning(), &track);
            left -= 1;
            assert!(left > 0, "never came back into line, {} rad", released.body);
        }
        assert!(released.boost > 0.0, "the drift paid out on the way out");

        let mut held = car;
        run(&mut held, ACCELERATE.with_drift(true), 2.0, &track);
        assert!(held.is_drifting(), "{} rad", held.body);
    }

    /// With the button held, the stick well against the drift carries it over to the other side,
    /// charge and all: an S is one drift.
    #[test]
    fn a_held_button_carries_a_drift_over_to_the_other_side() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        press_into_a_drift(&mut car, 1.0, &track);
        run(&mut car, CarInput::new(true, 1.0), 1.0, &track);
        let charge = car.drift_charge;

        run(&mut car, drifting(-1.0), 0.5, &track);
        assert_eq!(car.drift, -1);
        assert!(car.drift_charge > charge, "{} {charge}", car.drift_charge);
        // Swung across to the left: the nose to the left of the travel.
        assert!(
            car.body > 0.5 * tuning().drift_angle.to_radians(),
            "{}",
            car.body
        );
    }

    #[test]
    fn a_boost_pushes_hard_even_at_top_speed() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        car.boost = 0.5;
        run(&mut car, ACCELERATE, 0.5, &track);
        let (top_speed, boost_top_speed) = (tuning().top_speed, tuning().boost_top_speed);
        let speed = car.velocity.length();
        assert!(
            speed > top_speed + 0.5 * (boost_top_speed - top_speed),
            "{speed}"
        );
    }

    #[test]
    fn a_short_drift_gives_no_boost() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        press_into_a_drift(&mut car, 1.0, &track);
        // Straightened again at once: too little charge to be worth anything.
        run(&mut car, ACCELERATE, 1.0, &track);
        assert!(!car.is_drifting());
        assert_eq!(car.boost, 0.0);
    }

    #[test]
    fn a_drift_into_a_wall_is_lost_until_the_button_is_pressed_again() {
        let track = long_track();
        let mut car = Car::on_grid(&track, 0);
        run(&mut car, ACCELERATE, 5.0, &track);

        let mut drifted = false;
        let mut hit = false;
        for _ in 0..ticks(3.0) {
            let before = car.is_drifting();
            car.step(drifting(-1.0), &tuning(), &track);
            drifted |= car.is_drifting();
            hit |= before && !car.is_drifting();
        }
        assert!(drifted && hit, "{drifted} {hit}");
        assert!(
            !car.is_drifting(),
            "the held button does not start another drift"
        );

        run(&mut car, ACCELERATE, 0.5, &track);
        assert_eq!(car.boost, 0.0);
        // The body settles back into line on its own.
        assert!(car.body.abs() < 0.05, "{}", car.body);
    }

    #[test]
    fn input_quantization_round_trips() {
        assert_eq!(CarInput::new(true, -1.0).steer(), -1.0);
        assert!((CarInput::new(true, 0.5).steer() - 0.5).abs() < 0.01);
        assert_eq!(CarInput::new(false, 3.0).steer, 127);
        assert!(CarInput::new(true, 0.0).with_drift(true).drift);
        let hostile = CarInput {
            steer: i8::MIN,
            ..CarInput::default()
        };
        assert_eq!(hostile.steer(), -1.0);
    }

    #[test]
    fn invalid_tuning_is_refused() {
        let mut broken = tuning();
        broken.turn_radius_fast = 0.0;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.wall_bounce = f32::NAN;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.min_steering_speed = -1.0;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.drift_entry_angle = 0.0;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.boost_top_speed = broken.top_speed;
        assert!(broken.validate().is_err());
        // A body that never swung past the exit angle would end every drift as it began.
        let mut broken = tuning();
        broken.drift_exit_angle = broken.drift_angle;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.drift_angle = 95.0;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.drift_button_tighten = 1.5;
        assert!(broken.validate().is_err());
        assert!(tuning().validate().is_ok());
    }
}
