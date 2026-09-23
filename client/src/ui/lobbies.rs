//! The lobby browser: every lobby on the server, live, with the selected one's details beside the
//! list.
//!
//! A lobby is either about to race (waiting for players, cars on the grid, showing results), where
//! joining means racing in the next race, or in a race, where joining means watching it first. The
//! status badge tells them apart at a glance: grey, amber and green for the first, magenta for a
//! race on.

use std::collections::HashMap;

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use space_race_protocol::{ClientMessage, LobbyId, LobbyStatus, LobbySummary, Presence};

use super::create_lobby::CreateDialog;
use super::navigation::MenuInput;
use super::theme::{self, Fonts};
use super::touch::Touchscreen;
use super::widgets::{
    Badge, BadgeDot, BadgeText, ButtonKind, ButtonLabel, Selectable, badge, button, hint, hint_row,
    panel, set_badge,
};
use crate::lobby::{LobbyList, Notice, OnlinePlayers, Tracks};
use crate::net::{Connection, NetEvent, Network};
use crate::screen::Screen;

const TRACK_COLUMN: f32 = 150.0;
const PLAYERS_COLUMN: f32 = 80.0;
const STATUS_COLUMN: f32 = 176.0;
const DETAILS_WIDTH: f32 = 330.0;
/// Two rows of connected players fit under the lobby list; past that the panel clips, and its
/// caption still counts everyone.
const ONLINE_HEIGHT: f32 = 58.0;

pub struct LobbiesPlugin;

impl Plugin for LobbiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selected>()
            .init_resource::<Joining>()
            .add_systems(OnEnter(Screen::Lobbies), spawn_browser)
            .add_systems(
                Update,
                (
                    stop_joining_on_failure,
                    sync_rows,
                    keep_focus,
                    update_details,
                    update_online_players,
                    update_frame,
                    shortcuts,
                )
                    .chain()
                    .in_set(BrowserSystems)
                    .run_if(in_state(Screen::Lobbies)),
            );
    }
}

/// The browser's systems. The creation dialog runs after them: otherwise the Escape that closes the
/// dialog would reach the browser in the same frame, as a request to change nickname.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BrowserSystems;

/// The lobby whose details are shown: the focused row's, or the last one focused.
#[derive(Resource, Default)]
struct Selected(Option<LobbyId>);

/// A join request is on its way: further ones wait for its answer.
#[derive(Resource, Default)]
struct Joining(bool);

#[derive(Component)]
struct BrowserRoot;

#[derive(Component)]
struct RowList;

#[derive(Component)]
struct EmptyList;

#[derive(Component)]
struct LobbyCount;

#[derive(Component)]
struct NoticeText;

#[derive(Component)]
struct CreateButton;

#[derive(Component)]
struct JoinButton;

/// A lobby in the list, and the cells to update.
#[derive(Component)]
struct LobbyRow {
    id: LobbyId,
    name: Entity,
    track: Entity,
    players: Entity,
    badge: Entity,
}

#[derive(Component)]
enum Detail {
    Placeholder,
    Content,
    Name,
    Track,
    Description,
    PlayersCaption,
    Players,
}

/// The caption of the connected players panel, which counts them.
#[derive(Component)]
struct OnlineCaption;

/// Holds one chip per connected player.
#[derive(Component)]
struct OnlineList;

/// The connected players as the panel shows them, so it is rebuilt only when what it would say
/// changes. It lives on the panel and goes with it, unlike a system's own memory, which would
/// outlive the screen and leave the next one empty.
#[derive(Component, Default)]
struct ShownPlayers(Vec<Chip>);

/// One connected player, as a chip: their name, where they are, and the color of that place.
#[derive(Clone, PartialEq)]
struct Chip {
    nickname: String,
    place: String,
    color: Color,
    /// The player looking at the screen, whose name stands out. Two players may have chosen the
    /// same nickname, and then both of them do.
    own: bool,
}

#[derive(Component)]
struct DetailThumbnail;

#[derive(Component)]
struct DetailBadge;

fn spawn_browser(
    mut commands: Commands,
    fonts: Res<Fonts>,
    connection: Res<Connection>,
    touchscreen: Res<Touchscreen>,
    mut selected: ResMut<Selected>,
    mut joining: ResMut<Joining>,
    mut focus: ResMut<InputFocus>,
) {
    // A phone has no room beside the list for the details of one lobby, and no need of them: a
    // row says the track, the players and what the lobby is doing, and tapping it joins.
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    selected.0 = None;
    joining.0 = false;
    focus.clear();
    let nickname = match &*connection {
        Connection::Online { nickname } | Connection::Connecting { nickname } => nickname.clone(),
        _ => String::new(),
    };
    let caption = |text: &str| {
        (
            Text::new(text),
            fonts.caption(12.5),
            theme::CAPTION_SPACING,
            TextColor(theme::TEXT_FAINT),
        )
    };

    commands
        .spawn((
            BrowserRoot,
            DespawnOnExit(Screen::Lobbies),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(pick(36.0, 20.0)), px(pick(26.0, 14.0))),
                row_gap: px(pick(18.0, 10.0)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.85), percent(0)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.35), percent(60)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.7), percent(100)),
            ])),
        ))
        .with_children(|root| {
            // Header: title and count, then the player and the main action.
            root.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(14),
                ..default()
            })
            .with_children(|header| {
                header.spawn((
                    Text::new("LOBBIES"),
                    fonts.display(pick(50.0, 34.0)),
                    TextColor(theme::TEXT),
                    TextShadow {
                        offset: Vec2::new(3.0, 3.0),
                        color: theme::NEON.with_alpha(0.7),
                    },
                ));
                header.spawn((
                    LobbyCount,
                    Text::default(),
                    fonts.text(18.0, 600),
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        margin: UiRect::top(px(10)),
                        ..default()
                    },
                ));
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                header
                    .spawn((
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(8),
                            padding: UiRect::axes(px(12), px(6)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme::SURFACE),
                    ))
                    .with_children(|chip| {
                        chip.spawn((
                            Node {
                                width: px(8),
                                height: px(8),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(theme::OWN),
                        ));
                        chip.spawn((
                            Text::new(nickname),
                            fonts.text(16.0, 600),
                            TextColor(theme::TEXT),
                        ));
                        chip.spawn(button(&fonts, ButtonKind::Quiet, "CHANGE"))
                            .insert(Node {
                                padding: UiRect::axes(px(8), px(2)),
                                border: px(1.5).all(),
                                border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                                ..default()
                            })
                            .observe(change_nickname);
                    });
                header
                    .spawn((
                        CreateButton,
                        button(&fonts, ButtonKind::Primary, "+  CREATE LOBBY"),
                    ))
                    .observe(open_create_dialog);
            });

            // Body: the list with everyone connected under it, and the details of the selected
            // lobby.
            root.spawn(Node {
                flex_grow: 1.0,
                column_gap: px(18),
                min_height: px(0),
                ..default()
            })
            .with_children(|body| {
                body.spawn(Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(14),
                    min_width: px(0),
                    min_height: px(0),
                    ..default()
                })
                .with_children(|left| {
                    left.spawn((
                        Node {
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            padding: px(14).all(),
                            row_gap: px(6),
                            border: px(1).all(),
                            border_radius: BorderRadius::all(px(theme::RADIUS)),
                            ..default()
                        },
                        panel(),
                    ))
                    .with_children(|list| {
                        list.spawn(Node {
                            padding: UiRect::new(px(17), px(17), px(2), px(6)),
                            ..default()
                        })
                        .with_children(|columns| {
                            columns.spawn((
                                caption("NAME"),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            columns.spawn((
                                caption("TRACK"),
                                Node {
                                    width: px(TRACK_COLUMN),
                                    ..default()
                                },
                            ));
                            columns.spawn((
                                caption("PLAYERS"),
                                Node {
                                    width: px(PLAYERS_COLUMN),
                                    ..default()
                                },
                            ));
                            columns.spawn((
                                caption("STATUS"),
                                Node {
                                    width: px(STATUS_COLUMN),
                                    ..default()
                                },
                            ));
                        });
                        list.spawn((
                            RowList,
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(6),
                                overflow: Overflow::scroll_y(),
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        list.spawn((
                            EmptyList,
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                left: px(0),
                                right: px(0),
                                top: px(0),
                                bottom: px(0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                row_gap: px(8),
                                ..default()
                            },
                            children![
                                (
                                    Text::new("NO LOBBY YET"),
                                    fonts.display(34.0),
                                    TextColor(theme::TEXT_DIM),
                                ),
                                (
                                    Text::new("Create one, and your friends will find it here."),
                                    fonts.text(16.0, 400),
                                    TextColor(theme::TEXT_FAINT),
                                ),
                            ],
                        ));
                    });

                    // Everyone on the server, in a lobby or looking at this same list.
                    left.spawn((
                        Node {
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            padding: px(14).all(),
                            row_gap: px(9),
                            border: px(1).all(),
                            border_radius: BorderRadius::all(px(theme::RADIUS)),
                            ..default()
                        },
                        panel(),
                    ))
                    .with_children(|online| {
                        online.spawn((OnlineCaption, caption("PLAYERS ONLINE")));
                        online.spawn((
                            OnlineList,
                            ShownPlayers::default(),
                            Node {
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(8),
                                row_gap: px(6),
                                max_height: px(ONLINE_HEIGHT),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                        ));
                    });
                });

                if !touchscreen.0 {
                    body.spawn((
                        Node {
                            width: px(DETAILS_WIDTH),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            padding: px(18).all(),
                            border: px(1).all(),
                            border_radius: BorderRadius::all(px(theme::RADIUS)),
                            ..default()
                        },
                        panel(),
                    ))
                    .with_children(|details| {
                        details.spawn((
                            Detail::Placeholder,
                            Text::new("Select a lobby to see who is in it."),
                            fonts.text(15.0, 400),
                            TextColor(theme::TEXT_FAINT),
                            Node {
                                margin: UiRect::top(px(8)),
                                ..default()
                            },
                        ));
                        details
                            .spawn((
                                Detail::Content,
                                Node {
                                    display: Display::None,
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(10),
                                    ..default()
                                },
                            ))
                            .with_children(|content| {
                                content.spawn((
                                    DetailThumbnail,
                                    ImageNode::default(),
                                    Node {
                                        width: percent(100),
                                        aspect_ratio: Some(400.0 / 250.0),
                                        border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                                ));
                                content.spawn((
                                    Detail::Name,
                                    Text::default(),
                                    fonts.display(30.0),
                                    TextColor(theme::TEXT),
                                ));
                                content.spawn((
                                    Detail::Track,
                                    Text::default(),
                                    fonts.text(15.0, 500),
                                    TextColor(theme::TEXT_DIM),
                                    Node {
                                        margin: UiRect::top(px(-8)),
                                        ..default()
                                    },
                                ));
                                content
                                    .spawn((DetailBadge, Node::default()))
                                    .with_child(badge(&fonts, "", theme::TEXT_DIM));
                                content.spawn((
                                    Detail::Description,
                                    Text::default(),
                                    fonts.text(14.5, 400),
                                    TextColor(theme::TEXT_DIM),
                                ));
                                content.spawn((
                                    Detail::PlayersCaption,
                                    Text::default(),
                                    fonts.caption(12.5),
                                    theme::CAPTION_SPACING,
                                    TextColor(theme::TEXT_FAINT),
                                    Node {
                                        margin: UiRect::top(px(6)),
                                        ..default()
                                    },
                                ));
                                content.spawn((
                                    Detail::Players,
                                    Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(3),
                                        flex_grow: 1.0,
                                        overflow: Overflow::clip_y(),
                                        ..default()
                                    },
                                ));
                                content
                                    .spawn((
                                        JoinButton,
                                        button(&fonts, ButtonKind::Primary, "JOIN"),
                                    ))
                                    .insert(Node {
                                        height: px(48),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        border: px(1.5).all(),
                                        border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                                        ..default()
                                    })
                                    .observe(join_selected);
                            });
                    });
                }
            });

            // Footer: what just went wrong, and the controls.
            root.spawn(Node {
                align_items: AlignItems::Center,
                min_height: px(24),
                ..default()
            })
            .with_children(|footer| {
                footer.spawn((
                    NoticeText,
                    Text::default(),
                    fonts.text(15.0, 600),
                    TextColor(theme::AMBER),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                footer.spawn((
                    hint_row(),
                    children![
                        hint(&fonts, "↑ ↓", "D-pad", "Select"),
                        hint(&fonts, "Enter", "A", "Join"),
                        hint(&fonts, "C", "Y", "Create"),
                        hint(&fonts, "Esc", "B", "Change nickname"),
                    ],
                ));
            });
        });
}

fn spawn_row(commands: &mut Commands, fonts: &Fonts, id: LobbyId) -> Entity {
    let name = commands
        .spawn((
            Text::default(),
            fonts.text(18.0, 600),
            TextColor(theme::TEXT),
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                overflow: Overflow::clip(),
                ..default()
            },
        ))
        .id();
    let track = commands
        .spawn((
            Text::default(),
            fonts.text(15.0, 500),
            TextColor(theme::TEXT_DIM),
            Node {
                width: px(TRACK_COLUMN),
                ..default()
            },
        ))
        .id();
    let players = commands
        .spawn((
            Text::default(),
            fonts.text(16.0, 600),
            TextColor(theme::TEXT),
            Node {
                width: px(PLAYERS_COLUMN),
                ..default()
            },
        ))
        .id();
    let badge = commands.spawn(badge(fonts, "", theme::TEXT_DIM)).id();
    let status = commands
        .spawn(Node {
            width: px(STATUS_COLUMN),
            ..default()
        })
        .add_child(badge)
        .id();

    commands
        .spawn((
            LobbyRow {
                id,
                name,
                track,
                players,
                badge,
            },
            Selectable,
            Node {
                padding: UiRect::axes(px(16), px(11)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                border: px(1.5).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                ..default()
            },
        ))
        .add_children(&[name, track, players, status])
        .observe(join_row)
        .id()
}

/// The badge of a lobby: its phase, and the detail that matters in it.
pub fn status_badge(summary: &LobbySummary, elapsed: f64) -> (String, Color) {
    let label = match summary.status {
        LobbyStatus::Waiting => format!(
            "WAITING  {}/{}",
            summary.players.len(),
            summary.settings.min_players
        ),
        LobbyStatus::Starting { remaining_ms } => {
            format!("ON THE GRID  {}", seconds_left(remaining_ms, elapsed))
        }
        // Lap 0: the leader has not reached the start line yet.
        LobbyStatus::Racing { lap } => {
            format!("RACING  LAP {}/{}", lap.max(1), summary.settings.laps)
        }
        LobbyStatus::Results { .. } => "RESULTS".into(),
    };
    (label, status_color(summary.status))
}

/// The color a phase is always shown in: on a lobby's badge, and on the dot beside each player
/// who is in it.
fn status_color(status: LobbyStatus) -> Color {
    match status {
        LobbyStatus::Waiting => theme::TEXT_DIM,
        LobbyStatus::Starting { .. } => theme::AMBER,
        LobbyStatus::Racing { .. } => theme::MAGENTA,
        LobbyStatus::Results { .. } => theme::GREEN,
    }
}

fn status_description(summary: &LobbySummary, elapsed: f64) -> String {
    match summary.status {
        LobbyStatus::Waiting => {
            let missing =
                usize::from(summary.settings.min_players).saturating_sub(summary.players.len());
            match missing {
                1 => "Waiting for one more player to start a race.".into(),
                missing => format!("Waiting for {missing} more players to start a race."),
            }
        }
        LobbyStatus::Starting { remaining_ms } => format!(
            "The cars are on the grid, the race starts in {} s. Join now to take part.",
            seconds_left(remaining_ms, elapsed)
        ),
        LobbyStatus::Racing { .. } => {
            "A race is on: you will watch it, then race in the next one.".into()
        }
        LobbyStatus::Results { .. } => "The race just ended. Join now for the next one.".into(),
    }
}

/// Whole seconds left of `remaining_ms`, counted from when the list arrived `elapsed` seconds ago.
fn seconds_left(remaining_ms: u32, elapsed: f64) -> u32 {
    (f64::from(remaining_ms) / 1000.0 - elapsed).ceil().max(0.0) as u32
}

fn stop_joining_on_failure(mut events: MessageReader<NetEvent>, mut joining: ResMut<Joining>) {
    if events
        .read()
        .any(|event| matches!(event, NetEvent::JoinFailed(_)))
    {
        joining.0 = false;
    }
}

fn sync_rows(
    mut commands: Commands,
    fonts: Res<Fonts>,
    list: Res<LobbyList>,
    tracks: Res<Tracks>,
    time: Res<Time<Real>>,
    containers: Query<Entity, With<RowList>>,
    rows: Query<(Entity, &LobbyRow)>,
    mut texts: Query<&mut Text, Without<BadgeText>>,
    mut badges: Query<(&mut BackgroundColor, &mut BorderColor, &Children), With<Badge>>,
    mut dots: Query<&mut BackgroundColor, (With<BadgeDot>, Without<Badge>)>,
    mut badge_texts: Query<(&mut Text, &mut TextColor), With<BadgeText>>,
    mut empty: Query<&mut Node, With<EmptyList>>,
) {
    let Ok(container) = containers.single() else {
        return;
    };
    let elapsed = time.elapsed_secs_f64() - list.received_at;
    let existing: HashMap<LobbyId, (Entity, &LobbyRow)> = rows
        .iter()
        .map(|(entity, row)| (row.id, (entity, row)))
        .collect();

    for (entity, row) in existing.values() {
        if !list.lobbies.iter().any(|summary| summary.id == row.id) {
            commands.entity(*entity).despawn();
        }
    }

    for summary in &list.lobbies {
        let Some((_, row)) = existing.get(&summary.id) else {
            // The server lists lobbies in creation order, so new ones go last.
            let row = spawn_row(&mut commands, &fonts, summary.id);
            commands.entity(container).add_child(row);
            continue;
        };
        let mut set_text = |entity: Entity, value: String| {
            if let Ok(mut text) = texts.get_mut(entity)
                && text.0 != value
            {
                text.0 = value;
            }
        };
        set_text(row.name, summary.settings.name.clone());
        set_text(row.track, tracks.name(&summary.settings.track).to_owned());
        set_text(
            row.players,
            format!("{}/{}", summary.players.len(), summary.settings.max_players),
        );
        let (label, color) = status_badge(summary, elapsed);
        set_badge(
            row.badge,
            &label,
            color,
            &mut badges,
            &mut dots,
            &mut badge_texts,
        );
    }

    let show_empty = list.loaded && list.lobbies.is_empty();
    for mut node in &mut empty {
        node.display = if show_empty {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Keeps the focus on something that exists: the selected row, the first row, or the create button.
fn keep_focus(
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<Selected>,
    dialog: Res<CreateDialog>,
    rows: Query<(Entity, &LobbyRow)>,
    focusable: Query<(), With<bevy::ui::auto_directional_navigation::AutoDirectionalNavigation>>,
    create: Query<Entity, With<CreateButton>>,
    list: Res<LobbyList>,
) {
    if dialog.open {
        return;
    }
    if let Some(entity) = focus.get()
        && let Ok((_, row)) = rows.get(entity)
    {
        selected.0 = Some(row.id);
    }
    if selected
        .0
        .is_some_and(|id| !list.lobbies.iter().any(|summary| summary.id == id))
    {
        selected.0 = None;
    }
    if focus.get().is_some_and(|entity| focusable.contains(entity)) {
        return;
    }

    // Rows spawned this frame are not queryable yet: the next frame will focus them.
    let first_row = rows
        .iter()
        .find(|(_, row)| list.lobbies.first().is_some_and(|first| first.id == row.id))
        .map(|(entity, _)| entity);
    if let Some(entity) = first_row.or_else(|| create.single().ok()) {
        focus.set(entity, FocusCause::Navigated);
    }
}

fn update_details(
    selected: Res<Selected>,
    list: Res<LobbyList>,
    tracks: Res<Tracks>,
    joining: Res<Joining>,
    time: Res<Time<Real>>,
    fonts: Res<Fonts>,
    mut commands: Commands,
    mut details: Query<(
        Entity,
        &Detail,
        &mut Node,
        Option<&mut Text>,
        Option<&Children>,
    )>,
    mut thumbnails: Query<&mut ImageNode, With<DetailThumbnail>>,
    badge_holders: Query<&Children, With<DetailBadge>>,
    mut badges: Query<(&mut BackgroundColor, &mut BorderColor, &Children), With<Badge>>,
    mut dots: Query<&mut BackgroundColor, (With<BadgeDot>, Without<Badge>)>,
    mut badge_texts: Query<(&mut Text, &mut TextColor), (With<BadgeText>, Without<Detail>)>,
    join_buttons: Query<(Entity, &Children, Has<InteractionDisabled>), With<JoinButton>>,
    mut labels: Query<&mut Text, (With<ButtonLabel>, Without<Detail>, Without<BadgeText>)>,
    mut shown_players: Local<Vec<String>>,
) {
    let summary = selected
        .0
        .and_then(|id| list.lobbies.iter().find(|summary| summary.id == id));
    let elapsed = time.elapsed_secs_f64() - list.received_at;

    for (entity, detail, mut node, text, children) in &mut details {
        let display = match detail {
            Detail::Placeholder if summary.is_some() => Display::None,
            Detail::Content if summary.is_none() => Display::None,
            _ => Display::Flex,
        };
        if node.display != display {
            node.display = display;
        }
        let Some(summary) = summary else {
            continue;
        };
        let value = match detail {
            Detail::Name => summary.settings.name.clone(),
            Detail::Track => format!(
                "{}  ·  {} {}",
                tracks.name(&summary.settings.track),
                summary.settings.laps,
                if summary.settings.laps == 1 {
                    "lap"
                } else {
                    "laps"
                }
            ),
            Detail::Description => status_description(summary, elapsed),
            Detail::PlayersCaption => format!(
                "PLAYERS  {}/{}  ·  RACES FROM {}",
                summary.players.len(),
                summary.settings.max_players,
                summary.settings.min_players
            ),
            Detail::Players => {
                if *shown_players != summary.players {
                    for child in children.into_iter().flatten() {
                        commands.entity(*child).despawn();
                    }
                    for nickname in &summary.players {
                        let row = commands
                            .spawn((
                                Text::new(nickname.clone()),
                                fonts.text(15.5, 500),
                                TextColor(theme::TEXT),
                            ))
                            .id();
                        commands.entity(entity).add_child(row);
                    }
                    *shown_players = summary.players.clone();
                }
                continue;
            }
            Detail::Placeholder | Detail::Content => continue,
        };
        if let Some(mut text) = text
            && text.0 != value
        {
            text.0 = value;
        }
    }

    let Some(summary) = summary else {
        shown_players.clear();
        return;
    };
    if let Some(entry) = tracks.0.get(&summary.settings.track) {
        for mut image in &mut thumbnails {
            if image.image != entry.thumbnail {
                image.image = entry.thumbnail.clone();
            }
        }
    }
    let (label, color) = status_badge(summary, elapsed);
    for holder in &badge_holders {
        for badge in holder {
            set_badge(
                *badge,
                &label,
                color,
                &mut badges,
                &mut dots,
                &mut badge_texts,
            );
        }
    }

    let full = summary.players.len() >= usize::from(summary.settings.max_players);
    let racing = matches!(summary.status, LobbyStatus::Racing { .. });
    let label = if joining.0 {
        "JOINING…"
    } else if full {
        "LOBBY FULL"
    } else if racing {
        "JOIN AND WATCH"
    } else {
        "JOIN"
    };
    for (button, children, disabled) in &join_buttons {
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child)
                && text.0 != label
            {
                text.0 = label.into();
            }
        }
        let enabled = !full && !joining.0;
        if enabled && disabled {
            commands.entity(button).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(button).insert(InteractionDisabled);
        }
    }
}

/// Everyone connected to the server, named and placed. The whole panel is rebuilt when what it
/// would say changes, which is rare: a player connecting, leaving, or moving between the list and
/// a lobby.
fn update_online_players(
    mut commands: Commands,
    fonts: Res<Fonts>,
    online: Res<OnlinePlayers>,
    list: Res<LobbyList>,
    connection: Res<Connection>,
    mut captions: Query<&mut Text, With<OnlineCaption>>,
    mut panels: Query<(Entity, Option<&Children>, &mut ShownPlayers), With<OnlineList>>,
) {
    let own = match &*connection {
        Connection::Online { nickname } | Connection::Connecting { nickname } => nickname.as_str(),
        _ => "",
    };
    let chips: Vec<Chip> = online
        .0
        .iter()
        .map(|player| {
            let (place, color) = presence_place(player.presence, &list);
            Chip {
                nickname: player.nickname.clone(),
                place,
                color,
                own: player.nickname == own,
            }
        })
        .collect();

    let caption = format!("PLAYERS ONLINE  {}", chips.len());
    for mut text in &mut captions {
        if text.0 != caption {
            text.0 = caption.clone();
        }
    }

    for (container, children, mut shown) in &mut panels {
        if shown.0 == chips {
            continue;
        }
        for child in children.into_iter().flatten() {
            commands.entity(*child).despawn();
        }
        for chip in &chips {
            let entity = spawn_chip(&mut commands, &fonts, chip);
            commands.entity(container).add_child(entity);
        }
        shown.0 = chips.clone();
    }
}

/// Where a connected player is, as the panel says it, and the color of that place: the lobby's
/// own status color, so a chip and a badge always agree.
fn presence_place(presence: Presence, list: &LobbyList) -> (String, Color) {
    match presence {
        Presence::Browsing => ("on the list".into(), theme::TEXT_FAINT),
        Presence::InLobby(id) => match list.lobbies.iter().find(|summary| summary.id == id) {
            Some(summary) => (summary.settings.name.clone(), status_color(summary.status)),
            // The two lists travel apart: the lobby may not be listed yet, or no longer be.
            None => ("in a lobby".into(), theme::TEXT_FAINT),
        },
    }
}

fn spawn_chip(commands: &mut Commands, fonts: &Fonts, chip: &Chip) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(7),
                padding: UiRect::axes(px(10), px(3)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(theme::SURFACE),
            children![
                (
                    Node {
                        width: px(7),
                        height: px(7),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(chip.color),
                ),
                (
                    Text::new(chip.nickname.clone()),
                    fonts.text(15.0, 600),
                    TextColor(if chip.own { theme::OWN } else { theme::TEXT }),
                ),
                (
                    Text::new(chip.place.clone()),
                    fonts.text(14.0, 400),
                    TextColor(theme::TEXT_FAINT),
                ),
            ],
        ))
        .id()
}

fn update_frame(
    list: Res<LobbyList>,
    notice: Res<Notice>,
    time: Res<Time<Real>>,
    dialog: Res<CreateDialog>,
    mut roots: Query<&mut Visibility, With<BrowserRoot>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<LobbyCount>>,
        Query<&mut Text, With<NoticeText>>,
    )>,
) {
    let count = if list.loaded {
        list.lobbies.len().to_string()
    } else {
        "…".into()
    };
    for mut text in &mut texts.p0() {
        if text.0 != count {
            text.0 = count.clone();
        }
    }
    let notice = notice
        .current(time.elapsed_secs_f64())
        .unwrap_or_default()
        .to_owned();
    for mut text in &mut texts.p1() {
        if text.0 != notice {
            text.0 = notice.clone();
        }
    }
    // The creation dialog replaces the browser, so the focus cannot wander behind it.
    let visibility = if dialog.open {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut root in &mut roots {
        root.set_if_neq(visibility);
    }
}

fn shortcuts(
    input: MenuInput,
    dialog: Res<CreateDialog>,
    create: Query<Entity, With<CreateButton>>,
    mut commands: Commands,
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if dialog.open {
        return;
    }
    if input.key(KeyCode::KeyC) || input.gamepad(GamepadButton::North) {
        for entity in &create {
            commands.trigger(Activate { entity });
        }
    } else if input.back() {
        network.disconnect(&mut connection);
        next_screen.set(Screen::Login);
    }
}

fn join_row(
    activate: On<Activate>,
    rows: Query<&LobbyRow>,
    list: Res<LobbyList>,
    network: Res<Network>,
    mut joining: ResMut<Joining>,
    mut notice: ResMut<Notice>,
    time: Res<Time<Real>>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        request_join(row.id, &list, &network, &mut joining, &mut notice, &time);
    }
}

fn join_selected(
    _: On<Activate>,
    selected: Res<Selected>,
    list: Res<LobbyList>,
    network: Res<Network>,
    mut joining: ResMut<Joining>,
    mut notice: ResMut<Notice>,
    time: Res<Time<Real>>,
) {
    if let Some(id) = selected.0 {
        request_join(id, &list, &network, &mut joining, &mut notice, &time);
    }
}

fn request_join(
    id: LobbyId,
    list: &LobbyList,
    network: &Network,
    joining: &mut Joining,
    notice: &mut Notice,
    time: &Time<Real>,
) {
    if joining.0 {
        return;
    }
    let Some(summary) = list.lobbies.iter().find(|summary| summary.id == id) else {
        return;
    };
    if summary.players.len() >= usize::from(summary.settings.max_players) {
        notice.show("This lobby is full.", time.elapsed_secs_f64());
        return;
    }
    network.send(ClientMessage::JoinLobby(id));
    joining.0 = true;
}

fn change_nickname(
    _: On<Activate>,
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    network.disconnect(&mut connection);
    next_screen.set(Screen::Login);
}

fn open_create_dialog(_: On<Activate>, mut dialog: ResMut<CreateDialog>) {
    dialog.open = true;
}
