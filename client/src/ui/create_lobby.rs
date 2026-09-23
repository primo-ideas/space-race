//! The lobby creation dialog: a name, a track, the laps, and how many players it takes.
//!
//! Everything comes with a default, so creating a lobby can take a single confirmation. Every
//! setting but the name is a stepper, changed with left and right, so the whole dialog works with a
//! gamepad. The dialog opens on what the player created their last lobby with (see
//! [`crate::preferences`]).

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use space_race_protocol::{
    ClientMessage, LobbySettings, MAX_LAPS, MAX_LOBBY_NAME_CHARS, MAX_LOBBY_PLAYERS,
    is_valid_lobby_name,
};

use super::lobbies::BrowserSystems;
use super::navigation::MenuInput;
use super::theme::{self, Fonts};
use super::touch::Touchscreen;
use super::widgets::{
    ButtonKind, ButtonLabel, Selectable, Stepper, StepperValue, button, hint, hint_row, panel,
    step_button, text_field,
};
use crate::lobby::{Notice, Tracks};
use crate::net::{Connection, NetEvent, Network};
use crate::preferences::{DEFAULT_TRACK, LobbyPreferences, StoredPreferences};
use crate::screen::Screen;
use crate::settings::Script;

pub struct CreateLobbyPlugin;

impl Plugin for CreateLobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreateDialog>()
            .add_systems(OnEnter(Screen::Lobbies), open_scripted)
            .add_systems(OnExit(Screen::Lobbies), close)
            .add_systems(
                Update,
                (show_dialog, update_dialog, dialog_shortcuts)
                    .chain()
                    .after(BrowserSystems)
                    .run_if(in_state(Screen::Lobbies)),
            );
    }
}

#[derive(Resource, Default)]
pub struct CreateDialog {
    pub open: bool,
    /// The creation request is on its way.
    creating: bool,
    /// The lobby name the dialog offered when it opened. Confirming it as it stands is not a
    /// choice to remember, any more than playing under a generated nickname is.
    offered_name: String,
}

#[derive(Component)]
struct DialogRoot;

#[derive(Component)]
struct NameField;

#[derive(Component)]
struct NameHelp;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Track,
    Laps,
    MinPlayers,
    MaxPlayers,
}

#[derive(Component)]
enum TrackPreview {
    Thumbnail,
    Name,
    Length,
}

#[derive(Component)]
struct ConfirmButton;

#[derive(Component)]
struct DialogNotice;

fn open_scripted(mut script: ResMut<Script>, mut dialog: ResMut<CreateDialog>) {
    if std::mem::take(&mut script.create_dialog) {
        dialog.open = true;
    }
}

fn close(mut dialog: ResMut<CreateDialog>) {
    *dialog = CreateDialog::default();
}

fn show_dialog(
    mut commands: Commands,
    mut dialog: ResMut<CreateDialog>,
    roots: Query<Entity, With<DialogRoot>>,
    fonts: Res<Fonts>,
    tracks: Res<Tracks>,
    connection: Res<Connection>,
    preferences: Res<StoredPreferences>,
    touchscreen: Res<Touchscreen>,
    mut focus: ResMut<InputFocus>,
) {
    match (dialog.open, roots.single()) {
        (true, Err(_)) => {
            let (field, offered_name) = spawn_dialog(
                &mut commands,
                &fonts,
                &tracks,
                &connection,
                &preferences,
                *touchscreen,
            );
            dialog.offered_name = offered_name;
            focus.set(field, FocusCause::Navigated);
        }
        (false, Ok(root)) => {
            commands.entity(root).despawn();
            focus.clear();
        }
        _ => {}
    }
}

/// Spawns the dialog and returns the name field, to focus, and the name it offered.
fn spawn_dialog(
    commands: &mut Commands,
    fonts: &Fonts,
    tracks: &Tracks,
    connection: &Connection,
    preferences: &StoredPreferences,
    touchscreen: Touchscreen,
) -> (Entity, String) {
    // On a phone the card has to fit a screen half as tall: it gives up its padding, its title
    // shrinks, and the line explaining each setting goes, the settings being their own
    // explanation.
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    let offered_name = match connection {
        Connection::Online { nickname } => format!("{nickname}'s lobby"),
        _ => "New lobby".into(),
    };
    let offered_name: String = offered_name.chars().take(MAX_LOBBY_NAME_CHARS).collect();
    let chosen = &preferences.get().lobby;
    let initial_name = chosen.name.clone().unwrap_or_else(|| offered_name.clone());
    let track_count = tracks.0.len().max(1) as u8;
    // The track chosen last time, unless the server no longer has it.
    let wanted_track = chosen.track.as_deref().unwrap_or(DEFAULT_TRACK);
    let track_position = |wanted: &str| tracks.0.keys().position(|key| key == wanted);
    let default_track = track_position(wanted_track)
        .or_else(|| track_position(DEFAULT_TRACK))
        .unwrap_or(0) as u8;

    let field = commands
        .spawn((
            NameField,
            text_field(fonts, &initial_name, MAX_LOBBY_NAME_CHARS),
        ))
        .observe(confirm)
        .id();

    let card = commands
        .spawn((
            Node {
                width: px(pick(560.0, 470.0)),
                flex_direction: FlexDirection::Column,
                padding: px(pick(28.0, 15.0)).all(),
                row_gap: px(pick(10.0, 5.0)),
                border: px(1).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                ..default()
            },
            panel(),
        ))
        .id();
    commands.entity(card).with_children(|card| {
        card.spawn((
            Text::new("CREATE A LOBBY"),
            fonts.display(pick(40.0, 28.0)),
            TextColor(theme::TEXT),
            TextShadow {
                offset: Vec2::new(3.0, 3.0),
                color: theme::NEON.with_alpha(0.7),
            },
            Node {
                margin: UiRect::bottom(px(pick(8.0, 2.0))),
                ..default()
            },
        ));
        card.spawn((
            Text::new("NAME"),
            fonts.caption(12.5),
            theme::CAPTION_SPACING,
            TextColor(theme::TEXT_FAINT),
        ));
    });
    commands.entity(card).add_child(field);
    commands.entity(card).with_children(|card| {
        // A minimum height on a text itself inflates the panel around it: it goes on a box.
        card.spawn(Node {
            min_height: px(18),
            margin: UiRect::bottom(px(6)),
            ..default()
        })
        .with_child((
            NameHelp,
            Text::default(),
            fonts.text(13.5, 400),
            TextColor(theme::TEXT_FAINT),
        ));
    });

    let settings = [
        (
            Setting::Track,
            "Track",
            "Where the races take place.",
            Stepper {
                value: default_track,
                min: 0,
                max: track_count - 1,
                wrap: true,
            },
        ),
        (
            Setting::Laps,
            "Laps",
            "The length of each race.",
            Stepper {
                value: chosen.laps,
                min: 1,
                max: MAX_LAPS,
                wrap: false,
            },
        ),
        (
            Setting::MinPlayers,
            "Players to start",
            "A race starts once this many players are in.",
            Stepper {
                value: chosen.min_players,
                min: 1,
                max: MAX_LOBBY_PLAYERS,
                wrap: false,
            },
        ),
        (
            Setting::MaxPlayers,
            "Maximum players",
            "Spectators included.",
            Stepper {
                value: chosen.max_players,
                min: 1,
                max: MAX_LOBBY_PLAYERS,
                wrap: false,
            },
        ),
    ];
    for (setting, label, help, stepper) in settings {
        let initial = stepper.value;
        let row = commands
            .spawn((
                setting,
                stepper,
                Selectable,
                Node {
                    padding: UiRect::axes(px(14), px(pick(8.0, 4.0))),
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    border: px(1.5).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                    ..default()
                },
            ))
            .id();
        commands.entity(row).with_children(|row_content| {
            row_content.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    ..default()
                },
                children![
                    (
                        Text::new(label),
                        fonts.text(16.5, 600),
                        TextColor(theme::TEXT),
                    ),
                    (
                        Text::new(touchscreen.pick(help, "")),
                        fonts.text(13.0, 400),
                        TextColor(theme::TEXT_FAINT),
                    ),
                ],
            ));
            row_content.spawn(step_button(fonts, row, -1));
            if setting == Setting::Track {
                row_content.spawn((
                    Node {
                        width: px(200),
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        ..default()
                    },
                    children![
                        (
                            TrackPreview::Thumbnail,
                            ImageNode::default(),
                            Node {
                                width: px(80),
                                height: px(50),
                                ..default()
                            },
                        ),
                        (
                            Node {
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                            children![
                                (
                                    TrackPreview::Name,
                                    Text::default(),
                                    fonts.text(16.0, 600),
                                    TextColor(theme::TEXT),
                                ),
                                (
                                    TrackPreview::Length,
                                    Text::default(),
                                    fonts.text(13.0, 500),
                                    TextColor(theme::TEXT_DIM),
                                ),
                            ],
                        ),
                    ],
                ));
            } else {
                row_content.spawn((
                    StepperValue(row),
                    Text::new(initial.to_string()),
                    fonts.display(26.0),
                    TextColor(theme::TEXT),
                    TextLayout::justify(Justify::Center),
                    Node {
                        width: px(44),
                        ..default()
                    },
                ));
            }
            row_content.spawn(step_button(fonts, row, 1));
        });
        commands.entity(card).add_child(row);
    }

    commands.entity(card).with_children(|card| {
        card.spawn(Node {
            min_height: px(18),
            margin: UiRect::top(px(4)),
            ..default()
        })
        .with_child((
            DialogNotice,
            Text::default(),
            fonts.text(14.0, 600),
            TextColor(theme::AMBER),
        ));
        card.spawn(Node {
            justify_content: JustifyContent::FlexEnd,
            column_gap: px(12),
            margin: UiRect::top(px(6)),
            ..default()
        })
        .with_children(|buttons| {
            buttons
                .spawn(button(fonts, ButtonKind::Secondary, "CANCEL"))
                .observe(cancel);
            buttons
                .spawn((
                    ConfirmButton,
                    button(fonts, ButtonKind::Primary, "CREATE LOBBY"),
                ))
                .observe(confirm);
        });
    });

    commands
        .spawn((
            DialogRoot,
            DespawnOnExit(Screen::Lobbies),
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
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
                    hint(fonts, "↑ ↓", "D-pad", "Choose"),
                    hint(fonts, "← →", "D-pad", "Change"),
                    hint(fonts, "Enter", "A", "Create"),
                    hint(fonts, "Esc", "B", "Cancel"),
                ],
            ));
        });
    (field, offered_name)
}

fn update_dialog(
    mut commands: Commands,
    mut dialog: ResMut<CreateDialog>,
    mut events: MessageReader<NetEvent>,
    tracks: Res<Tracks>,
    notice: Res<Notice>,
    time: Res<Time<Real>>,
    fields: Query<&EditableText, With<NameField>>,
    mut steppers: Query<(&Setting, &mut Stepper)>,
    mut previews: Query<(&TrackPreview, Option<&mut Text>, Option<&mut ImageNode>)>,
    mut help: Query<(&mut Text, &mut TextColor), (With<NameHelp>, Without<TrackPreview>)>,
    mut notices: Query<&mut Text, (With<DialogNotice>, Without<NameHelp>, Without<TrackPreview>)>,
    confirm_buttons: Query<(Entity, &Children, Has<InteractionDisabled>), With<ConfirmButton>>,
    mut labels: Query<
        &mut Text,
        (
            With<ButtonLabel>,
            Without<DialogNotice>,
            Without<NameHelp>,
            Without<TrackPreview>,
        ),
    >,
) {
    if events
        .read()
        .any(|event| matches!(event, NetEvent::JoinFailed(_)))
    {
        dialog.creating = false;
    }
    let Ok(field) = fields.single() else {
        return;
    };
    let name = field.value().to_string();
    let valid = is_valid_lobby_name(&name);

    // At least as many players allowed as needed: whichever was just changed wins.
    let mut min_players = None;
    let mut max_players = None;
    for (setting, stepper) in &mut steppers {
        match setting {
            Setting::MinPlayers => min_players = Some((stepper.value, stepper.is_changed())),
            Setting::MaxPlayers => max_players = Some((stepper.value, stepper.is_changed())),
            _ => {}
        }
    }
    if let (Some((min, min_changed)), Some((max, _))) = (min_players, max_players)
        && min > max
    {
        for (setting, mut stepper) in &mut steppers {
            match setting {
                Setting::MaxPlayers if min_changed => stepper.value = min,
                Setting::MinPlayers if !min_changed => stepper.value = max,
                _ => {}
            }
        }
    }

    let track = steppers
        .iter()
        .find(|(setting, _)| **setting == Setting::Track)
        .and_then(|(_, stepper)| tracks.0.values().nth(usize::from(stepper.value)));
    if let Some(track) = track {
        for (preview, text, image) in &mut previews {
            match (preview, text, image) {
                (TrackPreview::Thumbnail, _, Some(mut image)) => {
                    if image.image != track.thumbnail {
                        image.image = track.thumbnail.clone();
                    }
                }
                (TrackPreview::Name, Some(mut text), _) => {
                    if text.0 != track.description.name {
                        text.0 = track.description.name.clone();
                    }
                }
                (TrackPreview::Length, Some(mut text), _) => {
                    let length = format!("{:.0} m per lap", track.track.length());
                    if text.0 != length {
                        text.0 = length;
                    }
                }
                _ => {}
            }
        }
    }

    let (help_text, help_color) = if name.is_empty() {
        ("Give the lobby a name.".to_owned(), theme::AMBER)
    } else if name.trim() != name {
        (
            "A name can't start or end with a space.".to_owned(),
            theme::AMBER,
        )
    } else if !valid {
        (
            "A name can't contain control characters.".to_owned(),
            theme::AMBER,
        )
    } else {
        (
            format!("{} / {MAX_LOBBY_NAME_CHARS}", name.chars().count()),
            theme::TEXT_FAINT,
        )
    };
    for (mut text, mut color) in &mut help {
        if text.0 != help_text {
            text.0 = help_text.clone();
        }
        color.0 = help_color;
    }

    let notice = notice
        .current(time.elapsed_secs_f64())
        .unwrap_or_default()
        .to_owned();
    for mut text in &mut notices {
        if text.0 != notice {
            text.0 = notice.clone();
        }
    }

    let enabled = valid && track.is_some() && !dialog.creating;
    let label = if dialog.creating {
        "CREATING…"
    } else {
        "CREATE LOBBY"
    };
    for (button, children, disabled) in &confirm_buttons {
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child)
                && text.0 != label
            {
                text.0 = label.into();
            }
        }
        if enabled && disabled {
            commands.entity(button).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(button).insert(InteractionDisabled);
        }
    }
}

fn dialog_shortcuts(input: MenuInput, mut dialog: ResMut<CreateDialog>) {
    if dialog.open && !dialog.creating && input.back() {
        dialog.open = false;
    }
}

fn cancel(_: On<Activate>, mut dialog: ResMut<CreateDialog>) {
    if !dialog.creating {
        dialog.open = false;
    }
}

fn confirm(
    _: On<Activate>,
    mut dialog: ResMut<CreateDialog>,
    fields: Query<&EditableText, With<NameField>>,
    steppers: Query<(&Setting, &Stepper)>,
    tracks: Res<Tracks>,
    network: Res<Network>,
    mut preferences: ResMut<StoredPreferences>,
) {
    if dialog.creating {
        return;
    }
    let Ok(field) = fields.single() else {
        return;
    };
    let value = |wanted: Setting| {
        steppers
            .iter()
            .find(|(setting, _)| **setting == wanted)
            .map(|(_, stepper)| stepper.value)
    };
    let (Some(track), Some(laps), Some(min_players), Some(max_players)) = (
        value(Setting::Track),
        value(Setting::Laps),
        value(Setting::MinPlayers),
        value(Setting::MaxPlayers),
    ) else {
        return;
    };
    let Some(track) = tracks.0.keys().nth(usize::from(track)) else {
        return;
    };
    let settings = LobbySettings {
        name: field.value().to_string(),
        track: track.clone(),
        laps,
        min_players,
        max_players,
    };
    if !settings.is_valid() {
        return;
    }
    // The next dialog opens on this lobby, minus a name that was only the one offered.
    let chosen = LobbyPreferences::of(&settings, &dialog.offered_name);
    preferences.update(|preferences| preferences.lobby = chosen);
    network.send(ClientMessage::CreateLobby(settings));
    dialog.creating = true;
}
