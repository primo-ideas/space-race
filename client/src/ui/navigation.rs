//! Moving around the menus without a mouse.
//!
//! - Arrow keys, the D-pad or the left stick move the focus to the nearest widget in that
//!   direction (Bevy's automatic directional navigation). Tab and Shift+Tab go through every widget
//!   in reading order, and wrap around.
//! - Left and right step a focused [`Stepper`] instead, and stay inside a focused text field.
//! - Enter or the gamepad's A button activates the focused widget. Buttons handle Enter
//!   themselves; everything else gets an [`Activate`] from here.
//! - Escape or the gamepad's B button is "back", which each screen interprets.

use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::input::gamepad::GamepadEvent;
use bevy::input_focus::FocusCause;
use bevy::input_focus::directional_navigation::DirectionalNavigationPlugin;
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::auto_directional_navigation::{AutoDirectionalNavigation, AutoDirectionalNavigator};
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::ui_widgets::{Activate, Button};

use super::widgets::Stepper;

/// How far the stick must lean to move the focus, and how fast holding it repeats.
const STICK_THRESHOLD: f32 = 0.55;
const REPEAT_DELAY: f64 = 0.4;
const REPEAT_INTERVAL: f64 = 0.12;

pub struct NavigationPlugin;

impl Plugin for NavigationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(DirectionalNavigationPlugin)
            .init_resource::<LastInput>()
            .add_systems(
                PreUpdate,
                (track_last_input, navigate).chain().after(InputSystems),
            );
    }
}

/// The device the player used last, so hints show the right buttons.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastInput {
    #[default]
    Keyboard,
    Gamepad,
}

/// Menu actions from the keyboard and every gamepad at once.
#[derive(SystemParam)]
pub struct MenuInput<'w, 's> {
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

impl MenuInput<'_, '_> {
    pub fn back(&self) -> bool {
        self.keyboard.just_pressed(KeyCode::Escape) || self.gamepad(GamepadButton::East)
    }

    pub fn key(&self, key: KeyCode) -> bool {
        self.keyboard.just_pressed(key)
    }

    pub fn gamepad(&self, button: GamepadButton) -> bool {
        self.gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(button))
    }
}

fn track_last_input(
    mut last_input: ResMut<LastInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut gamepad_events: MessageReader<GamepadEvent>,
) {
    let gamepad_used = gamepad_events.read().fold(false, |used, event| {
        used || match event {
            GamepadEvent::Button(button) => button.state.is_pressed(),
            GamepadEvent::Axis(axis) => axis.value.abs() > STICK_THRESHOLD,
            GamepadEvent::Connection(_) => false,
        }
    });
    let used = if gamepad_used {
        LastInput::Gamepad
    } else if keyboard.get_just_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some()
    {
        LastInput::Keyboard
    } else {
        return;
    };
    last_input.set_if_neq(used);
}

/// The stick direction held, and when it moves the focus again.
#[derive(Default)]
struct StickRepeat {
    held: Option<CompassOctant>,
    next_at: f64,
}

fn navigate(
    input: MenuInput,
    mut navigator: AutoDirectionalNavigator,
    mut steppers: Query<&mut Stepper>,
    text_fields: Query<(), With<EditableText>>,
    buttons: Query<(), With<Button>>,
    widgets: Query<NavigableWidget, With<AutoDirectionalNavigation>>,
    time: Res<Time<Real>>,
    mut stick: Local<StickRepeat>,
    mut commands: Commands,
) {
    let focused = navigator.input_focus();
    let in_text = focused.is_some_and(|entity| text_fields.contains(entity));
    let shift =
        input.keyboard.pressed(KeyCode::ShiftLeft) || input.keyboard.pressed(KeyCode::ShiftRight);

    let mut direction = None;
    for (key, octant) in [
        (KeyCode::ArrowUp, CompassOctant::North),
        (KeyCode::ArrowDown, CompassOctant::South),
        (KeyCode::ArrowLeft, CompassOctant::West),
        (KeyCode::ArrowRight, CompassOctant::East),
    ] {
        let horizontal = matches!(octant, CompassOctant::West | CompassOctant::East);
        if input.key(key) && !(horizontal && in_text) {
            direction = Some(octant);
        }
    }
    if input.key(KeyCode::Tab) {
        let order = reading_order(&widgets);
        let next = match focused.and_then(|entity| order.iter().position(|&e| e == entity)) {
            Some(index) if shift => order.get((index + order.len() - 1) % order.len()),
            Some(index) => order.get((index + 1) % order.len()),
            None => order.first(),
        };
        if let Some(&next) = next {
            navigator
                .manual_directional_navigation
                .focus
                .set(next, FocusCause::Navigated);
        }
    }
    for (button, octant) in [
        (GamepadButton::DPadUp, CompassOctant::North),
        (GamepadButton::DPadDown, CompassOctant::South),
        (GamepadButton::DPadLeft, CompassOctant::West),
        (GamepadButton::DPadRight, CompassOctant::East),
    ] {
        if input.gamepad(button) {
            direction = Some(octant);
        }
    }
    if let Some(octant) = stick_direction(&input, &mut stick, time.elapsed_secs_f64()) {
        direction = Some(octant);
    }

    if let Some(octant) = direction {
        let step = match octant {
            CompassOctant::West => Some(-1),
            CompassOctant::East => Some(1),
            _ => None,
        };
        match (
            focused.and_then(|entity| steppers.get_mut(entity).ok()),
            step,
        ) {
            (Some(mut stepper), Some(step)) => stepper.step(step),
            _ => {
                // No widget in that direction: the focus stays where it is.
                let _ = navigator.navigate(octant);
            }
        }
    }

    let Some(focused) = navigator.input_focus() else {
        return;
    };
    let enter = input.key(KeyCode::Enter) || input.key(KeyCode::NumpadEnter);
    // Buttons already handle Enter.
    if input.gamepad(GamepadButton::South) || (enter && !buttons.contains(focused)) {
        commands.trigger(Activate { entity: focused });
    }
}

/// Every visible widget that takes the focus, as a page reads: rows from top to bottom, each from
/// left to right. A widget belongs to the row of the first one above it whose height spans its
/// center, so a small button beside a tall one still shares its row.
fn reading_order(widgets: &Query<NavigableWidget, With<AutoDirectionalNavigation>>) -> Vec<Entity> {
    let mut areas: Vec<(Entity, Vec2, f32)> = widgets
        .iter()
        .filter(|(_, node, _, visibility)| !node.is_empty() && visibility.get())
        .map(|(entity, node, transform, _)| (entity, transform.translation, node.size().y))
        .collect();
    areas.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));

    let mut order = Vec::with_capacity(areas.len());
    let mut rest = areas.as_slice();
    while let Some(&(_, first_center, first_height)) = rest.first() {
        let bottom = first_center.y + first_height / 2.0;
        let len = rest
            .iter()
            .position(|(_, center, _)| center.y > bottom)
            .unwrap_or(rest.len());
        let mut row = rest[..len].to_vec();
        row.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        order.extend(row.into_iter().map(|(entity, _, _)| entity));
        rest = &rest[len..];
    }
    order
}

type NavigableWidget = (
    Entity,
    &'static ComputedNode,
    &'static UiGlobalTransform,
    &'static InheritedVisibility,
);

/// A direction when the stick is first pushed, then repeatedly while it stays pushed.
fn stick_direction(input: &MenuInput, stick: &mut StickRepeat, now: f64) -> Option<CompassOctant> {
    let lean = input
        .gamepads
        .iter()
        .map(Gamepad::left_stick)
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
        .unwrap_or(Vec2::ZERO);
    let held = if lean.length() < STICK_THRESHOLD {
        None
    } else if lean.x.abs() > lean.y.abs() {
        Some(if lean.x > 0.0 {
            CompassOctant::East
        } else {
            CompassOctant::West
        })
    } else {
        Some(if lean.y > 0.0 {
            CompassOctant::North
        } else {
            CompassOctant::South
        })
    };

    if held != stick.held {
        stick.held = held;
        stick.next_at = now + REPEAT_DELAY;
        return held;
    }
    if held.is_some() && now >= stick.next_at {
        stick.next_at = now + REPEAT_INTERVAL;
        return held;
    }
    None
}
