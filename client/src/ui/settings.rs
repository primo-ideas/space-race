//! The settings, in a dialog over the first screen: which key and which gamepad button does what,
//! and whether anything vibrates. On a touchscreen, where there are no keys, it holds what thumbs
//! care about instead: which side the steering is on, how large the controls are, and how far the
//! thumb rolls for full lock.
//!
//! A binding is changed by picking it and pressing the key or the button it should be: the dialog
//! listens for the next one, and the menus stop answering meanwhile, so the arrows, Space and the
//! gamepad's own A can all be bound. Escape, or Start on a gamepad, gives up; Backspace or Delete
//! leaves the place empty. A key does one thing, so binding it takes it from whatever it did.
//! Everything is written out as it changes (see [`crate::preferences`]) and is in force from the
//! next lobby entered.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::navigation::MenuInput;
use super::theme::{self, Fonts};
use super::touch::{TouchOptions, Touchscreen};
use super::widgets::{ButtonKind, ButtonLabel, button, hint, hint_row, panel};
use crate::bindings::{Action, Bindable, Bindings, Input};
use crate::preferences::{Preferences, StoredPreferences};
use crate::screen::Screen;
use crate::settings::Script;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsDialog>()
            .init_resource::<Listening>()
            .add_systems(OnEnter(Screen::Login), open_scripted)
            .add_systems(OnExit(Screen::Login), close)
            .add_systems(
                Update,
                (listen, close_on_back, show, refresh)
                    .chain()
                    .run_if(in_state(Screen::Login)),
            );
    }
}

/// Whether the dialog is up.
#[derive(Resource, Default)]
pub struct SettingsDialog {
    pub open: bool,
}

/// The binding waiting for the next key or button, if one is. While it waits, the menus do not
/// answer (see `navigation.rs`): what is pressed is the answer.
#[derive(Resource, Default)]
pub struct Listening {
    current: Option<Listen>,
    /// A binding was settled on this frame: the press that settled it is not also "back".
    just_ended: bool,
}

struct Listen {
    action: Action,
    input: Input,
    /// The button that was picked, to give the focus back to.
    button: Entity,
    /// Not on the frame it starts: the press that picked the binding is still down.
    armed: bool,
}

impl Listening {
    pub fn active(&self) -> bool {
        self.current.is_some()
    }
}

#[derive(Component)]
struct SettingsRoot;

/// A place in the table of bindings.
#[derive(Component, Clone, Copy)]
struct BindButton {
    action: Action,
    input: Input,
}

/// A choice changed by pressing it: it moves on to its next value.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Vibration,
    Side,
    Size,
    Steering,
}

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Self::Vibration => "Vibration",
            Self::Side => "Steering thumb",
            Self::Size => "Size of the controls",
            Self::Steering => "Steering",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Vibration => "On hits, drifts and boosts.",
            Self::Side => "The other thumb accelerates.",
            Self::Size => "How far the thumbs travel.",
            Self::Steering => "How far the thumb rolls for full lock.",
        }
    }

    fn value(self, preferences: &Preferences) -> &'static str {
        match self {
            Self::Vibration if preferences.vibration => "ON",
            Self::Vibration => "OFF",
            Self::Side if preferences.touch.swapped => "RIGHT",
            Self::Side => "LEFT",
            Self::Size => preferences.touch.size.label(),
            Self::Steering => preferences.touch.steering.label(),
        }
    }

    fn step(self, preferences: &mut Preferences) {
        match self {
            Self::Vibration => preferences.vibration = !preferences.vibration,
            Self::Side => preferences.touch.swapped = !preferences.touch.swapped,
            Self::Size => preferences.touch.size = preferences.touch.size.next(),
            Self::Steering => preferences.touch.steering = preferences.touch.steering.next(),
        }
    }
}

/// The line under the table that says what to do next.
#[derive(Component)]
struct Help;

#[derive(Component)]
struct CloseButton;

/// `--settings` opens the dialog at once, for unattended looks at it.
fn open_scripted(mut script: ResMut<Script>, mut dialog: ResMut<SettingsDialog>) {
    if std::mem::take(&mut script.settings) {
        dialog.open = true;
    }
}

fn close(mut dialog: ResMut<SettingsDialog>, mut listening: ResMut<Listening>) {
    dialog.open = false;
    *listening = Listening::default();
}

fn close_on_back(input: MenuInput, mut dialog: ResMut<SettingsDialog>, listening: Res<Listening>) {
    if dialog.open && !listening.active() && !listening.just_ended && input.back() {
        dialog.open = false;
    }
}

fn close_button(
    _: On<Activate>,
    mut dialog: ResMut<SettingsDialog>,
    mut listening: ResMut<Listening>,
) {
    dialog.open = false;
    *listening = Listening::default();
}

fn reset(
    _: On<Activate>,
    mut preferences: ResMut<StoredPreferences>,
    mut listening: ResMut<Listening>,
) {
    *listening = Listening::default();
    preferences.update(|preferences| {
        preferences.controls = Bindings::default();
        preferences.touch = TouchOptions::default();
        preferences.vibration = true;
    });
}

fn start_listening(
    activate: On<Activate>,
    buttons: Query<&BindButton>,
    mut listening: ResMut<Listening>,
    mut focus: ResMut<InputFocus>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    listening.current = Some(Listen {
        action: button.action,
        input: button.input,
        button: activate.entity,
        armed: false,
    });
    // Nothing has the focus while the dialog listens: a focused button would take Enter and
    // Space for itself, and Space is there to be bound.
    focus.clear();
}

fn step_choice(
    activate: On<Activate>,
    choices: Query<&Choice>,
    mut preferences: ResMut<StoredPreferences>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        preferences.update(|preferences| choice.step(preferences));
    }
}

/// What the next press means to a binding that is listening.
#[derive(Debug, PartialEq)]
enum Heard {
    Key(KeyCode),
    Button(GamepadButton),
    Unbind,
    GiveUp,
}

/// What these presses say to a binding listening for `input`: a key for a key, a button for a
/// button, and what gives up or unbinds from either device. Anything else is not heard at all,
/// and the dialog goes on listening.
fn hear(
    input: Input,
    keys: impl Iterator<Item = KeyCode>,
    buttons: impl Iterator<Item = GamepadButton>,
) -> Option<Heard> {
    for key in keys {
        match key {
            KeyCode::Escape => return Some(Heard::GiveUp),
            KeyCode::Backspace | KeyCode::Delete => return Some(Heard::Unbind),
            key if input != Input::Gamepad && key.bindable() => return Some(Heard::Key(key)),
            _ => {}
        }
    }
    for button in buttons {
        match button {
            GamepadButton::Start => return Some(Heard::GiveUp),
            button if input == Input::Gamepad && button.bindable() => {
                return Some(Heard::Button(button));
            }
            _ => {}
        }
    }
    None
}

/// Takes the next key or button for the binding that is waiting for one.
fn listen(
    mut listening: ResMut<Listening>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut preferences: ResMut<StoredPreferences>,
    mut focus: ResMut<InputFocus>,
) {
    listening.just_ended = false;
    let Some(listen) = &mut listening.current else {
        return;
    };
    if !listen.armed {
        listen.armed = true;
        return;
    }
    let heard = hear(
        listen.input,
        keyboard.get_just_pressed().copied(),
        gamepads
            .iter()
            .flat_map(|gamepad| gamepad.get_just_pressed().copied()),
    );
    let Some(heard) = heard else {
        return;
    };
    let (action, input, button) = (listen.action, listen.input, listen.button);
    match heard {
        Heard::Key(key) => preferences.update(|preferences| {
            preferences
                .controls
                .bind_key(action, input == Input::Alternate, key);
        }),
        Heard::Button(pressed) => {
            preferences.update(|preferences| preferences.controls.bind_button(action, pressed));
        }
        Heard::Unbind => preferences.update(|preferences| preferences.controls.clear(action, input)),
        Heard::GiveUp => {}
    }
    listening.current = None;
    listening.just_ended = true;
    focus.set(button, FocusCause::Navigated);
}

/// Draws the dialog when it opens, and takes it down when it closes.
fn show(
    mut commands: Commands,
    dialog: Res<SettingsDialog>,
    fonts: Res<Fonts>,
    touchscreen: Res<Touchscreen>,
    roots: Query<Entity, With<SettingsRoot>>,
    mut focus: ResMut<InputFocus>,
    mut opener: Local<Option<Entity>>,
) {
    match (dialog.open, roots.single()) {
        (true, Err(_)) => {
            *opener = focus.get();
            let first = spawn_dialog(&mut commands, &fonts, *touchscreen);
            focus.set(first, FocusCause::Navigated);
        }
        (false, Ok(root)) => {
            commands.entity(root).despawn();
            // Back to the button that opened it, so a gamepad carries on from there.
            match opener.take() {
                Some(opener) if commands.get_entity(opener).is_ok() => {
                    focus.set(opener, FocusCause::Navigated);
                }
                _ => focus.clear(),
            }
        }
        _ => {}
    }
}

/// Spawns the dialog and returns what takes the focus first.
fn spawn_dialog(commands: &mut Commands, fonts: &Fonts, touchscreen: Touchscreen) -> Entity {
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    let card = commands
        .spawn((
            Node {
                width: px(pick(620.0, 470.0)),
                flex_direction: FlexDirection::Column,
                padding: px(pick(24.0, 12.0)).all(),
                row_gap: px(pick(6.0, 5.0)),
                border: px(1).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                ..default()
            },
            panel(),
        ))
        // Nearly opaque: the first screen behind it is busy, and a table is read, not glanced at.
        .insert(BackgroundColor(Color::srgba(0.015, 0.03, 0.06, 0.97)))
        .id();
    commands.entity(card).with_child((
        Text::new("SETTINGS"),
        fonts.display(pick(36.0, 26.0)),
        TextColor(theme::TEXT),
        TextShadow {
            offset: Vec2::new(3.0, 3.0),
            color: theme::NEON.with_alpha(0.7),
        },
        Node {
            margin: UiRect::bottom(px(pick(6.0, 0.0))),
            ..default()
        },
    ));

    let mut first = None;
    if touchscreen.0 {
        // No keys on a phone: what its thumbs care about.
        for choice in [Choice::Side, Choice::Size, Choice::Steering, Choice::Vibration] {
            let row = choice_row(commands, fonts, choice, touchscreen);
            first.get_or_insert(row.1);
            commands.entity(card).add_child(row.0);
        }
    } else {
        let caption = |text: &'static str, width: Val| {
            (
                Text::new(text),
                fonts.caption(12.5),
                theme::CAPTION_SPACING,
                TextColor(theme::TEXT_FAINT),
                TextLayout::justify(Justify::Center),
                Node { width, ..default() },
            )
        };
        commands.entity(card).with_children(|card| {
            card.spawn(Node {
                column_gap: px(8),
                align_items: AlignItems::End,
                ..default()
            })
            .with_children(|header| {
                header.spawn((
                    Text::new("CONTROLS"),
                    fonts.caption(12.5),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                header.spawn(caption("KEY", px(BIND_WIDTH)));
                header.spawn(caption("OR", px(BIND_WIDTH)));
                header.spawn(caption("GAMEPAD", px(BIND_WIDTH)));
            });
        });
        for action in Action::ALL {
            let row = commands
                .spawn(Node {
                    column_gap: px(8),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .id();
            commands.entity(row).with_child((
                Text::new(action.label()),
                fonts.text(16.5, 600),
                TextColor(theme::TEXT),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            for input in Input::ALL {
                let bind = commands
                    .spawn((
                        BindButton { action, input },
                        button(fonts, ButtonKind::Secondary, ""),
                    ))
                    .insert(Node {
                        width: px(BIND_WIDTH),
                        height: px(36),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: px(1.5).all(),
                        border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                        ..default()
                    })
                    .observe(start_listening)
                    .id();
                first.get_or_insert(bind);
                commands.entity(row).add_child(bind);
            }
            commands.entity(card).add_child(row);
        }
        commands.entity(card).with_children(|card| {
            // In a box of its own height, so the card does not jump as the line changes.
            card.spawn(Node {
                min_height: px(38),
                margin: UiRect::vertical(px(4)),
                ..default()
            })
            .with_child((
                Help,
                Text::default(),
                fonts.text(13.5, 400),
                TextColor(theme::TEXT_DIM),
            ));
        });
        let row = choice_row(commands, fonts, Choice::Vibration, touchscreen);
        commands.entity(card).add_child(row.0);
    }

    commands.entity(card).with_children(|card| {
        card.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            margin: UiRect::top(px(pick(10.0, 3.0))),
            ..default()
        })
        .with_children(|buttons| {
            buttons
                .spawn(button(fonts, ButtonKind::Quiet, "RESET TO DEFAULTS"))
                .observe(reset);
            buttons
                .spawn((CloseButton, button(fonts, ButtonKind::Primary, "CLOSE")))
                .observe(close_button);
        });
    });

    commands
        .spawn((
            SettingsRoot,
            DespawnOnExit(Screen::Login),
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                ..default()
            },
            BackgroundColor(theme::BACKDROP),
        ))
        .add_child(card)
        .with_children(|root| {
            root.spawn((
                hint_row(),
                children![
                    hint(fonts, "Enter", "A", "Change"),
                    hint(fonts, "Backspace", "Start", "Unbind / cancel"),
                    hint(fonts, "Esc", "B", "Close"),
                ],
            ));
        });
    first.expect("the dialog has something to pick")
}

/// How wide a place in the table of bindings is: "D-pad" and its arrow fit.
const BIND_WIDTH: f32 = 118.0;

/// A row naming a choice, with the button that shows its value and steps it. Returns the row and
/// its button.
fn choice_row(
    commands: &mut Commands,
    fonts: &Fonts,
    choice: Choice,
    touchscreen: Touchscreen,
) -> (Entity, Entity) {
    let value = commands
        .spawn((choice, button(fonts, ButtonKind::Secondary, "")))
        .insert(Node {
            width: px(touchscreen.pick(BIND_WIDTH, 150.0)),
            height: px(36),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: px(1.5).all(),
            border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
            ..default()
        })
        .observe(step_choice)
        .id();
    let row = commands
        .spawn(Node {
            column_gap: px(12),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_child((
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                ..default()
            },
            children![
                (
                    Text::new(choice.label()),
                    fonts.text(16.5, 600),
                    TextColor(theme::TEXT),
                ),
                (
                    Text::new(choice.help()),
                    fonts.text(13.0, 400),
                    TextColor(theme::TEXT_FAINT),
                ),
            ],
        ))
        .add_child(value)
        .id();
    (row, value)
}

/// Keeps what the dialog shows in step with what is stored, and with what it is listening for.
fn refresh(
    preferences: Res<StoredPreferences>,
    listening: Res<Listening>,
    binds: Query<(&BindButton, &Children)>,
    choices: Query<(&Choice, &Children)>,
    mut labels: Query<&mut Text, (With<ButtonLabel>, Without<Help>)>,
    mut help: Query<(&mut Text, &mut TextColor), With<Help>>,
) {
    let preferences = preferences.get();
    let waiting = listening.current.as_ref();
    let mut set = |children: &Children, value: &str| {
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child)
                && text.0 != value
            {
                text.0 = value.to_owned();
            }
        }
    };
    for (bind, children) in &binds {
        let listening_here =
            waiting.is_some_and(|listen| listen.action == bind.action && listen.input == bind.input);
        let label = if listening_here {
            "PRESS\u{2026}"
        } else {
            preferences.controls.of(bind.action).label(bind.input)
        };
        set(children, label);
    }
    for (choice, children) in &choices {
        set(children, choice.value(preferences));
    }

    let (line, color) = match waiting {
        Some(listen) if listen.input == Input::Gamepad => (
            format!(
                "Press the gamepad button for {}. Start gives up, Backspace leaves it empty.",
                listen.action.label().to_lowercase()
            ),
            theme::NEON,
        ),
        Some(listen) => (
            format!(
                "Press the key for {}. Esc gives up, Backspace leaves it empty.",
                listen.action.label().to_lowercase()
            ),
            theme::NEON,
        ),
        None => (
            "Pick a place, then press the key or the button it should be. The left stick \
             always steers."
                .to_owned(),
            theme::TEXT_DIM,
        ),
    };
    for (mut text, mut text_color) in &mut help {
        if text.0 != line {
            text.0 = line.clone();
        }
        text_color.0 = color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(keys: &[KeyCode]) -> impl Iterator<Item = KeyCode> {
        keys.to_vec().into_iter()
    }

    fn buttons(buttons: &[GamepadButton]) -> impl Iterator<Item = GamepadButton> {
        buttons.to_vec().into_iter()
    }

    #[test]
    fn a_key_is_heard_for_a_key_and_a_button_for_a_button() {
        assert_eq!(
            hear(Input::Key, keys(&[KeyCode::KeyQ]), buttons(&[])),
            Some(Heard::Key(KeyCode::KeyQ))
        );
        assert_eq!(
            hear(Input::Gamepad, keys(&[]), buttons(&[GamepadButton::West])),
            Some(Heard::Button(GamepadButton::West))
        );
        // The other device is not an answer, and the dialog goes on listening.
        assert_eq!(
            hear(Input::Gamepad, keys(&[KeyCode::KeyQ]), buttons(&[])),
            None
        );
        assert_eq!(
            hear(Input::Alternate, keys(&[]), buttons(&[GamepadButton::West])),
            None
        );
    }

    /// What works the menus everywhere else can be bound here: the dialog is listening.
    #[test]
    fn the_keys_of_the_menus_can_be_bound() {
        for key in [KeyCode::Space, KeyCode::ArrowLeft] {
            assert_eq!(
                hear(Input::Key, keys(&[key]), buttons(&[])),
                Some(Heard::Key(key))
            );
        }
        assert_eq!(
            hear(Input::Gamepad, keys(&[]), buttons(&[GamepadButton::South])),
            Some(Heard::Button(GamepadButton::South))
        );
    }

    #[test]
    fn escape_and_start_give_up_and_backspace_unbinds() {
        assert_eq!(
            hear(Input::Key, keys(&[KeyCode::Escape]), buttons(&[])),
            Some(Heard::GiveUp)
        );
        assert_eq!(
            hear(Input::Gamepad, keys(&[]), buttons(&[GamepadButton::Start])),
            Some(Heard::GiveUp)
        );
        assert_eq!(
            hear(Input::Gamepad, keys(&[KeyCode::Backspace]), buttons(&[])),
            Some(Heard::Unbind)
        );
        // What the game keeps for itself is not heard.
        assert_eq!(hear(Input::Key, keys(&[KeyCode::F3]), buttons(&[])), None);
    }

    #[test]
    fn a_choice_steps_through_its_values_and_comes_round() {
        let mut preferences = Preferences::default();
        assert_eq!(Choice::Vibration.value(&preferences), "ON");
        Choice::Vibration.step(&mut preferences);
        assert_eq!(Choice::Vibration.value(&preferences), "OFF");

        assert_eq!(Choice::Side.value(&preferences), "LEFT");
        Choice::Side.step(&mut preferences);
        assert_eq!(Choice::Side.value(&preferences), "RIGHT");

        let first = Choice::Size.value(&preferences);
        for _ in 0..3 {
            Choice::Size.step(&mut preferences);
        }
        assert_eq!(Choice::Size.value(&preferences), first);
    }
}
