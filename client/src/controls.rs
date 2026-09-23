//! What the player presses, sent to the server whenever it changes.
//!
//! There is no brake: the game is full speed all the time.
//!
//! Gamepad: the right bumper (R, above the trigger) accelerates, the left stick or D-pad steers, X
//! (the left face button) drifts. Keyboard: W accelerates, A and D steer, Shift drifts. Keys are
//! read by their position on the keyboard, not by the letter printed on them, so other layouts get
//! the same physical keys. On a touchscreen, two thumbs on the controls drawn over the race do all
//! three (see `ui/touch.rs`).
//!
//! Every device is read at once and they are added together, so a gamepad, a keyboard and a
//! screen can be used one after another, or at the same time, without anything being chosen.

use bevy::platform::time::Instant;
use bevy::prelude::*;
use space_race_protocol::{ClientMessage, LobbyPhase};
use space_race_sim::autopilot;
use space_race_sim::car::CarInput;

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
            );
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
        player_input(&gamepads, &keyboard, &touch)
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
) -> CarInput {
    let mut accelerate = keyboard.pressed(KeyCode::KeyW) || touch.accelerating();
    let mut drift =
        keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) || touch.drifting();
    let mut steer: f32 = touch.steer();

    for gamepad in gamepads {
        accelerate |= gamepad.pressed(GamepadButton::RightTrigger);
        drift |= gamepad.pressed(GamepadButton::West);
        steer += gamepad.left_stick().x + gamepad.dpad().x;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        steer -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        steer += 1.0;
    }

    CarInput::new(accelerate, steer).with_drift(drift)
}
