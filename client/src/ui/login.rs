//! The first screen: the game's title, and a nickname to connect with.
//!
//! The field comes filled in, with the nickname last played under or a generated one, so playing
//! is one press away, even with a gamepad. The dice button, or Y on a gamepad, rolls a new one.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, LetterSpacing};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use space_race_protocol::{MAX_NICKNAME_CHARS, PROTOCOL_VERSION, RejectReason, is_valid_nickname};

use super::navigation::MenuInput;
use super::theme::{self, Fonts};
use super::touch::Touchscreen;
use super::widgets::{ButtonKind, button, hint, hint_row, panel, set_text, text_field};
use crate::net::{Connection, Network};
use crate::nickname;
use crate::preferences::{OfferedNickname, StoredPreferences};
use crate::screen::Screen;
use crate::settings::Script;

pub struct LoginPlugin;

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(Screen::Login),
            (spawn_login, connect_scripted).chain(),
        )
        .add_systems(
            Update,
            (reroll_shortcut, update_login).run_if(in_state(Screen::Login)),
        );
    }
}

#[derive(Component)]
struct NicknameField;

#[derive(Component)]
struct PlayButton;

#[derive(Component)]
struct RerollButton;

#[derive(Component)]
struct CharCount;

#[derive(Component)]
struct FieldHelp;

#[derive(Component)]
struct Status;

fn spawn_login(
    mut commands: Commands,
    fonts: Res<Fonts>,
    network: Res<Network>,
    connection: Res<Connection>,
    preferences: Res<StoredPreferences>,
    touchscreen: Res<Touchscreen>,
    mut offered: ResMut<OfferedNickname>,
    mut focus: ResMut<InputFocus>,
) {
    // A phone screen is short: the title and the card give up the room they can spare.
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    // The nickname of the run being played, then the one the player made theirs, then the dice.
    let initial = match &*connection {
        Connection::Connecting { nickname } | Connection::Online { nickname } => {
            Some(nickname.clone())
        }
        _ => None,
    }
    .or_else(|| preferences.get().nickname.clone())
    .unwrap_or_else(|| {
        let generated = nickname::generate(None);
        offered.0 = Some(generated.clone());
        generated
    });

    let field = commands
        .spawn((
            NicknameField,
            text_field(&fonts, &initial, MAX_NICKNAME_CHARS),
        ))
        .id();

    let card = commands
        .spawn((
            Node {
                width: px(pick(440.0, 380.0)),
                flex_direction: FlexDirection::Column,
                padding: px(pick(28.0, 18.0)).all(),
                row_gap: px(pick(12.0, 9.0)),
                border: px(1).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                ..default()
            },
            panel(),
        ))
        .with_children(|card| {
            card.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new("NICKNAME"),
                    fonts.caption(13.0),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_FAINT),
                ));
                row.spawn((
                    CharCount,
                    Text::default(),
                    fonts.caption(13.0),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_FAINT),
                ));
            });
            card.spawn(Node {
                column_gap: px(10),
                align_items: AlignItems::Stretch,
                ..default()
            })
            .add_child(field)
            .with_children(|row| {
                row.spawn((RerollButton, button(&fonts, ButtonKind::Secondary, "")))
                    .insert(Node {
                        width: px(48),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: px(1.5).all(),
                        border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                        ..default()
                    })
                    .with_child(die())
                    .observe(reroll);
            });
            // Texts that come and go sit in a box of fixed minimum height, so the card does not
            // jump when they change.
            card.spawn(Node {
                min_height: px(20),
                ..default()
            })
            .with_child((
                FieldHelp,
                Text::default(),
                fonts.text(14.0, 400),
                TextColor(theme::TEXT_DIM),
            ));
            card.spawn((PlayButton, button(&fonts, ButtonKind::Primary, "PLAY")))
                .insert(Node {
                    height: px(pick(50.0, 44.0)),
                    margin: UiRect::top(px(6)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: px(1.5).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                    ..default()
                })
                .observe(play);
            card.spawn(Node {
                min_height: px(22),
                margin: UiRect::top(px(4)),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_child((
                Status,
                Text::default(),
                fonts.text(15.0, 500),
                TextColor(theme::TEXT_DIM),
                TextLayout::justify(Justify::Center),
            ));
        })
        .id();

    commands.entity(field).observe(play);

    commands
        .spawn((
            DespawnOnExit(Screen::Login),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.75), percent(0)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.1), percent(55)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.55), percent(100)),
            ])),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("SPACE RACE"),
                fonts.display(pick(92.0, 56.0)),
                TextColor(theme::TEXT),
                TextShadow {
                    offset: Vec2::new(4.0, 4.0),
                    color: theme::NEON.with_alpha(0.8),
                },
            ));
            root.spawn((
                Text::new("ONLINE ARCADE RACING  ·  ALPHA"),
                fonts.caption(15.0),
                LetterSpacing::Px(5.0),
                TextColor(theme::NEON),
                Node {
                    margin: UiRect::bottom(px(pick(46.0, 18.0))),
                    ..default()
                },
            ));
        })
        .add_child(card)
        .with_children(|root| {
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(28),
                    right: px(28),
                    bottom: px(22),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![
                    (
                        Text::new(format!(
                            "{}  ·  protocol {PROTOCOL_VERSION}",
                            network.server()
                        )),
                        fonts.text(13.0, 400),
                        TextColor(theme::TEXT_FAINT),
                    ),
                    (
                        hint_row(),
                        children![
                            hint(&fonts, "Enter", "A", "Play"),
                            hint(&fonts, "F2", "Y", "Other nickname"),
                        ],
                    ),
                ],
            ));
        });

    focus.set(field, FocusCause::Navigated);
}

/// A die, drawn with nodes, for the button that rolls a new nickname.
fn die() -> impl Bundle {
    let pip = |offset: f32| {
        (
            Node {
                position_type: PositionType::Absolute,
                left: px(offset),
                top: px(offset),
                width: px(4),
                height: px(4),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(theme::TEXT),
        )
    };
    (
        Node {
            width: px(21),
            height: px(21),
            border: px(1.5).all(),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BorderColor::all(theme::TEXT),
        children![pip(2.5), pip(7.0), pip(11.5)],
    )
}

fn connect_scripted(
    mut script: ResMut<Script>,
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
) {
    if let Some(nickname) = script.nickname.take() {
        network.connect(nickname, &mut connection);
    }
}

fn reroll(
    _: On<Activate>,
    mut fields: Query<&mut EditableText, With<NicknameField>>,
    connection: Res<Connection>,
    mut offered: ResMut<OfferedNickname>,
) {
    if matches!(*connection, Connection::Connecting { .. }) {
        return;
    }
    for mut field in &mut fields {
        let current = field.value().to_string();
        let generated = nickname::generate(Some(&current));
        offered.0 = Some(generated.clone());
        set_text(&mut field, &generated);
    }
}

fn reroll_shortcut(
    input: MenuInput,
    buttons: Query<Entity, With<RerollButton>>,
    mut commands: Commands,
) {
    if input.key(KeyCode::F2) || input.gamepad(GamepadButton::North) {
        for entity in &buttons {
            commands.trigger(Activate { entity });
        }
    }
}

fn play(
    _: On<Activate>,
    fields: Query<&EditableText, With<NicknameField>>,
    offered: Res<OfferedNickname>,
    mut preferences: ResMut<StoredPreferences>,
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
) {
    if matches!(*connection, Connection::Connecting { .. }) {
        return;
    }
    let Ok(field) = fields.single() else {
        return;
    };
    let nickname = field.value().to_string();
    if !is_valid_nickname(&nickname) {
        return;
    }
    // A nickname the dice offered and the player simply played with is not remembered: they get a
    // new one next time. Editing it makes it theirs.
    let theirs = offered.0.as_deref() != Some(nickname.as_str());
    preferences.update(|preferences| preferences.nickname = theirs.then(|| nickname.clone()));
    network.connect(nickname, &mut connection);
}

fn update_login(
    mut commands: Commands,
    fields: Query<&EditableText, With<NicknameField>>,
    connection: Res<Connection>,
    network: Res<Network>,
    time: Res<Time<Real>>,
    play_buttons: Query<(Entity, Has<InteractionDisabled>), With<PlayButton>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<CharCount>>,
        Query<(&mut Text, &mut TextColor), With<FieldHelp>>,
        Query<(&mut Text, &mut TextColor), With<Status>>,
    )>,
) {
    let Ok(field) = fields.single() else {
        return;
    };
    let nickname = field.value().to_string();
    let chars = nickname.chars().count();
    let valid = is_valid_nickname(&nickname);
    let connecting = matches!(*connection, Connection::Connecting { .. });

    for mut text in &mut texts.p0() {
        set(&mut text, format!("{chars} / {MAX_NICKNAME_CHARS}"));
    }

    let (help, help_color) = if chars == 0 {
        ("Pick a nickname, or roll the dice.", theme::TEXT_DIM)
    } else if nickname.trim() != nickname {
        ("A nickname can't start or end with a space.", theme::AMBER)
    } else if !valid {
        ("A nickname can't contain control characters.", theme::AMBER)
    } else {
        ("This is how other players will see you.", theme::TEXT_FAINT)
    };
    for (mut text, mut color) in &mut texts.p1() {
        set(&mut text, help.to_owned());
        color.0 = help_color;
    }

    let (status, status_color) = match &*connection {
        Connection::Offline | Connection::Online { .. } => (String::new(), theme::TEXT_DIM),
        Connection::Connecting { .. } => {
            let dots = ".".repeat(1 + (time.elapsed_secs() * 3.0) as usize % 3);
            (format!("Connecting{dots}"), theme::NEON)
        }
        Connection::Rejected(reason) => (rejection_text(reason), theme::RED),
        Connection::Disconnected {
            reason,
            was_online: true,
        } => (
            format!("Disconnected from the server: {reason}"),
            theme::RED,
        ),
        Connection::Disconnected {
            was_online: false, ..
        } => (
            format!("Can't reach the server at {}.", network.server()),
            theme::RED,
        ),
    };
    for (mut text, mut color) in &mut texts.p2() {
        set(&mut text, status.clone());
        color.0 = status_color;
    }

    let enabled = valid && !connecting;
    for (button, disabled) in &play_buttons {
        if enabled && disabled {
            commands.entity(button).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(button).insert(InteractionDisabled);
        }
    }
}

/// Changes a text only when it differs, so its layout is not recomputed every frame.
fn set(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

fn rejection_text(reason: &RejectReason) -> String {
    match reason {
        RejectReason::ProtocolMismatch { server_version } if *server_version > PROTOCOL_VERSION => {
            "The server runs a newer version of the game: update yours.".into()
        }
        RejectReason::ProtocolMismatch { .. } => {
            "The server runs an older version of the game.".into()
        }
        RejectReason::InvalidNickname => "The server refused this nickname.".into(),
        RejectReason::AuthenticationFailed => "The server could not verify your identity.".into(),
        RejectReason::AlreadyConnected => "You are already connected, from another window.".into(),
    }
}
