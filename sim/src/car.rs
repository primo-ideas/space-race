//! One car: its state, what the player presses, the tuning, and one simulation step.
//!
//! An arcade model, built for feel rather than realism. There is no brake: the game is full speed
//! all the time, and letting go of the accelerator only coasts. The velocity is kept apart from the
//! heading, so the car can slide: when it grips, sideways speed dies out at a rate set by the grip.
//!
//! The car is **heavy**: its rotation gathers and dies away steadily rather than at once, it is
//! carried a little wide of where its nose points while it grips, it keeps its momentum above top
//! speed, and a wall takes speed along it as well as into it.
//!
//! **Drift** is the heart of the gameplay, in the spirit of Rocket Racing, and a driving mode of its
//! own: the car skates. A tap of the drift button with the stick turned throws the car into a drift
//! toward that side, and so does turning hard enough to break away. While it lasts, the stick sets
//! the curve the car travels on, and the body swings far across it: that angle is drawn, and it is
//! what ends the drift, but it never carries the car off its line. Nothing has to be held and the
//! drift costs no speed; holding the button tightens the curve, at a cost that grows the longer it
//! is held. Bringing the body back into line ends the drift and pays out what it built up as a
//! boost, unless the button is held. Hitting a wall loses it; grazing one does not.
//!
//! Drifting also fills the **turbo** meter, which holds up to [`TURBO_CHARGES`] charges. A press of
//! the turbo button spends one on a boost stronger and longer than a drift's.

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
    /// ahead of the velocity and the grip pulls back. It is what carries a gripping car wide of
    /// where it points, and the grip keeps it small. Nothing while the car drifts, its heading
    /// being its travel.
    pub slip: f32,
    /// How far the body is swung off the heading, in radians, positive to the left: the nose
    /// unscrewing into a turn while the car grips, and the drift angle once it has broken away. It
    /// is what is drawn, what starts a drift when it goes far enough and what ends one, but it
    /// moves the car nowhere. It settles back to nothing on a straight.
    pub body: f32,
    /// How fast the body swings, in radians per second, positive to the left. The swing has a
    /// speed of its own so that it starts and stops gently instead of all at once.
    pub body_rate: f32,
    /// Where the steering stands, from -1 at full lock left to 1 at full lock right. It follows the
    /// stick at `steering_rate`: it moves on the very tick the stick does, and eases in as it gets
    /// there, so a key pressed or a stick flicked turns the car at once but gently.
    pub steering: f32,
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
    /// Seconds a press of the drift button is still remembered for: a press made a moment before
    /// the stick is turned, or before the car is fast enough, still starts its drift.
    pub drift_press: f32,
    /// The turbo meter, in charges, from 0 to [`TURBO_CHARGES`]. Drifting fills it.
    pub turbo: f32,
    /// Seconds left of the turbo being fired.
    pub turbo_boost: f32,
    /// Whether the turbo button was down on the last tick, so a press is told apart from a hold.
    pub turbo_button: bool,
}

/// How many charges the turbo meter holds.
pub const TURBO_CHARGES: f32 = 3.0;

/// A press of the drift button starts nothing unless the stick points at least this far to one
/// side: a drift needs a side.
const DRIFT_PRESS_STICK: f32 = 0.2;

/// With the drift button held, the stick pointing this far against the drift carries it over to
/// the other side, charge and all: how a drift is chained through an S.
const DRIFT_SWITCH_STICK: f32 = 0.5;

/// A car going into a wall slower than this, in m/s, is running along it rather than into it.
const WALL_BRUSH: f32 = 0.01;

/// What a player presses, quantized: compact on the wire, and the same number on every machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Encode, Decode)]
pub struct CarInput {
    pub accelerate: bool,
    /// Held to drift, with the stick turned toward the side to drift to.
    pub drift: bool,
    /// From -127 (full left) to 127 (full right).
    pub steer: i8,
    /// Pressed to fire a charge of the turbo.
    pub turbo: bool,
}

impl CarInput {
    /// `steer` in `[-1, 1]`, right positive. The drift button is not pressed.
    pub fn new(accelerate: bool, steer: f32) -> Self {
        Self {
            accelerate,
            drift: false,
            steer: (steer.clamp(-1.0, 1.0) * 127.0).round() as i8,
            turbo: false,
        }
    }

    pub fn with_drift(self, drift: bool) -> Self {
        Self { drift, ..self }
    }

    pub fn with_turbo(self, turbo: bool) -> Self {
        Self { turbo, ..self }
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
    /// Deceleration when not accelerating, in m/s².
    pub coasting: f32,
    /// How quickly speed above `top_speed` bleeds away, per second: what a boost or a dive gave
    /// beyond it fades by this share of itself every second, a lot of it quickly and the last of
    /// it slowly. Low keeps the momentum, so a dive carries the car past top speed and a boost
    /// lingers; zero keeps it for good.
    pub overspeed_drag: f32,
    /// Turning radius at full lock when slow, in meters.
    pub turn_radius_slow: f32,
    /// Turning radius at full lock at top speed. In between, the radius follows the speed
    /// linearly. Wider at speed keeps the car controllable.
    pub turn_radius_fast: f32,
    /// The car steers at least as if it were going this fast, in m/s. Without a brake there is no
    /// reverse, so this is what lets a car stopped against a wall turn away from it.
    pub min_steering_speed: f32,
    /// How quickly the steering follows the stick, per second. It closes that share of the gap per
    /// second, most of it at first: it moves on the very tick the stick does, so there is no delay
    /// before the car turns, and eases in as it gets there, so it turns gently at first and a
    /// little more as the steering comes round. Most of the way in `2 / steering_rate` seconds.
    pub steering_rate: f32,
    /// How quickly the rotation follows the steering, per second. Well above `steering_rate`, so
    /// the rotation follows the steering almost at once, and only smooths what the steering does
    /// not: the change of radius when a drift starts or ends, or when the drift button tightens it.
    pub steering_response: f32,
    /// The fastest the car's rotation can change, in degrees per second per second: its weight.
    /// The rotation starts on the very tick the steering does and gathers speed steadily, and
    /// once the stick lets go it dies away the same way, the car carrying on round a little. A
    /// small correction never meets it; a full turn of the wheel, a drift carried over to the
    /// other side or a slalom does. While drifting, it is the rotation of the travel.
    pub yaw_acceleration: f32,
    /// How quickly sliding sideways dies out, per second, when not drifting. Higher feels like
    /// rails.
    pub grip: f32,
    /// Pull down the slopes of the road, in m/s² (9.81 is Earth's). It slows a car climbing, speeds
    /// one diving and draws one toward the inside of a banked turn; zero makes the relief and the
    /// banking purely visual.
    pub gravity: f32,

    /// Slowest speed a drift starts at or keeps going at, in m/s.
    pub drift_min_speed: f32,
    /// How far a gripping car's nose must unscrew into a turn, in degrees, for it to break into a
    /// drift: turning hard enough does it without the button (see `grip_swing_angle`). A tap of
    /// the button does it at once.
    pub drift_entry_angle: f32,
    /// How far back into line the body must come, in degrees, with the stick keeping it there, for
    /// a drift to end. Holding the drift button keeps a drift alive however straight the car runs.
    pub drift_exit_angle: f32,
    /// How far the nose unscrews into a turn while the car grips, in degrees, at full lock at top
    /// speed: the body swings off the travel in proportion to how hard the car turns, tail out,
    /// without carrying the car anywhere. Past `drift_entry_angle`, it has broken into a drift.
    pub grip_swing_angle: f32,
    /// How far the body swings off the travel in a drift, in degrees, with the steering fully into
    /// the drift. Most of it comes with the first half of the steering's travel, easing in from
    /// the center: with the steering a share `s` of the way into the drift, the angle is
    /// `1 - (1 - s)²` of this. There is none with the steering centered or against the drift.
    pub drift_angle: f32,
    /// How quickly the body's swing gathers speed, in degrees per second per second. The body comes
    /// off the travel on the very tick a drift starts, slowly, and swings faster and faster, rather
    /// than lunging round at once; it slows down the same way as it reaches its angle, and swings
    /// back into line the same way.
    pub drift_swing_acceleration: f32,
    /// The fastest the body swings, in degrees per second.
    pub drift_swing_rate: f32,
    /// How far across the travel, in degrees, the body swings at full rate. Beyond it the slide
    /// grows less directly: the body swings further out no faster than `drift_swing_knee_rate`
    /// times `drift_swing_rate`, reaching that slower rate as smoothly as any change of its swing.
    /// Swinging back into line is never held back.
    pub drift_swing_knee: f32,
    /// The share of `drift_swing_rate` the body swings out at beyond `drift_swing_knee`, from 0
    /// (it stops there) to 1 (no knee at all).
    pub drift_swing_knee_rate: f32,
    /// Radius of the curve the car travels on while drifting with the stick fully into the drift
    /// and the body swung fully out, in meters. The stick sets the curve directly: the car does not
    /// run wide. The curve tightens from the gripping car's to this one as the body swings out.
    pub drift_radius_tight: f32,
    /// How quickly the curve follows the steering while drifting, per second: like
    /// `steering_response`, well above `steering_rate`, so the curve follows the steering almost
    /// at once and only the button's tightening is smoothed.
    pub drift_steering_response: f32,
    /// What holding the drift button multiplies the drift radius by, from 0 to 1: the button
    /// tightens the turn beyond what the stick alone gives.
    pub drift_button_tighten: f32,
    /// Seconds the drift button must stay held for its tightening to take full effect. It comes in
    /// steadily from the press, so the tap that starts a drift barely tightens it.
    pub drift_button_tighten_time: f32,
    /// Deceleration while the drift button is held in a drift, in m/s², from the moment it is
    /// pressed. Tightening costs speed; a drift that holds nothing costs none.
    pub drift_button_drag: f32,
    /// How much more holding the button drags for every second it is held, in m/s² per second: a
    /// drift tightened for long brakes harder and harder, so the button has to be let go of.
    pub drift_button_drag_ramp: f32,
    /// Seconds a press of the drift button is remembered for. A thumb often lands on the button a
    /// moment before the stick has left the center; without this, that press started nothing.
    pub drift_press_buffer: f32,
    /// The hardest a drifting car can meet a wall and keep its drift, in m/s into the wall: a
    /// graze is forgiven, though it charges nothing while it lasts; a hit loses the drift.
    pub drift_wall_tolerance: f32,
    /// Charge a drift needs before it gives any boost, in seconds.
    pub drift_min_charge: f32,
    /// Seconds of boost per second of charge beyond the minimum.
    pub drift_boost_rate: f32,
    /// Longest boost a single drift can give, in seconds.
    pub drift_max_boost: f32,
    /// Seconds of boost a speed pad gives, counted from when the car leaves it (see
    /// [`crate::track::Pad`]).
    pub pad_boost: f32,
    /// Top speed while boosting, in m/s. Above `top_speed`.
    pub boost_top_speed: f32,
    /// Acceleration while boosting, in m/s². It pushes in full up to `top_speed`, then fades to zero
    /// at `boost_top_speed`, so a boost is felt even at top speed. A boost pushes whether the
    /// accelerator is pressed or not.
    pub boost_acceleration: f32,
    /// Seconds of boost for a perfect start: pressing the accelerator right at the start. Lesser
    /// starts get a share of it (see [`StartGrade`](super::race::StartGrade)).
    pub start_boost: f32,
    /// Charges of turbo a second of drift charge fills: at 0.2, a charge of turbo for every five
    /// seconds of charge, which the tightest drifts build in a little over three.
    pub turbo_fill: f32,
    /// Seconds a charge of turbo boosts for.
    pub turbo_seconds: f32,
    /// Top speed while the turbo fires, in m/s. Above `top_speed`.
    pub turbo_top_speed: f32,
    /// Acceleration while the turbo fires, in m/s², fading out toward `turbo_top_speed` the way a
    /// boost's does.
    pub turbo_acceleration: f32,

    /// Radius of the circle the car collides as, in meters.
    pub collision_radius: f32,
    /// Share of the speed into a wall that bounces back, from 0 to 1.
    pub wall_bounce: f32,
    /// Speed along the wall that a hit takes away, per m/s of the hit: the friction of the
    /// impact. A graze costs little and a blow at an angle a lot, and the car, slow to get back up
    /// to speed, pays for it a while.
    pub wall_impact_friction: f32,
    /// How quickly scraping along a wall slows the car, per second.
    pub wall_friction: f32,
    /// How quickly a wall turns a car that runs into it along itself, per second: the wall
    /// deflects the car, which comes away from it instead of grinding on with its nose in. High,
    /// so it is done in a few ticks, each of which is another hit; the body swings round after
    /// it in its own time. Zero leaves the car pointing where it did.
    pub wall_alignment: f32,
}

impl CarTuning {
    /// Rejects values that would break the simulation, such as a zero radius dividing by zero.
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            ("top_speed", self.top_speed),
            ("acceleration", self.acceleration),
            ("turn_radius_slow", self.turn_radius_slow),
            ("turn_radius_fast", self.turn_radius_fast),
            ("steering_rate", self.steering_rate),
            ("steering_response", self.steering_response),
            ("yaw_acceleration", self.yaw_acceleration),
            ("drift_entry_angle", self.drift_entry_angle),
            ("drift_exit_angle", self.drift_exit_angle),
            ("grip_swing_angle", self.grip_swing_angle),
            ("drift_angle", self.drift_angle),
            ("drift_swing_acceleration", self.drift_swing_acceleration),
            ("drift_swing_rate", self.drift_swing_rate),
            ("drift_swing_knee", self.drift_swing_knee),
            ("drift_button_tighten", self.drift_button_tighten),
            ("drift_button_tighten_time", self.drift_button_tighten_time),
            ("drift_radius_tight", self.drift_radius_tight),
            ("drift_steering_response", self.drift_steering_response),
            ("boost_top_speed", self.boost_top_speed),
            ("boost_acceleration", self.boost_acceleration),
            ("turbo_seconds", self.turbo_seconds),
            ("turbo_top_speed", self.turbo_top_speed),
            ("turbo_acceleration", self.turbo_acceleration),
            ("collision_radius", self.collision_radius),
        ];
        let non_negative = [
            ("coasting", self.coasting),
            ("overspeed_drag", self.overspeed_drag),
            ("min_steering_speed", self.min_steering_speed),
            ("grip", self.grip),
            ("gravity", self.gravity),
            ("drift_min_speed", self.drift_min_speed),
            ("drift_button_drag", self.drift_button_drag),
            ("drift_button_drag_ramp", self.drift_button_drag_ramp),
            ("drift_min_charge", self.drift_min_charge),
            ("drift_boost_rate", self.drift_boost_rate),
            ("drift_max_boost", self.drift_max_boost),
            ("pad_boost", self.pad_boost),
            ("start_boost", self.start_boost),
            ("wall_impact_friction", self.wall_impact_friction),
            ("wall_friction", self.wall_friction),
            ("wall_alignment", self.wall_alignment),
            ("drift_press_buffer", self.drift_press_buffer),
            ("drift_wall_tolerance", self.drift_wall_tolerance),
            ("turbo_fill", self.turbo_fill),
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
        if !(0.0..=1.0).contains(&self.drift_swing_knee_rate) {
            return Err(format!(
                "drift_swing_knee_rate must be between 0 and 1, not {}",
                self.drift_swing_knee_rate
            ));
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
        if self.turbo_top_speed <= self.top_speed {
            return Err(format!(
                "turbo_top_speed must be above top_speed, not {}",
                self.turbo_top_speed
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

    /// Whether a boost is pushing the car, a drift's, a start's or the turbo's.
    pub fn is_boosting(&self) -> bool {
        self.boost > 0.0 || self.turbo_boost > 0.0
    }

    /// Advances the car by one tick.
    pub fn step(&mut self, input: CarInput, tuning: &CarTuning, track: &Track) {
        let dt = TICK_SECONDS;
        let (charge, turbo) = (self.drift_charge, self.turbo);
        self.fire_turbo(input, tuning, dt);
        // The steering moves on the very tick the stick does, and eases in as it gets there.
        self.steering += (input.steer() - self.steering) * (1.0 - decay(tuning.steering_rate, dt));
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
        // A speed pad under the car boosts it, as long as it is on it, and for `pad_boost` after.
        if !track.pads().is_empty() && track.on_pad(&track.project(self.position)) {
            self.boost = self.boost.max(tuning.pad_boost);
        }

        let hit = self.collide_with_walls(tuning, track, dt);
        let moving = self.velocity.length_squared() > 1e-6;
        if let Some(impact) = hit
            && self.is_drifting()
        {
            if impact > tuning.drift_wall_tolerance {
                // A drift into a wall is lost, and pays nothing. The body settles back into line
                // on its own, as after any drift.
                self.drift = 0;
                self.drift_charge = 0.0;
                self.drift_held = 0.0;
            } else {
                // A graze is forgiven, but a drift along a wall builds nothing up.
                self.drift_charge = self.drift_charge.min(charge);
                self.turbo = self.turbo.min(turbo);
            }
        }
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
        self.swing(tuning, dt);
        self.drift_button = input.drift;
        self.turbo_button = input.turbo;
    }

    /// A press of the turbo button spends a charge of the meter, if there is a whole one, on
    /// `turbo_seconds` of the strongest boost there is. A press while one is firing does nothing.
    fn fire_turbo(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        self.turbo_boost = (self.turbo_boost - dt).max(0.0);
        let pressed = input.turbo && !self.turbo_button;
        if pressed && self.turbo >= 1.0 && self.turbo_boost <= 0.0 {
            self.turbo -= 1.0;
            self.turbo_boost = tuning.turbo_seconds;
        }
    }

    /// Swings the body toward its angle. Its speed changes by no more than
    /// `drift_swing_acceleration`, so a swing starts on the very tick it is asked for, slowly, and
    /// gathers speed steadily, up to `drift_swing_rate`; it slows down the same way to stop on its
    /// angle. A spring, tried first, pulled hardest at the start: the body barely moved for a few
    /// ticks, then lunged round.
    fn swing(&mut self, tuning: &CarTuning, dt: f32) {
        let gap = self.body_target(tuning) - self.body;
        let acceleration = tuning.drift_swing_acceleration.to_radians();
        // As fast as it can go and still stop on the angle, slowing down by a tick's worth of
        // acceleration every tick, and no faster than reaches it this tick, so it settles there
        // instead of hunting around it.
        let braking = acceleration * dt / 2.0;
        // Past the knee, a body swinging further out goes no faster than its share of the rate:
        // the slide keeps growing, less directly. Its speed still changes only at `acceleration`,
        // so the knee is taken smoothly.
        let outward = gap * self.body > 0.0;
        let past_knee = self.body.abs() >= tuning.drift_swing_knee.to_radians();
        let fastest = if outward && past_knee {
            tuning.drift_swing_rate * tuning.drift_swing_knee_rate
        } else {
            tuning.drift_swing_rate
        };
        let wanted = ((braking * braking + 2.0 * acceleration * gap.abs()).sqrt() - braking)
            .min(fastest.to_radians())
            .min(gap.abs() / dt);
        self.body_rate = move_towards(self.body_rate, wanted * gap.signum(), acceleration * dt);
        self.body += self.body_rate * dt;
    }

    /// Starts, holds, carries over or ends the drift.
    ///
    /// A **press** of the drift button with the stick turned starts one toward the stick, and so
    /// does turning hard enough to unscrew the nose past `drift_entry_angle` without it. Nothing has
    /// to be held afterward. The drift ends when the body is back in line with the stick keeping
    /// it there, centered or against the drift, unless the button is held at that moment: held,
    /// it carries the drift down a straight, and with the stick well against the drift it carries
    /// it over to the other side, charge and all.
    fn update_drift(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        self.boost = (self.boost - dt).max(0.0);
        // A press is remembered for a moment, so one made just before the stick is turned still
        // starts its drift.
        let pressed = input.drift && !self.drift_button;
        self.drift_press = if pressed {
            tuning.drift_press_buffer
        } else {
            (self.drift_press - dt).max(0.0)
        };

        let speed = self.velocity.length();
        if self.is_drifting() {
            self.drift_press = 0.0;
            // Decided on the stick itself, which says what the player means before the steering
            // has followed it.
            if input.drift && input.steer() * f32::from(self.drift) < -DRIFT_SWITCH_STICK {
                self.drift = -self.drift;
            }
            let exit = tuning.drift_exit_angle.to_radians();
            let in_line = self.body.abs() < exit && self.body_target(tuning).abs() < exit;
            if speed < tuning.drift_min_speed || (in_line && !input.drift) {
                self.end_drift(tuning);
            } else {
                // Tighter drifts build up faster: from half to one and a half times real time.
                // Steering out of the drift charges at the slow end, never backwards.
                let gained = dt * (0.5 + self.drift_stick().max(0.0));
                self.drift_charge += gained;
                // Drifting is what fills the turbo.
                self.turbo = (self.turbo + gained * tuning.turbo_fill).min(TURBO_CHARGES);
                self.drift_held = if input.drift {
                    self.drift_held + dt
                } else {
                    0.0
                };
            }
        } else if speed >= tuning.drift_min_speed {
            let pressed = pressed || self.drift_press > 0.0;
            let side = if pressed && input.steer().abs() >= DRIFT_PRESS_STICK {
                // Steering right is a drift to the right, side 1.
                input.steer().signum() as i8
            } else if self.unscrewed_past(tuning.drift_entry_angle.to_radians(), tuning) {
                // A nose to the left of the travel is a car coming round to the left, side -1.
                if self.body > 0.0 { -1 } else { 1 }
            } else {
                0
            };
            if side != 0 {
                self.start_drift(side);
            }
        }
    }

    /// Whether the gripping car's nose has unscrewed past `angle` into a turn hard enough to keep
    /// it there: the body is past it and still asked beyond it, on the same side. A body left
    /// across by a wall or by a drift just ended is neither.
    fn unscrewed_past(&self, angle: f32, tuning: &CarTuning) -> bool {
        let target = self.body_target(tuning);
        self.body.abs() >= angle && target.abs() >= angle && self.body * target > 0.0
    }

    /// Starts a drift toward `side`. The heading becomes the travel, and the body takes over
    /// whatever slide the car had, so nothing on screen jumps.
    fn start_drift(&mut self, side: i8) {
        self.drift = side;
        self.drift_charge = 0.0;
        self.drift_held = 0.0;
        self.drift_press = 0.0;
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

    /// How far the steering points into the drift, from -1 fully against it to 1 fully into it,
    /// and 0 centered. The sign matters: centered means straight ahead, and against the drift
    /// means steering out of it, which is how a slide is caught.
    fn drift_stick(&self) -> f32 {
        self.steering * f32::from(self.drift)
    }

    /// The angle the body swings toward: across the travel, into the drift, as far as the
    /// steering points into it, most of it with the first half of its travel; back in line
    /// otherwise.
    fn body_target(&self, tuning: &CarTuning) -> f32 {
        if !self.is_drifting() {
            // Gripping, the nose unscrews into the turn as far as the car is turning hard: by
            // the share its sideways acceleration is of full lock's at top speed.
            let full = tuning.top_speed * tuning.top_speed / tuning.turn_radius_fast;
            let share = (self.yaw_rate * self.velocity.length() / full).clamp(-1.5, 1.5);
            return tuning.grip_swing_angle.to_radians() * share;
        }
        let into = self.drift_stick().clamp(0.0, 1.0);
        // Eases in from the center, where a square root would swing the body on every twitch.
        let share = 1.0 - (1.0 - into) * (1.0 - into);
        // A drift to the left, side -1, swings the nose to the left: a positive angle.
        -f32::from(self.drift) * tuning.drift_angle.to_radians() * share
    }

    /// How far the body is swung out into the drift, from 0 in line, or on the other side, to 1 at
    /// the full `drift_angle`.
    fn swung(&self, tuning: &CarTuning) -> f32 {
        // A drift to the left, side -1, swings the body to the left: a positive angle.
        (-self.body * f32::from(self.drift) / tuning.drift_angle.to_radians()).clamp(0.0, 1.0)
    }

    fn steer(&mut self, input: CarInput, tuning: &CarTuning, dt: f32) {
        let steering_speed = self.forward_speed().max(tuning.min_steering_speed);
        // The curvature at full lock of a car that grips, on a radius that widens with speed.
        let speed_share = (steering_speed / tuning.top_speed).min(1.0);
        let gripping = 1.0
            / (tuning.turn_radius_slow
                + (tuning.turn_radius_fast - tuning.turn_radius_slow) * speed_share);
        let curvature = if self.is_drifting() {
            // The steering sets the curve the car travels on, and it is signed: centered runs
            // straight, fully to one side turns on `drift_radius_tight`, whichever side the drift
            // is on. The curve tightens from the gripping car's as the body swings out, so a drift
            // turns harder as it gets under way rather than all at once; and that is all the body
            // does to where the car goes.
            let mut drifting = 1.0 / tuning.drift_radius_tight;
            if input.drift {
                // Holding the button pulls the drift tighter than the stick alone can, more and
                // more from the press: a tap to start a drift barely does.
                let held = (self.drift_held / tuning.drift_button_tighten_time).min(1.0);
                drifting /= 1.0 + (tuning.drift_button_tighten - 1.0) * held;
            }
            gripping + (drifting - gripping) * self.swung(tuning)
        } else {
            gripping
        };
        // Steering right turns clockwise, a negative yaw rate.
        let target_yaw_rate = -self.steering * steering_speed * curvature;
        let response = if self.is_drifting() {
            tuning.drift_steering_response
        } else {
            tuning.steering_response
        };
        let wanted =
            self.yaw_rate + (target_yaw_rate - self.yaw_rate) * (1.0 - decay(response, dt));
        // The car's weight: its rotation changes no faster than this, so it gathers steadily and
        // dies away steadily once the stick lets go.
        let weight = tuning.yaw_acceleration.to_radians() * dt;
        self.yaw_rate = move_towards(self.yaw_rate, wanted, weight);
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
    /// limit. Above top speed, whatever put the car there, the excess bleeds away at
    /// `overspeed_drag`.
    fn drive(
        &self,
        forward_speed: f32,
        speed: f32,
        input: CarInput,
        tuning: &CarTuning,
        dt: f32,
    ) -> f32 {
        // The turbo is the stronger of the two, and takes over from a boost while it fires.
        let boost = if self.turbo_boost > 0.0 {
            Some((tuning.turbo_top_speed, tuning.turbo_acceleration))
        } else if self.boost > 0.0 {
            Some((tuning.boost_top_speed, tuning.boost_acceleration))
        } else {
            None
        };
        let forward_speed = if let Some((boost_top_speed, acceleration)) = boost {
            // Full push up to top speed, fading out toward the boost's own top speed.
            let fade =
                ((boost_top_speed - speed) / (boost_top_speed - tuning.top_speed)).clamp(0.0, 1.0);
            forward_speed + acceleration * fade * dt
        } else if input.accelerate && speed < tuning.top_speed {
            let fade = 1.0 - speed / tuning.top_speed;
            forward_speed + tuning.acceleration * fade * dt
        } else {
            // Faster than the engine alone goes, after a boost or down a dive, the car keeps its
            // momentum: the excess fades, a lot of it quickly and the last of it slowly.
            let excess = (speed - tuning.top_speed).max(0.0);
            let held_back = excess * (1.0 - decay(tuning.overspeed_drag, dt));
            let coasting = if input.accelerate {
                0.0
            } else {
                tuning.coasting * dt
            };
            move_towards(forward_speed, 0.0, held_back + coasting)
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

    /// Pushes the car back onto the road. Returns how fast it was going into the wall it ran into,
    /// in m/s, and nothing when it touched none or only ran along one.
    ///
    /// The walls are read where the car is, not from the track as a whole: through a bottleneck
    /// they stand closer in, and the road itself squeezes the car toward the centerline.
    fn collide_with_walls(&mut self, tuning: &CarTuning, track: &Track, dt: f32) -> Option<f32> {
        let projection = track.project(self.position);
        let limit = track.half_width_at(projection.distance) - tuning.collision_radius;
        let overlap = projection.lateral.abs() - limit;
        if overlap <= 0.0 {
            return None;
        }

        let outward = (self.position - projection.closest).normalize_or_zero();
        self.position -= outward * overlap;

        let into_wall = self.velocity.dot(outward);
        if into_wall > 0.0 {
            let impact = into_wall * (1.0 + tuning.wall_bounce);
            self.velocity -= outward * impact;
            // The friction of the impact: the harder the car hits, the more of its speed along the
            // wall the hit takes, so a graze costs little and a blow at an angle a lot.
            let along_wall = self.velocity - outward * self.velocity.dot(outward);
            self.velocity -= along_wall.clamp_length_max(tuning.wall_impact_friction * impact);
        }
        // A car running along the wall without pushing into it only brushes it, and loses nothing:
        // the wall rubs a car that leans on it.
        if into_wall <= WALL_BRUSH {
            return None;
        }
        let along_wall = self.velocity - outward * self.velocity.dot(outward);
        self.velocity -= along_wall * (1.0 - decay(tuning.wall_friction, dt));
        if !self.is_drifting() && along_wall.length_squared() > 1.0 {
            // The wall deflects a gripping car: its nose comes round to where the hit sent it,
            // along the wall and a little off it with the bounce, so it comes away instead of
            // grinding on with its nose in. Only the way it was already going along the wall: a
            // car that hits head on, or stands against the wall, is left to steer away by itself.
            let along = wrap_angle(self.velocity.to_angle() - self.heading);
            if along.abs() < PI / 2.0 {
                // The body keeps pointing where it did and swings back into line in its own
                // time, as it does when a drift starts: the car turns at once, the picture
                // does not jump.
                let turn = along * (1.0 - decay(tuning.wall_alignment, dt));
                self.heading = wrap_angle(self.heading + turn);
                self.body -= turn;
            }
        }
        Some(into_wall)
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
    /// Gripping, the nose unscrews into the turn without carrying the car wide: at half lock the
    /// body swings well off the travel and holds there, the travel itself staying close to where
    /// the car steers, and no drift starts. At full lock it goes past the entry angle, and the car
    /// breaks away.
    #[test]
    fn the_nose_unscrews_into_a_turn_without_carrying_the_car_wide() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        run(&mut car, CarInput::new(true, 0.5), 1.5, &track);
        assert!(!car.is_drifting(), "half lock broke away");
        // Steering right: the nose is to the right of the heading, a negative angle.
        let unscrewed = -car.body.to_degrees();
        assert!(unscrewed > 4.0 && unscrewed < tuning().drift_entry_angle, "{unscrewed}");
        assert!(car.slip.abs() < unscrewed.to_radians(), "carried wider than it unscrews");

        let mut car = at_top_speed(&track);
        run(&mut car, CarInput::new(true, 1.0), 1.5, &track);
        assert!(car.is_drifting(), "full lock never broke away");
        assert_eq!(car.drift, 1);
    }

    /// A car driving over a speed pad is boosted, for `pad_boost` once it has left it; one driving
    /// beside it is not.
    #[test]
    fn a_speed_pad_boosts_whoever_drives_over_it() {
        use crate::track::Pad;
        let mut description = open_track();
        description.pads = vec![Pad {
            at: 40.0,
            lateral: 0.0,
            length: 10.0,
            width: 6.0,
        }];
        let track = Track::build(&description).unwrap();
        let drive_past = |lateral: f32| {
            let mut car = Car::new(Vec2::new(0.0, lateral), 0.0);
            car.velocity = Vec2::new(tuning().top_speed, 0.0);
            let mut boosted = false;
            for _ in 0..ticks(2.0) {
                car.step(ACCELERATE, &tuning(), &track);
                boosted |= car.is_boosting();
            }
            (boosted, car)
        };
        let (on, car) = drive_past(0.0);
        assert!(on, "the pad gave nothing");
        assert!(car.velocity.length() > tuning().top_speed, "{}", car.velocity.length());
        let (beside, _) = drive_past(8.0);
        assert!(!beside, "a car beside the pad was boosted");
    }

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
        turbo: false,
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
        // Three seconds: while its nose still points into the wall, the car pushes against it,
        // and the friction of that push holds it back, until the wall turns it along itself.
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

    /// Past the knee the body swings further out at no more than its share of the fastest swing,
    /// taken without a jump, and still reaches its full angle; swinging back is not held back.
    #[test]
    fn the_slide_grows_less_directly_past_the_knee() {
        let track = Track::build(&open_track()).unwrap();
        let knee = CarTuning {
            drift_swing_knee: 20.0,
            drift_swing_knee_rate: 0.4,
            ..tuning()
        };
        let swing = |tuning: &CarTuning| {
            let mut car = at_top_speed(&track);
            let mut rates = Vec::new();
            for tick in 0..ticks(2.5) {
                let input = CarInput::new(true, 1.0).with_drift(tick < 6);
                car.step(input, tuning, &track);
                rates.push((car.body.abs().to_degrees(), car.body_rate.abs().to_degrees()));
            }
            (car, rates)
        };
        let (free, _) = swing(&tuning());
        let (held, rates) = swing(&knee);
        assert!((free.body - held.body).abs() < 1e-3, "the knee kept the body from its angle");

        // The swing slows onto the knee at its acceleration, so the slower rate holds a little
        // past it: 66 deg/s shed at 450 deg/s² take about 11 degrees.
        let mut previous = 0.0f32;
        for &(angle, rate) in &rates {
            if angle > 35.0 {
                assert!(rate <= 110.0 * 0.4 + 0.5, "{rate} deg/s at {angle} degrees");
            }
            // No jump: the swing changes by no more than its acceleration allows.
            assert!((rate - previous).abs() <= 450.0 * TICK_SECONDS + 0.5);
            previous = rate;
        }
        let fastest = rates.iter().map(|&(_, rate)| rate).fold(0.0, f32::max);
        assert!(fastest > 60.0, "the body never swung at full rate before the knee");
    }

    /// The body comes off the travel on the very tick a drift starts, slowly, and swings faster and
    /// faster rather than lunging round, then settles on its angle: far across the travel, and
    /// further with the stick further into the drift.
    #[test]
    fn the_body_unscrews_at_once_slowly_then_faster() {
        let track = Track::build(&open_track()).unwrap();
        let full = tuning().drift_angle.to_radians();
        let mut angles = Vec::new();
        for steer in [1.0, 0.5] {
            let mut car = at_top_speed(&track);
            press_into_a_drift(&mut car, steer, &track);
            // Off the travel on the very tick, but only just.
            assert!(car.body_rate.abs() > 0.0, "the body has not moved");
            assert!(body_angle(&car) < 0.01, "{} rad", body_angle(&car));
            let mut rates = vec![car.body_rate.abs()];
            for _ in 0..ticks(0.2) {
                car.step(CarInput::new(true, steer), &tuning(), &track);
                rates.push(car.body_rate.abs());
            }
            // Faster and faster, by no more than a tick's worth of acceleration at a time.
            let most = tuning().drift_swing_acceleration.to_radians() * TICK_SECONDS * 1.01;
            assert!(
                rates
                    .windows(2)
                    .all(|pair| pair[1] > pair[0] && pair[1] - pair[0] <= most),
                "{rates:?}"
            );
            run(&mut car, CarInput::new(true, steer), 1.0, &track);
            assert!(car.is_drifting());
            angles.push(body_angle(&car));
        }
        assert!(
            angles[0] > 0.95 * full && angles[0] <= full * 1.01,
            "{} rad",
            angles[0]
        );
        assert!(
            angles[1] < angles[0] && angles[1] > 0.5 * full,
            "{angles:?}"
        );
    }

    /// No delay before the car turns: it starts to on the very tick the stick moves, gently, and
    /// turns a little harder on every tick after it as the steering comes round.
    #[test]
    fn the_car_turns_at_once_then_a_little_harder() {
        let track = Track::build(&open_track()).unwrap();
        let start = at_top_speed(&track);
        let mut settled = start;
        run(&mut settled, CarInput::new(true, 0.5), 1.0, &track);
        assert!(!settled.is_drifting());

        let mut car = start;
        let mut shares = Vec::new();
        for _ in 0..ticks(0.1) {
            car.step(CarInput::new(true, 0.5), &tuning(), &track);
            shares.push(car.yaw_rate / settled.yaw_rate);
        }
        assert!(shares[0] > 0.1 && shares[0] < 0.3, "{shares:?}");
        assert!(
            shares.windows(2).all(|pair| pair[1] > pair[0]),
            "{shares:?}"
        );
        assert!(shares[shares.len() - 1] > 0.6, "{shares:?}");
    }

    /// The car's weight: its rotation starts on the very tick the stick moves, gathers no faster
    /// than `yaw_acceleration` and, once the stick lets go, dies away no faster either, so a heavy
    /// car carries on round further than a light one.
    #[test]
    fn the_rotation_gathers_and_dies_away_with_the_weight() {
        let track = Track::build(&open_track()).unwrap();
        let start = at_top_speed(&track);
        let most = tuning().yaw_acceleration.to_radians() * TICK_SECONDS * 1.001;
        // Full lock, then the stick let go: the rotation through both, and how far the car turned
        // after letting go. The car is kept from breaking away, which would hand the rotation over
        // to a drift.
        let turned_after_letting_go = |tuning: &CarTuning| {
            let tuning = &CarTuning {
                drift_entry_angle: 45.0,
                ..tuning.clone()
            };
            let mut car = start;
            let mut rates = vec![0.0];
            for _ in 0..ticks(0.5) {
                car.step(CarInput::new(true, 1.0), tuning, &track);
                rates.push(car.yaw_rate);
            }
            let heading = car.heading;
            for _ in 0..ticks(1.0) {
                car.step(ACCELERATE, tuning, &track);
                rates.push(car.yaw_rate);
            }
            assert!(!car.is_drifting());
            (
                rates,
                Vec2::from_angle(heading).angle_to(car.forward()).abs(),
            )
        };

        let (rates, heavy) = turned_after_letting_go(&tuning());
        assert!(rates[1] < 0.0, "not turning right on the first tick");
        assert!(
            rates
                .windows(2)
                .all(|pair| (pair[1] - pair[0]).abs() <= most),
            "{rates:?}"
        );
        let light = CarTuning {
            yaw_acceleration: 1e5,
            ..tuning()
        };
        let (_, light) = turned_after_letting_go(&light);
        println!(
            "after letting go, a heavy car turns {} degrees more, a light one {}",
            heavy.to_degrees(),
            light.to_degrees()
        );
        assert!(heavy > light * 1.3, "{heavy} against {light}");
    }

    /// Above top speed the car keeps its momentum: a dive carries it past top speed, and what a
    /// boost gave fades by the same share of itself every second rather than all at once.
    #[test]
    fn momentum_carries_the_car_past_top_speed_and_fades() {
        let top_speed = tuning().top_speed;
        let mut description = oval(2000.0, 60.0, 0.0);
        description.elevation = vec![
            Elevation {
                at: 0.0,
                height: 0.0,
            },
            Elevation {
                at: 1000.0,
                height: -60.0,
            },
        ];
        let diving = Track::build(&description).unwrap();
        let mut car = Car::new(Vec2::ZERO, 0.0);
        car.velocity = Vec2::X * top_speed;
        run(&mut car, ACCELERATE, 15.0, &diving);
        assert!(
            car.velocity.length() > top_speed + 0.5,
            "{}",
            car.velocity.length()
        );

        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        car.boost = 1.0;
        run(&mut car, ACCELERATE, 1.0, &track);
        let excess = |car: &Car| car.velocity.length() - top_speed;
        let mut after = Vec::new();
        for _ in 0..3 {
            run(&mut car, ACCELERATE, 1.0, &track);
            after.push(excess(&car));
        }
        let kept = (-tuning().overspeed_drag).exp();
        println!("excess speed a second apart: {after:?}");
        assert!(after[0] > 1.0, "{after:?}");
        for pair in after.windows(2) {
            assert!((pair[1] / pair[0] - kept).abs() < 0.01, "{after:?}");
        }
    }

    /// A hit takes speed along the wall as well as into it, more the harder the car hits: a graze
    /// costs little, a blow at an angle a lot.
    #[test]
    fn a_wall_costs_more_the_harder_it_is_hit() {
        let track = long_track();
        // The speed lost on the tick the car meets the wall.
        let speed_lost_hitting = |degrees: f32| {
            let mut car = Car::new(Vec2::new(100.0, 0.0), degrees.to_radians());
            car.velocity = car.forward() * 30.0;
            let limit = track.half_width() - tuning().collision_radius;
            loop {
                let before = car.velocity.length();
                car.step(CarInput::new(true, 0.0), &tuning(), &track);
                if track.project(car.position).lateral >= limit - 1e-3 {
                    return before - car.velocity.length();
                }
            }
        };
        let graze = speed_lost_hitting(3.0);
        let blow = speed_lost_hitting(25.0);
        println!("a graze costs {graze} m/s, a blow {blow} m/s");
        assert!(graze < 2.5, "{graze}");
        assert!(blow > 4.0 * graze, "{blow} against {graze}");
    }

    /// The steering turns toward the stick rather than jumping to it, and the body swings with a
    /// speed of its own, so the nose never jerks: into a drift, tapped with a press that lasts a
    /// few ticks the way a thumb's does, or broken into by turning hard, then easing the stick,
    /// pushing it back and straightening, its rotation changes only a little from one tick to the
    /// next, and never turns faster than the travel and the fastest swing together.
    #[test]
    fn the_nose_turns_without_jerking() {
        let track = Track::build(&open_track()).unwrap();
        for tapped in [true, false] {
            let mut car = at_top_speed(&track);
            let mut heading = car.body_heading();
            let mut rate = 0.0;
            let mut sharpest: f32 = 0.0;
            let mut fastest: f32 = 0.0;
            for tick in 0..ticks(2.5) {
                let steer = match tick {
                    0..=59 => 1.0,
                    60..=83 => 0.5,
                    84..=107 => 1.0,
                    _ => 0.0,
                };
                // A tap held for 0.1 s, straight from running straight.
                let input = CarInput::new(true, steer).with_drift(tapped && tick < 6);
                car.step(input, &tuning(), &track);
                if tapped && tick == 0 {
                    assert!(car.is_drifting());
                    // The steering moves on the very tick the stick does, but only part of the
                    // way.
                    assert!(
                        car.steering > 0.1 && car.steering < 0.4,
                        "the wheel went to {}",
                        car.steering
                    );
                }
                let turned =
                    Vec2::from_angle(heading).angle_to(Vec2::from_angle(car.body_heading()));
                heading = car.body_heading();
                let new_rate = turned * TICK_RATE as f32;
                sharpest = sharpest.max((new_rate - rate).abs());
                fastest = fastest.max(new_rate.abs());
                rate = new_rate;
            }
            assert!(car.drift_charge > 0.0 || car.boost > 0.0, "no drift at all");
            // In degrees per second, from one tick to the next. A body that followed its target at
            // a plain rate jumped by some 460 on the tick a drift began, and a spring by 93 on the
            // tick turning hard broke the car away.
            println!(
                "tapped {tapped}: sharpest change of the nose's rotation {} deg/s, fastest {} deg/s",
                sharpest.to_degrees(),
                fastest.to_degrees()
            );
            assert!(sharpest.to_degrees() < 40.0, "{}", sharpest.to_degrees());
            let tightest = tuning().top_speed / (tuning().drift_radius_tight * 0.7);
            assert!(
                fastest < tightest + tuning().drift_swing_rate.to_radians() * 1.05,
                "{} deg/s",
                fastest.to_degrees()
            );
        }
    }

    /// However far the body swings, the car travels on the curve the stick asks for, a curve that
    /// tightens from the gripping car's to the drift's as the body swings out: a quarter turn at
    /// full lock takes a drifting car less far ahead than a gripping one, never wide.
    #[test]
    fn a_drift_goes_where_the_stick_says_without_running_wide() {
        let track = Track::build(&open_track()).unwrap();
        let start = at_top_speed(&track);
        let quarter_turn = |car: &mut Car| {
            let mut ticks_left = ticks(6.0);
            // Until the direction of travel has turned a quarter turn to the right.
            while start.velocity.angle_to(car.velocity) > -std::f32::consts::FRAC_PI_2 {
                car.step(CarInput::new(true, 1.0), &tuning(), &track);
                ticks_left -= 1;
                assert!(ticks_left > 0, "never turned");
            }
            (car.position - start.position).dot(start.forward())
        };
        let mut drifting = start;
        press_into_a_drift(&mut drifting, 1.0, &track);
        let drift = quarter_turn(&mut drifting);
        assert!(body_angle(&drifting) > 0.8 * tuning().drift_angle.to_radians());

        // Gripping all the way: a car that turns hard breaks into a drift, so this one is held in
        // its grip by hand, as if it never could.
        let mut gripping = start;
        let mut ticks_left = ticks(6.0);
        while start.velocity.angle_to(gripping.velocity) > -std::f32::consts::FRAC_PI_2 {
            gripping.step(CarInput::new(true, 1.0), &tuning(), &track);
            // Its nose never unscrews far enough to break away.
            gripping.body = 0.0;
            gripping.body_rate = 0.0;
            ticks_left -= 1;
            assert!(ticks_left > 0, "never turned");
        }
        let grip = (gripping.position - start.position).dot(start.forward());
        println!("a quarter turn drifting {drift} m ahead, gripping {grip} m");
        assert!(drift < grip - 2.0, "{drift} against {grip}");
        assert!(drift > tuning().drift_radius_tight, "{drift}");
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
        // The steering unwinds and the body swings back in about half a second.
        let mut released = car;
        let mut left = ticks(0.75);
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
    /// charge and all: an S is one drift. The body swings all the way across, twice the angle, at
    /// its fastest swing.
    #[test]
    fn a_held_button_carries_a_drift_over_to_the_other_side() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        press_into_a_drift(&mut car, 1.0, &track);
        run(&mut car, CarInput::new(true, 1.0), 1.0, &track);
        let charge = car.drift_charge;

        run(&mut car, drifting(-1.0), 1.2, &track);
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

    /// A press made a moment before the stick is turned still starts its drift; one made long
    /// before does not.
    #[test]
    fn a_press_just_before_the_stick_turns_still_starts_a_drift() {
        let track = Track::build(&open_track()).unwrap();
        for (wait, starts) in [(0.1, true), (0.4, false)] {
            let mut car = at_top_speed(&track);
            // A tap of two ticks with the stick still centered.
            run(&mut car, drifting(0.0), 2.0 / TICK_RATE as f32, &track);
            assert!(!car.is_drifting());
            run(&mut car, ACCELERATE, wait - 2.0 / TICK_RATE as f32, &track);
            car.step(CarInput::new(true, 0.6), &tuning(), &track);
            assert_eq!(car.is_drifting(), starts, "{wait} s after the press");
        }
    }

    /// A car set drifting along the left wall of a long straight, `into_wall` m/s into it.
    fn drifting_into_the_wall(track: &Track, into_wall: f32) -> Car {
        let mut car = Car::new(
            Vec2::new(100.0, track.half_width() - tuning().collision_radius - 0.01),
            0.0,
        );
        car.velocity = Vec2::new(30.0, into_wall);
        car.heading = car.velocity.to_angle();
        car.drift = -1;
        car.drift_charge = 1.0;
        car.steering = -1.0;
        car
    }

    /// A drift survives a graze of the wall, building nothing up while it touches; a real hit
    /// loses it.
    #[test]
    fn a_drift_survives_a_graze_and_is_lost_to_a_hit() {
        let track = long_track();
        let mut grazing = drifting_into_the_wall(&track, 1.5);
        let turbo = grazing.turbo;
        grazing.step(CarInput::new(true, -1.0), &tuning(), &track);
        assert!(grazing.is_drifting());
        assert_eq!(grazing.drift_charge, 1.0);
        assert_eq!(grazing.turbo, turbo);

        let mut hitting = drifting_into_the_wall(&track, 8.0);
        hitting.step(CarInput::new(true, -1.0), &tuning(), &track);
        assert!(!hitting.is_drifting());
        assert_eq!(hitting.boost, 0.0);
    }

    /// A wall turns the nose of a gripping car that runs into it along itself, so the car comes
    /// away from it: without that, it grinds along with its nose in and loses far more speed.
    #[test]
    fn a_wall_deflects_the_car_along_itself() {
        let track = long_track();
        // A hit too shallow to break the car away into a drift, which follows the wall by itself.
        let shallow = CarTuning {
            drift_entry_angle: 9.0,
            ..tuning()
        };
        let after_hitting = |tuning: &CarTuning| {
            let mut car = Car::new(Vec2::new(100.0, 0.0), 7f32.to_radians());
            car.velocity = car.forward() * 35.0;
            for _ in 0..ticks(2.0) {
                car.step(ACCELERATE, tuning, &track);
            }
            car
        };
        let deflected = after_hitting(&shallow);
        let stubborn = after_hitting(&CarTuning {
            wall_alignment: 0.0,
            ..shallow.clone()
        });
        println!(
            "two seconds after a hit at 7 degrees: {} m/s deflected, {} m/s left alone",
            deflected.velocity.length(),
            stubborn.velocity.length()
        );
        assert!(deflected.heading.abs() < 1f32.to_radians(), "{deflected:?}");
        assert!(deflected.body.abs() < 1f32.to_radians(), "{deflected:?}");
        assert!(stubborn.heading > 6f32.to_radians(), "{stubborn:?}");
        assert!(deflected.velocity.length() > stubborn.velocity.length() + 10.0);

        // The body does not jump with the car: it is left pointing where it did, and swings back.
        let mut car = Car::new(Vec2::new(100.0, 0.0), 7f32.to_radians());
        car.velocity = car.forward() * 35.0;
        let mut pointing = car.body_heading();
        while car.body == 0.0 {
            pointing = car.body_heading();
            car.step(ACCELERATE, &shallow, &track);
        }
        let jumped = wrap_angle(car.body_heading() - pointing).abs();
        assert!(jumped < 0.5f32.to_radians(), "{jumped}");
    }

    /// Drifting fills the turbo meter, up to its three charges and no further. A press spends one
    /// charge on a boost faster than a drift's; a held button spends no more, and an empty meter
    /// fires nothing.
    #[test]
    fn drifting_fills_the_turbo_and_a_press_fires_a_charge() {
        let track = Track::build(&open_track()).unwrap();
        let mut car = at_top_speed(&track);
        car.step(ACCELERATE.with_turbo(true), &tuning(), &track);
        assert_eq!(car.turbo_boost, 0.0, "nothing to fire");
        car.step(ACCELERATE, &tuning(), &track);

        press_into_a_drift(&mut car, 1.0, &track);
        run(&mut car, CarInput::new(true, 1.0), 4.0, &track);
        assert!(car.turbo > 1.0 && car.turbo < 2.0, "{}", car.turbo);
        run(&mut car, CarInput::new(true, 1.0), 20.0, &track);
        assert_eq!(car.turbo, TURBO_CHARGES);

        // Out of the drift, and its boost left to run out.
        run(&mut car, ACCELERATE, 6.0, &track);
        assert!(!car.is_boosting());
        let turbo = ACCELERATE.with_turbo(true);
        car.step(turbo, &tuning(), &track);
        assert_eq!(car.turbo, TURBO_CHARGES - 1.0);
        assert!(car.turbo_boost > 0.0);
        run(&mut car, turbo, 1.5, &track);
        assert_eq!(car.turbo, TURBO_CHARGES - 1.0, "held, it fires once");
        let speed = car.velocity.length();
        assert!(
            speed > tuning().boost_top_speed && speed <= tuning().turbo_top_speed,
            "{speed}"
        );
        run(&mut car, turbo, 1.0, &track);
        assert!(!car.is_boosting());
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
