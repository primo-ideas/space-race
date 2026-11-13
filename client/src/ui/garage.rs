//! The garage: where the player picks their pod and paints it.
//!
//! Reached from the first screen, connected or not. The pod stands over the menu's neon grid,
//! turning slowly, to the right of a card with three choices: which pod, its paint and its neon,
//! each stepped through with left and right, the arrows beside it, or a click. What is chosen is
//! kept with the menu choices and sent to the server, which shows it to everyone in the next lobby
//! the player enters. Every pod drives the same: this is how a car looks, never how it handles.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use star_racer_protocol::{CAR_MODELS, CAR_NEONS, CAR_PAINTS, CarLook};

use super::navigation::MenuInput;
use super::safe_area::SafePadding;
use super::theme::{self, Fonts};
use super::touch::Touchscreen;
use super::widgets::{ButtonKind, Selectable, Stepper, button, hint, hint_row, panel, step_button};
use crate::car_model::{CarModel, NEONS, PAINTS};
use crate::net::Network;
use crate::preferences::{LookPreference, StoredPreferences};
use crate::race::{CarAssets, spawn_car};
use crate::screen::Screen;
use crate::settings::Script;

/// Where the pod stands in front of the menu camera, which `backdrop.rs` places at 3 m over the
/// grid looking far ahead: a little to the right of the card, close enough to fill the screen.
const PREVIEW_AT: Vec3 = Vec3::new(2.6, 0.0, -8.5);
/// How fast it turns, in radians per second.
const TURN_RATE: f32 = 0.45;

pub struct GaragePlugin;

impl Plugin for GaragePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Login), open_scripted)
            .add_systems(OnEnter(Screen::Garage), spawn_garage)
            .add_systems(
                Update,
                (show_choices, show_preview, turn_preview, leave)
                    .chain()
                    .run_if(in_state(Screen::Garage)),
            );
    }
}

/// Which part of the look a row steps through.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Model,
    Paint,
    Neon,
}

/// The name of what a row holds.
#[derive(Component)]
struct ChoiceName(Part);

/// The swatch of a paint or neon row.
#[derive(Component)]
struct Swatch(Part);

/// The line describing the pod.
#[derive(Component)]
struct Blurb;

/// The pod turning beside the card, and the look it is drawn in.
#[derive(Component)]
struct Preview(CarLook);

/// The light over the pod, so its paint shows against the night.
#[derive(Component)]
struct PreviewLight;

/// The look as the rows now say it.
fn look_of(rows: &Query<(&Part, &Stepper)>) -> CarLook {
    let mut look = CarLook::default();
    for (part, stepper) in rows {
        match part {
            Part::Model => look.model = stepper.value,
            Part::Paint => look.paint = stepper.value,
            Part::Neon => look.neon = stepper.value,
        }
    }
    look.checked()
}

fn spawn_garage(
    mut commands: Commands,
    fonts: Res<Fonts>,
    preferences: Res<StoredPreferences>,
    touchscreen: Res<Touchscreen>,
    mut focus: ResMut<InputFocus>,
) {
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    let look = preferences.get().car_look();

    let card = commands
        .spawn((
            Node {
                width: px(pick(420.0, 360.0)),
                flex_direction: FlexDirection::Column,
                padding: px(pick(24.0, 12.0)).all(),
                row_gap: px(pick(12.0, 6.0)),
                border: px(1).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                ..default()
            },
            panel(),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new("GARAGE"),
                fonts.display(pick(44.0, 30.0)),
                TextColor(theme::TEXT),
                TextShadow {
                    offset: Vec2::new(3.0, 3.0),
                    color: theme::NEON.with_alpha(0.8),
                },
            ));
            card.spawn((
                Blurb,
                Text::default(),
                fonts.text(14.0, 400),
                TextColor(theme::TEXT_DIM),
                Node {
                    min_height: px(pick(20.0, 16.0)),
                    ..default()
                },
            ));
        })
        .id();

    let mut first_row = None;
    for (part, label, value, count) in [
        (Part::Model, "POD", look.model, CAR_MODELS),
        (Part::Paint, "PAINT", look.paint, CAR_PAINTS),
        (Part::Neon, "NEON", look.neon, CAR_NEONS),
    ] {
        let row = commands
            .spawn((
                part,
                Stepper {
                    value,
                    min: 0,
                    max: count - 1,
                    wrap: true,
                },
                Selectable,
                Node {
                    padding: UiRect::axes(px(14), px(pick(8.0, 4.0))),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    border: px(1.5).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                    ..default()
                },
            ))
            .id();
        first_row.get_or_insert(row);
        commands.entity(row).with_children(|content| {
            content.spawn((
                Text::new(label),
                fonts.caption(13.0),
                theme::CAPTION_SPACING,
                TextColor(theme::TEXT_FAINT),
                Node {
                    width: px(56),
                    ..default()
                },
            ));
            content.spawn(step_button(&fonts, row, -1));
            content
                .spawn(Node {
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|middle| {
                    if part != Part::Model {
                        middle.spawn((
                            Swatch(part),
                            Node {
                                width: px(18),
                                height: px(18),
                                border: px(1.5).all(),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(theme::PANEL_BORDER),
                        ));
                    }
                    middle.spawn((
                        ChoiceName(part),
                        Text::default(),
                        fonts.text(19.0, 700),
                        TextColor(theme::TEXT),
                    ));
                });
            content.spawn(step_button(&fonts, row, 1));
        });
        commands.entity(card).add_child(row);
    }

    commands.entity(card).with_children(|card| {
        card.spawn((
            Text::new("Every pod drives the same: it is how yours looks."),
            fonts.text(13.0, 400),
            TextColor(theme::TEXT_FAINT),
        ));
        card.spawn(button(&fonts, ButtonKind::Primary, "DONE"))
            .insert(Node {
                height: px(pick(52.0, 44.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: px(1.5).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                margin: UiRect::top(px(pick(6.0, 2.0))),
                ..default()
            })
            .observe(done);
    });

    commands
        .spawn((
            DespawnOnExit(Screen::Garage),
            SafePadding::default(),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::left(px(pick(70.0, 24.0))),
                ..default()
            },
        ))
        .add_child(card)
        .with_children(|root| {
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: px(28),
                    bottom: px(22),
                    display: touchscreen.pick(Display::Flex, Display::None),
                    ..default()
                },
                children![(
                    hint_row(),
                    children![
                        hint(&fonts, "↑ ↓", "D-pad", "Choose"),
                        hint(&fonts, "← →", "D-pad", "Change"),
                        hint(&fonts, "Esc", "B", "Done"),
                    ],
                )],
            ));
        });

    commands.spawn((
        PreviewLight,
        DespawnOnExit(Screen::Garage),
        PointLight {
            intensity: 250_000.0,
            range: 30.0,
            color: Color::srgb(0.85, 0.9, 1.0),
            ..default()
        },
        Transform::from_translation(PREVIEW_AT + Vec3::new(-2.0, 5.0, 4.0)),
    ));

    if let Some(row) = first_row {
        focus.set(row, FocusCause::Navigated);
    }
}

/// `--garage` opens it at once, for unattended looks at it.
fn open_scripted(mut script: ResMut<Script>, mut next: ResMut<NextState<Screen>>) {
    if std::mem::take(&mut script.garage) {
        next.set(Screen::Garage);
    }
}

/// Keeps the choice and tells the server, then goes back to the first screen.
fn done(
    _: On<Activate>,
    rows: Query<(&Part, &Stepper)>,
    mut preferences: ResMut<StoredPreferences>,
    mut network: ResMut<Network>,
    mut next: ResMut<NextState<Screen>>,
) {
    keep(look_of(&rows), &mut preferences, &mut network);
    next.set(Screen::Login);
}

fn keep(look: CarLook, preferences: &mut StoredPreferences, network: &mut Network) {
    if preferences.get().car_look() != look {
        preferences.update(|chosen| chosen.look = Some(LookPreference::of(look)));
    }
    network.set_look(look);
}

/// Back, from the keyboard or a gamepad, is DONE: what is chosen is kept.
fn leave(
    input: MenuInput,
    rows: Query<(&Part, &Stepper)>,
    mut preferences: ResMut<StoredPreferences>,
    mut network: ResMut<Network>,
    mut next: ResMut<NextState<Screen>>,
) {
    if input.back() {
        keep(look_of(&rows), &mut preferences, &mut network);
        next.set(Screen::Login);
    }
}

/// Writes what each row holds, its swatch, and the pod's line. The steppers step themselves (see
/// `widgets.rs` and `navigation.rs`).
fn show_choices(
    rows: Query<(&Part, &Stepper)>,
    mut names: Query<(&ChoiceName, &mut Text), Without<Blurb>>,
    mut swatches: Query<(&Swatch, &mut BackgroundColor)>,
    mut blurbs: Query<&mut Text, With<Blurb>>,
) {
    let look = look_of(&rows);
    let model = CarModel::of(look.model);
    let paint = PAINTS[usize::from(look.paint)];
    let neon = NEONS[usize::from(look.neon)];
    for (name, mut text) in &mut names {
        let wanted = match name.0 {
            Part::Model => model.name(),
            Part::Paint => paint.0,
            Part::Neon => neon.0,
        };
        if text.0 != wanted {
            text.0 = wanted.to_owned();
        }
    }
    for (swatch, mut color) in &mut swatches {
        // The neon as it would be seen, not brighter than white.
        let wanted = match swatch.0 {
            Part::Paint => paint.1,
            Part::Neon | Part::Model => {
                let max = neon.1.red.max(neon.1.green).max(neon.1.blue);
                Color::LinearRgba(neon.1 * (1.0 / max)).with_alpha(1.0)
            }
        };
        color.0 = wanted;
    }
    for mut text in &mut blurbs {
        if text.0 != model.blurb() {
            text.0 = model.blurb().to_owned();
        }
    }
}

/// Draws the pod in the look chosen, again whenever it changes, keeping its turn.
fn show_preview(
    mut commands: Commands,
    rows: Query<(&Part, &Stepper)>,
    previews: Query<(Entity, &Preview, &Transform)>,
    mut assets: ResMut<CarAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let look = look_of(&rows);
    let mut rotation = Quat::from_rotation_y(-0.6);
    for (entity, preview, transform) in &previews {
        if preview.0 == look {
            return;
        }
        rotation = transform.rotation;
        commands.entity(entity).despawn();
    }
    spawn_car(
        &mut commands,
        &mut assets,
        &mut meshes,
        look,
        None,
        false,
        (
            Preview(look),
            DespawnOnExit(Screen::Garage),
            Transform::from_translation(PREVIEW_AT).with_rotation(rotation),
            Visibility::default(),
        ),
    );
}

fn turn_preview(time: Res<Time>, mut previews: Query<&mut Transform, With<Preview>>) {
    for mut transform in &mut previews {
        transform.rotate_y(TURN_RATE * time.delta_secs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows step round: past the last pod comes the first.
    #[test]
    fn the_choices_wrap_round() {
        let mut stepper = Stepper {
            value: CAR_MODELS - 1,
            min: 0,
            max: CAR_MODELS - 1,
            wrap: true,
        };
        stepper.step(1);
        assert_eq!(stepper.value, 0);
        stepper.step(-1);
        assert_eq!(stepper.value, CAR_MODELS - 1);
    }
}
