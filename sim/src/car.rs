//! One car: its state, what the player presses, the tuning, and one simulation step.
//!
//! An arcade model, built for feel rather than realism. There is no brake: the game is full speed
//! all the time, and letting go of the accelerator only coasts. The velocity is kept apart from the
//! heading, so the car can slide: when it grips, sideways speed dies out at a rate set by the grip.
//!
//! **Drift** is the heart of the gameplay, and a driving mode of its own: the car skates. Holding
//! the drift button with the stick turned starts a drift toward that side, locked until the button
//! is let go. While it lasts, the car turns on a
//! radius the stick only tightens or widens, its body turns into the slide while its direction of
//! travel follows closely without losing speed, and it builds up a charge. Letting go releases the
//! charge as a boost, and the body settles back without costing speed. Touching a wall ends the
//! drift and loses the charge.

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
    /// Radians counter-clockwise from `+x`, in `[-π, π)`. Where the nose points, which is no
    /// longer where the car travels once it slides (see [`Car::slip`]).
    pub heading: f32,
    /// The angle between where the car travels and where its nose points, in radians, positive
    /// when the nose is to the left of the travel. This is the slide itself and not a drawing of
    /// it: the rotation puts the nose ahead of the velocity, and the grip pulls the velocity back
    /// toward it. Going far enough off this axis starts a drift, and coming back into it ends one.
    pub slip: f32,
    /// Radians per second, positive to the left.
    pub yaw_rate: f32,
    /// The side the car drifts toward: -1 left, 1 right, 0 when not drifting.
    pub drift: i8,
    /// What the current drift has built up: seconds of drift, counted faster in tight drifts.
    pub drift_charge: f32,
    /// Seconds of boost left.
    pub boost: f32,
}

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
    /// Pull down a banked road, in m/s² (9.81 is Earth's). It draws the car toward the inside of a
    /// banked turn; zero makes banking purely visual.
    pub gravity: f32,

    /// Slowest speed a drift starts at or keeps going at, in m/s.
    pub drift_min_speed: f32,
    /// How far off the axis of travel the car must come, in degrees, for its slide to become a
    /// drift. Nothing but steering hard reaches it: the rotation puts the nose ahead of the
    /// velocity, and past this angle the car is drifting.
    pub drift_entry_angle: f32,
    /// How far back into the axis the car must come, in degrees, for a drift to end. Below the
    /// entry angle, so a drift does not flicker on and off around one threshold. Holding the drift
    /// button keeps a drift alive however straight the car runs.
    pub drift_exit_angle: f32,
    /// Turning radius while drifting with the stick fully into the drift, in meters.
    pub drift_radius_tight: f32,
    /// How quickly the direction of travel catches up with the heading while drifting, per second.
    /// Lower slides wider.
    pub drift_grip: f32,
    /// How quickly the rotation follows the stick while drifting, per second. Higher than
    /// `steering_response`, so a drift answers the stick at once and does not carry the car wide
    /// while its rotation builds up.
    pub drift_steering_response: f32,
    /// What holding the drift button multiplies the drift radius by, from 0 to 1: the button
    /// tightens the turn beyond what the stick alone gives.
    pub drift_button_tighten: f32,
    /// Extra deceleration while the drift button is held, in m/s². Tightening costs speed.
    pub drift_button_drag: f32,
    /// Deceleration when a drift begins, in m/s²: what a drift costs before its boost pays it back.
    pub drift_drag: f32,
    /// How much more the drift drags for every second of charge, in m/s² per second: holding a
    /// drift brakes the car harder and harder, so a drift has to be let go of.
    pub drift_drag_ramp: f32,
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
            ("drift_grip", self.drift_grip),
            ("drift_button_drag", self.drift_button_drag),
            ("drift_drag", self.drift_drag),
            ("drift_drag_ramp", self.drift_drag_ramp),
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
        // A drift that ended no earlier than it started would flicker on and off around one angle.
        if self.drift_exit_angle >= self.drift_entry_angle {
            return Err(format!(
                "drift_exit_angle must be below drift_entry_angle, not {} against {}",
                self.drift_exit_angle, self.drift_entry_angle
            ));
        }
        if self.drift_entry_angle >= 90.0 {
            return Err(format!(
                "drift_entry_angle must be below 90 degrees, not {}",
                self.drift_entry_angle
            ));
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

    /// Where the body points. The nose leads and the car travels off to one side of it while it
    /// slides (see [`Car::slip`]), so the body is simply the heading.
    pub fn body_heading(&self) -> f32 {
        self.heading
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

        let forward = self.forward();
        // In a drift the travel swings toward the nose without losing any speed, so a slide
        // carries its speed through a turn. `drift_grip` is what decides the feel: the lower it
        // is, the longer the car stays off its axis, the more it unscrews into the turn, and the
        // wider it runs while doing so. That width is what a wide road buys.
        if self.is_drifting() && self.velocity.length_squared() > 1e-6 {
            let swing = self.velocity.angle_to(forward) * (1.0 - decay(tuning.drift_grip, dt));
            self.velocity = Vec2::from_angle(swing).rotate(self.velocity);
        }

        // Split the velocity along the heading. When gripping, whatever the rotation left sideways
        // slides and dies out; a slide has already turned it.
        let right = -forward.perp();
        let speed = self.velocity.length();
        let forward_speed = self.drive(self.velocity.dot(forward), speed, input, tuning, dt);
        let mut sideways_speed = self.velocity.dot(right);
        if !self.is_drifting() {
            sideways_speed *= decay(tuning.grip, dt);
        }
        self.velocity = forward * forward_speed + right * sideways_speed;
        self.position += self.velocity * dt;

        let hit_wall = self.collide_with_walls(tuning, track, dt);
        // The slide is read back from the velocity this tick leaves behind, so the drift always
        // decides from what the car actually did, walls included.
        self.slip = if self.velocity.length_squared() > 1e-6 {
            self.velocity.angle_to(self.forward())
        } else {
            0.0
        };
        if hit_wall && self.is_drifting() {
            // A drift into a wall is lost, and pays nothing.
            self.drift = 0;
            self.drift_charge = 0.0;
        }
    }

    /// Starts, holds or ends the drift, from how far off its axis of travel the car has come.
    ///
    /// **No button starts a drift.** Turning hard enough does: the rotation puts the nose ahead of
    /// the velocity, and past `drift_entry_angle` the car is drifting. The button is not needed to
    /// hold one either, only to tighten it (see [`Car::steer`]), which costs speed. Where the
    /// button does decide is the end: coming back into the axis ends a drift, unless the button is
    /// held at that moment, which carries the drift down a straight and into the next turn.
    fn update_drift(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        self.boost = (self.boost - dt).max(0.0);

        let speed = self.velocity.length();
        let off_axis = self.slip.abs();
        if self.is_drifting() {
            let back_in_line = off_axis < tuning.drift_exit_angle.to_radians();
            if speed < tuning.drift_min_speed || (back_in_line && !input.drift) {
                self.end_drift(tuning);
            } else {
                // Tighter drifts build up faster: from half to one and a half times real time.
                // Steering out of the drift charges at the slow end, never backwards.
                self.drift_charge += dt * (0.5 + self.drift_stick(input).max(0.0));
            }
        } else if speed >= tuning.drift_min_speed
            && off_axis >= tuning.drift_entry_angle.to_radians()
        {
            // A nose to the left of the travel is a car coming round to the left, side -1.
            self.drift = if self.slip > 0.0 { -1 } else { 1 };
            self.drift_charge = 0.0;
        }
    }

    /// Ends the drift and pays out what it built up.
    fn end_drift(&mut self, tuning: &CarTuning) {
        self.boost = self.boost.max(self.pending_boost(tuning));
        self.drift = 0;
        self.drift_charge = 0.0;
    }

    /// How far the stick points into the drift, from -1 fully against it to 1 fully into it, and
    /// 0 centered. The sign matters: centered means straight ahead, and against the drift means
    /// steering out of it, which is how a slide is caught.
    fn drift_stick(&self, input: CarInput) -> f32 {
        input.steer() * f32::from(self.drift)
    }

    fn steer(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        let steering_speed = self.forward_speed().max(tuning.min_steering_speed);
        // Steering right turns clockwise, a negative yaw rate.
        let target_yaw_rate = if self.is_drifting() {
            // A drift keeps turning toward its side; the stick only tightens or widens it.
            // The stick sets a curvature, and it is signed: centered runs straight, into the
            // drift turns on `drift_radius_tight`, and against it steers out of the slide. That
            // sign is the whole of the control a drift gives, and what lets one end: straighten,
            // the travel catches the nose up, and the car is back in its axis.
            let stick = self.drift_stick(input);
            let mut curvature = stick / tuning.drift_radius_tight;
            if input.drift {
                // Holding the button pulls the drift tighter than the stick alone can.
                curvature /= tuning.drift_button_tighten;
            }
            -f32::from(self.drift) * steering_speed * curvature
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
        self.heading = wrap_angle(self.heading + self.yaw_rate * dt);
    }

    /// New forward speed after one tick of boost, accelerator or coasting, and drift drag.
    /// `speed` is the whole speed, sliding included, which is what the top speeds limit.
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
        if self.is_drifting() {
            // The longer the drift is held, the harder it brakes, and tightening it with the
            // button costs more still.
            let mut drag = tuning.drift_drag + tuning.drift_drag_ramp * self.drift_charge;
            if input.drift {
                drag += tuning.drift_button_drag;
            }
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

    /// Angle between where the body points and where the car goes.
    fn slip(car: &Car) -> f32 {
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

    /// Drives at full lock until the car has come far enough off its axis to be drifting.
    fn into_a_drift(car: &mut Car, steer: f32, track: &Track) {
        for _ in 0..ticks(3.0) {
            car.step(CarInput::new(true, steer), &tuning(), track);
            if car.is_drifting() {
                return;
            }
        }
        panic!("the car never came off its axis");
    }

    /// Nothing but turning hard starts a drift: no button, and gentle steering will not do.
    #[test]
    fn a_drift_starts_when_the_car_comes_off_its_axis() {
        let track = Track::build(&open_track()).unwrap();

        let mut car = at_top_speed(&track);
        run(&mut car, CarInput::new(true, 0.3), 1.5, &track);
        assert!(!car.is_drifting(), "gentle, {} rad off axis", car.slip);

        let mut car = at_top_speed(&track);
        run(&mut car, CarInput::new(true, 1.0), 1.5, &track);
        assert!(car.is_drifting(), "full lock, {} rad off axis", car.slip);

        let mut car = Car::new(Vec2::ZERO, 0.0);
        run(&mut car, CarInput::new(true, 1.0), 0.2, &track);
        assert!(!car.is_drifting(), "too slow");

        // The side is the way the car has come round, which the button has no say in.
        for (steer, side) in [(1.0, 1), (-1.0, -1)] {
            let mut car = at_top_speed(&track);
            into_a_drift(&mut car, steer, &track);
            assert_eq!(car.drift, side);
        }
    }

    #[test]
    fn a_drift_turns_further_and_slides_more_than_gripping() {
        let track = Track::build(&open_track()).unwrap();
        let start = at_top_speed(&track);

        let mut gripping = start;
        run(&mut gripping, CarInput::new(true, 0.3), 1.5, &track);
        let mut sliding = start;
        run(&mut sliding, CarInput::new(true, 1.0), 1.5, &track);

        assert!(sliding.is_drifting() && !gripping.is_drifting());
        // Both turn right, clockwise; the drift turns further and sits further off its axis.
        assert!(
            -sliding.heading > -gripping.heading,
            "{} {}",
            sliding.heading,
            gripping.heading
        );
        assert!(
            slip(&sliding) > slip(&gripping),
            "{} {}",
            slip(&sliding),
            slip(&gripping)
        );
    }

    /// The button is never needed, but it earns its place: the same stick turns tighter with it
    /// held, and pays for it in speed.
    #[test]
    fn the_button_tightens_a_drift_and_costs_speed() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        into_a_drift(&mut car, 1.0, &track);

        let mut loose = car;
        let mut tight = car;
        run(&mut loose, CarInput::new(true, 1.0), 0.4, &track);
        run(&mut tight, drifting(1.0), 0.4, &track);

        assert!(loose.is_drifting() && tight.is_drifting());
        assert!(
            -tight.heading > -loose.heading,
            "{} {}",
            tight.heading,
            loose.heading
        );
        assert!(
            tight.velocity.length() < loose.velocity.length(),
            "{} {}",
            tight.velocity.length(),
            loose.velocity.length()
        );
    }

    /// The drift ends when the car comes back into line with where it travels, which the stick
    /// does by straightening the car. Holding the button through that moment keeps it alive, so a
    /// drift can be carried down a straight and into the next turn.
    #[test]
    fn coming_back_into_the_axis_ends_a_drift_unless_the_button_is_held() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        into_a_drift(&mut car, 1.0, &track);
        // Held into the turn for a while, so there is a charge worth paying out.
        run(&mut car, CarInput::new(true, 1.0), 1.5, &track);

        // A centered stick runs the car straight, so the travel catches the nose up. The boost is
        // checked on the tick the drift ends, since it starts running down from that moment.
        let mut released = car;
        let mut left = ticks(3.0);
        while released.is_drifting() {
            released.step(ACCELERATE, &tuning(), &track);
            left -= 1;
            assert!(
                left > 0,
                "never came back into line, {} rad off",
                released.slip
            );
        }
        assert!(released.boost > 0.0, "the drift paid out on the way out");

        let mut held = car;
        run(&mut held, ACCELERATE.with_drift(true), 2.0, &track);
        assert!(held.is_drifting(), "{} rad off axis", held.slip);
    }

    /// The car runs wide while it is off its axis, which is the point, but not without limit: a
    /// quarter turn must stay within the width of a road built for drifting.
    #[test]
    fn a_drift_runs_wide_but_not_without_limit() {
        let track = Track::build(&open_track()).unwrap();
        let start = at_top_speed(&track);
        let mut car = start;
        let mut ticks_left = ticks(6.0);
        // Until the direction of travel has turned a quarter turn to the right.
        while start.velocity.angle_to(car.velocity) > -std::f32::consts::FRAC_PI_2 {
            car.step(drifting(1.0), &tuning(), &track);
            ticks_left -= 1;
            assert!(ticks_left > 0, "never turned");
        }
        let ahead = (car.position - start.position).dot(start.forward());
        let radius = tuning().drift_radius_tight;
        println!("a quarter turn {ahead} m ahead, on a {radius} m radius");
        assert!(ahead < radius + 30.0, "{ahead}");
    }

    /// Holding a drift brakes harder and harder, so it has to be let go of rather than held for
    /// ever.
    #[test]
    fn a_drift_brakes_harder_the_longer_it_is_held() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        into_a_drift(&mut car, 1.0, &track);

        let start = car.velocity.length();
        run(&mut car, drifting(1.0), 1.0, &track);
        let after_one = car.velocity.length();
        run(&mut car, drifting(1.0), 1.0, &track);
        let after_two = car.velocity.length();

        let first = start - after_one;
        let second = after_one - after_two;
        assert!(car.is_drifting(), "still drifting");
        assert!(second > first, "{first} then {second}");
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
        into_a_drift(&mut car, 1.0, &track);
        assert!(car.is_drifting());
        // Straightened again at once: too little charge to be worth anything.
        car.drift_charge = 0.0;
        run(&mut car, ACCELERATE, 2.0, &track);
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

        run(&mut car, ACCELERATE, 0.1, &track);
        assert_eq!(car.boost, 0.0);
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
        // A drift that ended no earlier than it started would flicker around one angle.
        let mut broken = tuning();
        broken.drift_exit_angle = broken.drift_entry_angle;
        assert!(broken.validate().is_err());
        let mut broken = tuning();
        broken.drift_button_tighten = 1.5;
        assert!(broken.validate().is_err());
        assert!(tuning().validate().is_ok());
    }
}
