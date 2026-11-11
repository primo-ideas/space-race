//! What the player presses, sent to the server whenever it changes.
//!
//! There is no brake: the game is full speed all the time.
//!
//! Gamepad: the right bumper (R, above the trigger) accelerates, the left stick or D-pad steers, X
//! (the left face button) drifts, A (the bottom one) or the left bumper fires the turbo. Keyboard:
//! W accelerates, A and D steer, Shift drifts, E fires the turbo. Keys are
//! read by their position on the keyboard, not by the letter printed on them, so other layouts get
//! the same physical keys. On a touchscreen, two thumbs on the controls drawn over the race do all
//! three (see `ui/touch.rs`).
//!
//! Every device is read at once and they are added together, so a gamepad, a keyboard and a
//! screen can be used one after another, or at the same time, without anything being chosen.
//!
//! Each device's steering is shaped for what it is (docs/simulation.md#steering-from-each-device):
//!
//! - **A stick** has a dead zone of its own, so a worn stick resting a little off center does not
//!   steer, and reaches full lock a little short of its rim, which a stick pushed at an angle
//!   never reaches.
//! - **Keys and the D-pad** say nothing between straight and full lock, so they ease in: half lock
//!   on the press, full lock 0.2 s later. A tap is a correction and a held key a turn, where every
//!   tap was a full turn of the wheel. With both directions held, the last one pressed steers.
//! - **Thumbs on a screen** already say how hard, by where they rest (see `ui/touch.rs`).
//!
//! The arrow keys and Space do what W, A, D and Shift do, and the down arrow what E does. A gamepad
//! rumbles when its car hits a wall, breaks into a drift or fires a boost, harder for the turbo, as
//! a phone vibrates (see `ui/touch.rs`).

use std::time::Duration;

use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use star_racer_protocol::{ClientMessage, LobbyPhase};
use star_racer_sim::autopilot;
use star_racer_sim::car::CarInput;

use crate::latency::Latency;
use crate::lobby::CurrentLobby;
use crate::net::Network;
use crate::prediction::Prediction;
use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;
use crate::settings::Script;
use crate::track::CurrentTrack;
use crate::ui::hud::LeaveDialog;
use crate::ui::touch::{TouchControls, TouchUpdate};
use crate::weight::FollowedPush;

pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastSent>()
            .add_systems(OnEnter(Screen::Lobby), reset_last_sent)
            .add_systems(
                Update,
                send_input
                    .after(RaceUpdate)
                    .after(TouchUpdate)
                    .run_if(in_state(Screen::Lobby)),
            )
            .add_systems(PostUpdate, rumble.run_if(in_state(Screen::Lobby)));
    }
}

/// The input the server knows, and its number in this lobby.
#[derive(Resource, Default)]
struct LastSent {
    input: CarInput,
    /// 0 before the first one, which is 1: snapshots say 0 until the server has one.
    seq: u32,
}

/// A player enters a lobby with nothing pressed, as far as the server knows, and counts their
/// inputs there from the start.
fn reset_last_sent(mut last_sent: ResMut<LastSent>) {
    *last_sent = LastSent::default();
}

fn send_input(
    script: Res<Script>,
    view: Res<RaceView>,
    current: Res<CurrentLobby>,
    track: Option<Res<CurrentTrack>>,
    leave_dialog: Res<LeaveDialog>,
    gamepads: Query<&Gamepad>,
    keyboard: Res<ButtonInput<KeyCode>>,
    touch: Res<TouchControls>,
    time: Res<Time>,
    mut digital: Local<DigitalSteering>,
    network: Res<Network>,
    mut last_sent: ResMut<LastSent>,
    mut latency: ResMut<Latency>,
    mut prediction: ResMut<Prediction>,
) {
    let input = if leave_dialog.open || view.own_car.is_none() {
        // Asking whether to leave, or spectating: the car lets go.
        CarInput::default()
    } else if let Some(style) = script.autopilot {
        match (view.own_car, track) {
            // Waits for the start on screen to press, for a perfect start.
            _ if before_start(&current, &view) => CarInput::default(),
            (Some(car), Some(track)) => autopilot::drive_in_style(style, &car, &track.0),
            _ => CarInput::default(),
        }
    } else {
        player_input(
            &gamepads,
            &keyboard,
            &touch,
            &mut digital,
            time.delta_secs(),
        )
    };

    if last_sent.input != input {
        // Placed on the next tick of the player's own timeline, where their car is predicted,
        // and where the server will apply it too (see prediction.rs).
        let tick = prediction.place(input);
        let seq = last_sent.seq + 1;
        network.send(ClientMessage::Input { input, tick, seq });
        latency.input_sent(seq, Instant::now(), tick);
        *last_sent = LastSent { input, seq };
    }
}

/// Whether the cars on screen wait on the grid for the start.
fn before_start(current: &CurrentLobby, view: &RaceView) -> bool {
    let phase = current
        .0
        .as_ref()
        .and_then(|membership| membership.state.as_ref())
        .map(|state| &state.phase);
    // On the player's own timeline while their car is predicted: the start their car can make.
    match (phase, view.own_tick.or(view.tick)) {
        (Some(LobbyPhase::Racing { start_tick }), Some(tick)) => tick < f64::from(*start_tick),
        _ => false,
    }
}

/// Every connected gamepad, the keyboard and the thumbs on the screen at once: whichever is used
/// drives.
fn player_input(
    gamepads: &Query<&Gamepad>,
    keyboard: &ButtonInput<KeyCode>,
    touch: &TouchControls,
    digital: &mut DigitalSteering,
    seconds: f32,
) -> CarInput {
    let mut accelerate =
        keyboard.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) || touch.accelerating();
    let mut drift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::Space])
        || touch.drifting();
    // Never Control: with W held, it would close a browser tab.
    let mut turbo =
        keyboard.any_pressed([KeyCode::KeyE, KeyCode::ArrowDown]) || touch.firing_turbo();
    let mut steer: f32 = touch.steer();

    let mut left = keyboard.any_pressed(LEFT_KEYS);
    let mut right = keyboard.any_pressed(RIGHT_KEYS);
    let mut pressed = if keyboard.any_just_pressed(LEFT_KEYS) {
        -1
    } else if keyboard.any_just_pressed(RIGHT_KEYS) {
        1
    } else {
        0
    };
    for gamepad in gamepads {
        accelerate |= gamepad.pressed(GamepadButton::RightTrigger);
        drift |= gamepad.pressed(GamepadButton::West);
        // The bottom face button, or the left bumper for a thumb busy holding a drift.
        turbo |= gamepad.any_pressed([GamepadButton::South, GamepadButton::LeftTrigger]);
        steer += stick_steering(gamepad.left_stick().x);
        left |= gamepad.pressed(GamepadButton::DPadLeft);
        right |= gamepad.pressed(GamepadButton::DPadRight);
        if gamepad.just_pressed(GamepadButton::DPadLeft) {
            pressed = -1;
        } else if gamepad.just_pressed(GamepadButton::DPadRight) {
            pressed = 1;
        }
    }
    steer += digital.steer(left, right, pressed, seconds);

    CarInput::new(accelerate, steer)
        .with_drift(drift)
        .with_turbo(turbo)
}

const LEFT_KEYS: [KeyCode; 2] = [KeyCode::KeyA, KeyCode::ArrowLeft];
const RIGHT_KEYS: [KeyCode; 2] = [KeyCode::KeyD, KeyCode::ArrowRight];

/// A stick resting this far off center does not steer: a worn one rests a little off it.
const STICK_DEAD: f32 = 0.1;
/// A stick pushed this far steers at full lock: one pushed at an angle never reaches its rim.
const STICK_FULL: f32 = 0.94;

/// The steering a stick's sideways push asks for: nothing in its dead zone, then in proportion up
/// to full lock, a little short of the rim. In proportion in between: the handling with a gamepad
/// was judged right as it was, and only its two ends change.
fn stick_steering(x: f32) -> f32 {
    let amount = ((x.abs() - STICK_DEAD) / (STICK_FULL - STICK_DEAD)).clamp(0.0, 1.0);
    amount.copysign(x)
}

/// How far a key or a D-pad button steers the moment it is pressed, and how much further every
/// second it stays held, up to full lock: half lock at once, full lock 0.2 s later.
const KEY_FIRST: f32 = 0.5;
const KEY_RAMP: f32 = 2.5;
/// The ramp moves in steps of a simulation tick, so a held key sends no more inputs than the
/// server has ticks, however fast the frames come.
const KEY_STEP: f32 = 1.0 / 60.0;

/// Steering from keys and the D-pad, which say only left or right: it eases in while they are held.
#[derive(Default)]
struct DigitalSteering {
    /// The side held, -1 left or 1 right, and for how long, in seconds.
    side: i8,
    held: f32,
    /// The last side pressed, which steers while both are held.
    last: i8,
}

impl DigitalSteering {
    /// The steering for this frame, `seconds` after the last: with `left` and `right` held, and
    /// `pressed` the side pressed this frame, if one was.
    fn steer(&mut self, left: bool, right: bool, pressed: i8, seconds: f32) -> f32 {
        if pressed != 0 {
            self.last = pressed;
        }
        let side = match (left, right) {
            (true, false) => -1,
            (false, true) => 1,
            (true, true) => self.last,
            (false, false) => 0,
        };
        if side != self.side {
            self.side = side;
            self.held = 0.0;
        } else {
            self.held += seconds;
        }
        if side == 0 {
            return 0.0;
        }
        let held = (self.held / KEY_STEP + 1e-3).floor() * KEY_STEP;
        (KEY_FIRST + KEY_RAMP * held).min(1.0) * f32::from(side)
    }
}

/// How soon a rumble for a hit may follow the last one: scraping along a wall is a hit on every
/// frame.
const RUMBLE_EVERY: f64 = 0.3;

/// What the gamepad last told the player.
#[derive(Default)]
struct Rumbled {
    hit: f64,
    drifting: bool,
    boosting: bool,
    turbo: bool,
}

/// Rumbles every gamepad when the player's car hits a wall, harder the harder the hit, breaks into
/// a drift, or fires a boost: what a phone's vibration motor says on a touchscreen.
fn rumble(
    push: Res<FollowedPush>,
    view: Res<RaceView>,
    time: Res<Time<Real>>,
    gamepads: Query<Entity, With<Gamepad>>,
    mut requests: MessageWriter<GamepadRumbleRequest>,
    mut rumbled: Local<Rumbled>,
) {
    let Some(car) = view.own_car else {
        *rumbled = Rumbled::default();
        return;
    };
    let now = time.elapsed_secs_f64();
    let mut send = |strong: f32, weak: f32, millis: u64| {
        for gamepad in &gamepads {
            requests.write(GamepadRumbleRequest::Add {
                gamepad,
                duration: Duration::from_millis(millis),
                intensity: GamepadRumbleIntensity {
                    strong_motor: strong,
                    weak_motor: weak,
                },
            });
        }
    };
    if let Some(push) = push.0
        && let Some((strength, millis)) = hit_rumble(push.impact)
        && now - rumbled.hit >= RUMBLE_EVERY
    {
        rumbled.hit = now;
        send(strength, strength * 0.5, millis);
    }
    if car.is_drifting() && !rumbled.drifting {
        send(0.0, 0.3, 50);
    }
    if car.turbo_boost > 0.0 && !rumbled.turbo {
        // The turbo kicks: both motors, hard, for longer than any boost.
        send(0.8, 1.0, 320);
    } else if car.boost > 0.0 && !rumbled.boosting {
        send(0.35, 0.6, 180);
    }
    rumbled.drifting = car.is_drifting();
    rumbled.boosting = car.boost > 0.0;
    rumbled.turbo = car.turbo_boost > 0.0;
}

/// How hard, from 0 to 1, and how long, in milliseconds, a hit that took `impact` m/s from the car
/// rumbles, if it is more than a graze.
fn hit_rumble(impact: f32) -> Option<(f32, u64)> {
    (impact >= 1.5).then(|| {
        let strength = (impact / 12.0).clamp(0.25, 1.0);
        (strength, (60.0 + 100.0 * strength) as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stick_has_a_dead_zone_and_reaches_full_lock_short_of_its_rim() {
        assert_eq!(stick_steering(0.0), 0.0);
        assert_eq!(stick_steering(0.08), 0.0);
        assert_eq!(stick_steering(-0.08), 0.0);
        assert_eq!(stick_steering(0.95), 1.0);
        assert_eq!(stick_steering(-1.0), -1.0);
        let half = stick_steering(0.52);
        assert!((half - 0.5).abs() < 1e-4, "{half}");
        // In proportion in between.
        let quarter = stick_steering(-0.31);
        assert!((quarter + 0.25).abs() < 1e-4, "{quarter}");
    }

    /// Steps a held key for `seconds`, frame by frame at `fps`, and returns the last steering.
    fn hold(digital: &mut DigitalSteering, left: bool, right: bool, seconds: f32, fps: f32) -> f32 {
        let mut steer = 0.0;
        for _ in 0..(seconds * fps).round() as usize {
            steer = digital.steer(left, right, 0, 1.0 / fps);
        }
        steer
    }

    #[test]
    fn a_key_steers_at_half_lock_at_once_and_at_full_lock_soon_after() {
        let mut digital = DigitalSteering::default();
        assert_eq!(digital.steer(false, true, 1, 1.0 / 60.0), 0.5);
        let after_a_tenth = hold(&mut digital, false, true, 0.1, 60.0);
        assert!(
            after_a_tenth > 0.7 && after_a_tenth < 0.8,
            "{after_a_tenth}"
        );
        assert_eq!(hold(&mut digital, false, true, 0.2, 60.0), 1.0);
        // Let go, and it steers straight at once; the car's own steering eases it back.
        assert_eq!(digital.steer(false, false, 0, 1.0 / 60.0), 0.0);
    }

    /// At 300 frames a second the ramp moves no more often than at 60: one input per tick.
    #[test]
    fn a_held_key_changes_the_steering_no_faster_than_the_server_ticks() {
        let mut digital = DigitalSteering::default();
        let mut changes = 0;
        let mut last = digital.steer(true, false, -1, 1.0 / 300.0);
        for _ in 0..90 {
            let steer = digital.steer(true, false, 0, 1.0 / 300.0);
            if steer != last {
                changes += 1;
                last = steer;
            }
        }
        assert_eq!(last, -1.0);
        assert!(changes <= 13, "{changes}");
    }

    /// Both held: the last pressed steers, as on the touchscreen, rather than the two cancelling
    /// out and driving the car straight into the wall.
    #[test]
    fn with_both_keys_held_the_last_one_pressed_steers() {
        let mut digital = DigitalSteering::default();
        digital.steer(true, false, -1, 1.0 / 60.0);
        assert_eq!(digital.steer(true, true, 1, 1.0 / 60.0), 0.5);
        // Letting go of the right one goes back to the left, from half lock again.
        assert_eq!(digital.steer(true, false, 0, 1.0 / 60.0), -0.5);
    }

    #[test]
    fn a_hit_rumbles_as_hard_as_it_was() {
        assert_eq!(hit_rumble(1.0), None);
        let (light, short) = hit_rumble(2.0).unwrap();
        let (hard, long) = hit_rumble(20.0).unwrap();
        assert!(light < hard && short < long);
        assert_eq!(hard, 1.0);
    }
}
