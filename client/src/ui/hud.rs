//! Everything drawn over the race: the lobby's phase, the start lights and how well the player
//! started, the lap and time, the standings, the speed, the drift and boost gauge, the results, the other players' names above their cars, and the dialog to
//! leave the lobby.
//!
//! Each frame builds a [`HudModel`], what the HUD should say, from the lobby state and the race as
//! displayed; a single pass then writes it into the text entities. The start lights and race times
//! follow the displayed server tick, so the lights turn green when the cars on screen start moving.

use std::collections::{HashMap, HashSet};

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use space_race_protocol::{CarId, LobbyPhase, PlayerStatus};
use space_race_sim::TICK_RATE;
use space_race_sim::race::{self, RaceStanding, StartGrade};

use super::navigation::MenuInput;
use super::theme::{self, Fonts};
use super::touch::Touchscreen;
use super::widgets::{ButtonKind, button, hint, hint_row, panel};
use crate::camera::MainCamera;
use crate::latency::{Latency, LatencyOverlay};
use crate::lobby::{CurrentLobby, Membership, Tracks, leave_lobby};
use crate::net::Network;
use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;
use crate::track::CurrentTrack;
use crate::world;

/// Rows of the standings and of the results: the most players a lobby holds.
const ROWS: usize = space_race_protocol::MAX_LOBBY_PLAYERS as usize;
/// How long the start lights stay after the start, in seconds.
const GO_SECONDS: f64 = 1.0;
/// How long a start light pops when it lights up, in seconds.
const LIGHT_POP_SECONDS: f32 = 0.25;
/// How long the last start light blazes when it lights up at the start, in seconds.
const START_FLASH_SECONDS: f32 = 0.4;
const LIGHT_SIZE: f32 = 54.0;
/// How long the start grade stays after the start, in seconds.
const START_GRADE_SECONDS: f64 = 3.0;
/// Names are shown above cars closer than this, in meters.
const NAMEPLATE_DISTANCE: f32 = 120.0;
const NAMEPLATE_WIDTH: f32 = 220.0;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<LeaveDialog>()
            .add_systems(OnEnter(Screen::Lobby), spawn_hud)
            .add_systems(OnExit(Screen::Lobby), close_leave_dialog)
            .add_systems(
                Update,
                (
                    update_hud,
                    update_nameplates,
                    leave_shortcuts,
                    show_leave_dialog,
                )
                    .chain()
                    .after(RaceUpdate)
                    .run_if(in_state(Screen::Lobby)),
            );
    }
}

/// Whether the player is being asked to confirm leaving the lobby. Their car lets go meanwhile.
#[derive(Resource, Default)]
pub struct LeaveDialog {
    pub open: bool,
}

/// A text of the HUD.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    LobbyName,
    LobbyTrack,
    Title,
    Subtitle,
    StartGrade,
    StartLabel,
    Speed,
    GaugeLabel,
    StandingsTitle,
    StandingPosition(usize),
    StandingName(usize),
    StandingInfo(usize),
    ResultPosition(usize),
    ResultName(usize),
    ResultTime(usize),
    ResultsFooter,
    FrameRate,
    /// The latency figures, one per line, beside their names.
    Latency,
}

/// One of the start lights, counted from the left.
#[derive(Component)]
struct StartLight(u32);

/// The ring that bursts out of the last start light, at the start.
#[derive(Component)]
struct StartRing;

/// The filled part of the drift and boost gauge.
#[derive(Component)]
struct GaugeFill;

/// Width of the drift and boost gauge, in pixels.
const GAUGE_WIDTH: f32 = 220.0;

/// A part of the HUD shown only at times.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash)]
enum Block {
    StartLights,
    StartGrade,
    Speed,
    Gauge,
    Standings,
    StandingRow(usize),
    Results,
    ResultRow(usize),
    DrivingHints,
    /// Between the finish line and the next grid, when the car drives itself.
    WatchingHints,
    SpectatingHints,
    Latency,
}

/// What the HUD says this frame.
#[derive(Default)]
struct HudModel {
    texts: HashMap<Slot, (String, Color)>,
    shown: HashSet<Block>,
    /// How many start lights are lit, and seconds since the last of them lit up.
    lights: (u32, f32),
    /// How full the drift or boost gauge is, from 0 to 1, and its color.
    gauge: (f32, Color),
}

impl HudModel {
    fn text(&mut self, slot: Slot, text: impl Into<String>, color: Color) {
        self.texts.insert(slot, (text.into(), color));
    }
}

fn spawn_hud(mut commands: Commands, fonts: Res<Fonts>, touchscreen: Res<Touchscreen>) {
    // A phone is a short screen held close, with a thumb over each bottom corner: everything is
    // drawn smaller, and what lives in those corners on a desk moves up out of the way.
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    let text = |slot: Slot, font: TextFont, color: Color| {
        (
            slot,
            Text::default(),
            font,
            TextColor(color),
            theme::TEXT_SHADOW,
        )
    };

    commands
        .spawn((
            DespawnOnExit(Screen::Lobby),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|root| {
            // Top left: where the player is.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(26),
                top: px(18),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|corner| {
                corner.spawn(text(
                    Slot::LobbyName,
                    fonts.display(pick(26.0, 21.0)),
                    theme::TEXT,
                ));
                corner.spawn(text(
                    Slot::LobbyTrack,
                    fonts.text(pick(14.0, 12.0), 500),
                    theme::TEXT_DIM,
                ));
            });

            // Top center: the phase, the lap or who is watched.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(pick(16.0, 10.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|banner| {
                banner.spawn(text(
                    Slot::Title,
                    fonts.display(pick(38.0, 28.0)),
                    theme::TEXT,
                ));
                banner.spawn(text(
                    Slot::Subtitle,
                    fonts.text(pick(17.0, 14.0), 500),
                    theme::TEXT_DIM,
                ));
            });

            // Center: the start lights, in a row on a dark pill.
            root.spawn((
                Block::StartLights,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    // Below the start grade, which shows while they are still green.
                    top: px(pick(210.0, 128.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_children(|center| {
                center
                    .spawn((
                        Node {
                            padding: UiRect::axes(px(pick(26.0, 16.0)), px(pick(18.0, 12.0))),
                            column_gap: px(pick(24.0, 14.0)),
                            border: px(1).all(),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        panel(),
                    ))
                    .with_children(|row| {
                        let light_size = pick(LIGHT_SIZE, 36.0);
                        for light in 0..race::START_LIGHTS {
                            let mut spawned = row.spawn((
                                StartLight(light),
                                Node {
                                    width: px(light_size),
                                    height: px(light_size),
                                    border: px(2).all(),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(theme::SURFACE),
                                BorderColor::all(theme::PANEL_BORDER),
                                BoxShadow(Vec::new()),
                            ));
                            if light + 1 == race::START_LIGHTS {
                                spawned.with_child((
                                    StartRing,
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(-2),
                                        top: px(-2),
                                        width: px(light_size),
                                        height: px(light_size),
                                        border: px(3).all(),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BorderColor::all(Color::NONE),
                                ));
                            }
                        }
                    });
            });

            // Under the lap and time, over the start lights: how well the player started.
            root.spawn((
                Block::StartGrade,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(pick(92.0, 54.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|start| {
                start.spawn(text(
                    Slot::StartGrade,
                    fonts.display(pick(60.0, 42.0)),
                    theme::AMBER,
                ));
                start.spawn((
                    text(
                        Slot::StartLabel,
                        fonts.caption(pick(17.0, 13.0)),
                        theme::AMBER,
                    ),
                    theme::CAPTION_SPACING,
                ));
            });

            // Top right: standings, or who is in the lobby.
            root.spawn((
                Block::Standings,
                Node {
                    position_type: PositionType::Absolute,
                    right: px(22),
                    // Under the button that leaves, which only a touchscreen has.
                    top: px(pick(18.0, 54.0)),
                    width: px(pick(270.0, 220.0)),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(14), px(10)),
                    row_gap: px(2),
                    border: px(1).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS)),
                    ..default()
                },
                panel(),
            ))
            .with_children(|standings| {
                standings.spawn((
                    Slot::StandingsTitle,
                    Text::default(),
                    fonts.caption(12.5),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        margin: UiRect::bottom(px(4)),
                        ..default()
                    },
                ));
                for row in 0..ROWS {
                    standings
                        .spawn((
                            Block::StandingRow(row),
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                ..default()
                            },
                        ))
                        .with_children(|line| {
                            line.spawn((
                                Slot::StandingPosition(row),
                                Text::default(),
                                fonts.display(19.0),
                                TextColor(theme::TEXT_DIM),
                                Node {
                                    width: px(26),
                                    ..default()
                                },
                            ));
                            line.spawn((
                                Slot::StandingName(row),
                                Text::default(),
                                fonts.text(15.5, 600),
                                TextColor(theme::TEXT),
                                Node {
                                    flex_grow: 1.0,
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                            ));
                            line.spawn((
                                Slot::StandingInfo(row),
                                Text::default(),
                                fonts.text(13.5, 600),
                                TextColor(theme::TEXT_DIM),
                            ));
                        });
                }
            });

            // Beside the speed: the drift charge, then the boost it gave.
            let mut gauge = Node {
                position_type: PositionType::Absolute,
                left: px(pick(30.0, 27.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            };
            // On a desk the speed sits in the bottom corner with the gauge over it; under a thumb
            // that corner is taken, so both move up under the lobby's name.
            if touchscreen.0 {
                gauge.top = px(118);
            } else {
                gauge.bottom = px(104);
            }
            root.spawn((Block::Gauge, gauge)).with_children(|gauge| {
                gauge.spawn((
                    text(Slot::GaugeLabel, fonts.caption(15.0), theme::NEON),
                    theme::CAPTION_SPACING,
                ));
                gauge
                    .spawn((
                        Node {
                            width: px(GAUGE_WIDTH),
                            height: px(8),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme::NEON_FAINT),
                    ))
                    .with_child((
                        GaugeFill,
                        Node {
                            width: percent(0),
                            height: percent(100),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme::NEON),
                    ));
            });

            // Bottom left on a desk, top left under a thumb: the speed.
            let mut speed = Node {
                position_type: PositionType::Absolute,
                left: px(28),
                align_items: AlignItems::Baseline,
                column_gap: px(8),
                ..default()
            };
            if touchscreen.0 {
                speed.top = px(62);
            } else {
                speed.bottom = px(10);
            }
            root.spawn((Block::Speed, speed)).with_children(|speed| {
                speed.spawn(text(
                    Slot::Speed,
                    fonts.display(pick(76.0, 46.0)),
                    theme::TEXT,
                ));
                speed.spawn((
                    Text::new("KM/H"),
                    fonts.caption(pick(15.0, 12.0)),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_DIM),
                    theme::TEXT_SHADOW,
                ));
            });

            // Bottom right: the keys to press, and the frame rate. On a touchscreen there are no
            // keys to press and that corner is under the cross, so what is left of it -- the
            // frame rate -- goes to the bottom edge, between the two thumbs.
            let mut hints = Node {
                position_type: PositionType::Absolute,
                bottom: px(pick(18.0, 4.0)),
                flex_direction: FlexDirection::Column,
                align_items: touchscreen.pick(AlignItems::End, AlignItems::Center),
                row_gap: px(8),
                ..default()
            };
            if touchscreen.0 {
                (hints.left, hints.right) = (px(0), px(0));
            } else {
                hints.right = px(24);
            }
            root.spawn(hints).with_children(|corner| {
                corner.spawn((
                    Block::DrivingHints,
                    hint_row(),
                    children![
                        hint(&fonts, "W", "R", "Accelerate"),
                        hint(&fonts, "A D", "L-stick", "Steer"),
                        hint(&fonts, "Shift", "X", "Drift"),
                        hint(&fonts, "Esc", "Start", "Leave"),
                    ],
                ));
                corner.spawn((
                    Block::WatchingHints,
                    hint_row(),
                    children![hint(&fonts, "Esc", "Start", "Leave")],
                ));
                corner.spawn((
                    Block::SpectatingHints,
                    hint_row(),
                    children![
                        hint(&fonts, "← →", "D-pad", "Other car"),
                        hint(&fonts, "Esc", "Start", "Leave"),
                    ],
                ));
                corner.spawn(text(
                    Slot::FrameRate,
                    fonts.text(12.5, 500),
                    theme::TEXT_FAINT,
                ));
            });

            // Top right, out of the standings' way: the one way off a touchscreen, which has no
            // Escape key and no Start button.
            if touchscreen.0 {
                root.spawn((LeaveButton, button(&fonts, ButtonKind::Secondary, "LEAVE")))
                    .insert(Node {
                        position_type: PositionType::Absolute,
                        right: px(22),
                        top: px(12),
                        padding: UiRect::axes(px(16), px(6)),
                        border: px(1.5).all(),
                        border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                        ..default()
                    })
                    .observe(ask_to_leave);
            }

            // Left, under whatever the top left corner holds: the latency figures, when asked
            // for (F3). Two columns of the same number of lines, so names and values line up
            // without a monospaced font.
            root.spawn((
                Block::Latency,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(24),
                    top: px(pick(78.0, 150.0)),
                    column_gap: px(14),
                    padding: UiRect::axes(px(12), px(8)),
                    border: px(1).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                    ..default()
                },
                panel(),
            ))
            .with_children(|figures| {
                let names: Vec<&str> = Latency::default()
                    .lines()
                    .iter()
                    .map(|(name, _)| *name)
                    .collect();
                figures.spawn((
                    Text::new(names.join("\n")),
                    fonts.caption(12.0),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_FAINT),
                ));
                figures.spawn((
                    Slot::Latency,
                    Text::default(),
                    fonts.caption(12.0),
                    TextColor(theme::TEXT),
                ));
            });

            // Top center, above the car that keeps driving: the results between races.
            root.spawn((
                Block::Results,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(pick(84.0, 44.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_children(|center| {
                center
                    .spawn((
                        Node {
                            width: px(pick(470.0, 400.0)),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::axes(px(pick(26.0, 20.0)), px(pick(20.0, 14.0))),
                            row_gap: px(4),
                            border: px(1).all(),
                            border_radius: BorderRadius::all(px(theme::RADIUS)),
                            ..default()
                        },
                        panel(),
                    ))
                    .with_children(|card| {
                        card.spawn((
                            Text::new("RESULTS"),
                            fonts.display(pick(44.0, 32.0)),
                            TextColor(theme::TEXT),
                            TextShadow {
                                offset: Vec2::new(3.0, 3.0),
                                color: theme::GREEN.with_alpha(0.6),
                            },
                            Node {
                                margin: UiRect::bottom(px(8)),
                                ..default()
                            },
                        ));
                        for row in 0..ROWS {
                            card.spawn((
                                Block::ResultRow(row),
                                Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(12),
                                    padding: UiRect::axes(px(6), px(3)),
                                    ..default()
                                },
                            ))
                            .with_children(|line| {
                                line.spawn((
                                    Slot::ResultPosition(row),
                                    Text::default(),
                                    fonts.display(24.0),
                                    TextColor(theme::TEXT),
                                    Node {
                                        width: px(52),
                                        ..default()
                                    },
                                ));
                                line.spawn((
                                    Slot::ResultName(row),
                                    Text::default(),
                                    fonts.text(18.0, 600),
                                    TextColor(theme::TEXT),
                                    Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    },
                                ));
                                line.spawn((
                                    Slot::ResultTime(row),
                                    Text::default(),
                                    fonts.text(17.0, 600),
                                    TextColor(theme::TEXT_DIM),
                                ));
                            });
                        }
                        card.spawn((
                            Slot::ResultsFooter,
                            Text::default(),
                            fonts.caption(13.0),
                            theme::CAPTION_SPACING,
                            TextColor(theme::TEXT_DIM),
                            Node {
                                margin: UiRect::top(px(10)),
                                ..default()
                            },
                        ));
                    });
            });
        });
}

fn update_hud(
    current: Res<CurrentLobby>,
    view: Res<RaceView>,
    tracks: Res<Tracks>,
    track: Option<Res<CurrentTrack>>,
    touchscreen: Res<Touchscreen>,
    diagnostics: Res<DiagnosticsStore>,
    latency: Res<Latency>,
    overlay: Res<LatencyOverlay>,
    mut texts: Query<(&Slot, &mut Text, &mut TextColor)>,
    mut lights: Query<
        (
            &StartLight,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
            &mut UiTransform,
        ),
        Without<GaugeFill>,
    >,
    mut rings: Query<(&mut UiTransform, &mut BorderColor), (With<StartRing>, Without<StartLight>)>,
    mut blocks: Query<(&Block, &mut Node)>,
    mut gauge_fills: Query<(&mut Node, &mut BackgroundColor), (With<GaugeFill>, Without<Block>)>,
) {
    let Some(membership) = &current.0 else {
        return;
    };
    let track_length = track.as_ref().map_or(1.0, |track| track.0.length());
    let mut model = build_model(membership, &view, &tracks, track_length, *touchscreen);
    if let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
    {
        model.text(Slot::FrameRate, format!("{fps:.0} fps"), theme::TEXT_FAINT);
    }
    if overlay.0 {
        model.shown.insert(Block::Latency);
        let values: Vec<String> = latency
            .lines()
            .into_iter()
            .map(|(_, value)| value)
            .collect();
        model.text(Slot::Latency, values.join("\n"), theme::TEXT);
    }

    for (slot, mut text, mut color) in &mut texts {
        let (value, value_color) = model
            .texts
            .get(slot)
            .map_or(("", color.0), |(value, color)| (value.as_str(), *color));
        if text.0 != value {
            text.0 = value.to_owned();
        }
        if color.0 != value_color {
            color.0 = value_color;
        }
    }
    let (lit, since) = model.lights;
    // From 1 as a light comes on down to 0 once its effect is over.
    let fade = |seconds: f32| (1.0 - since / seconds).clamp(0.0, 1.0);
    for (light, mut background, mut border, mut shadow, mut transform) in &mut lights {
        let on = light.0 < lit;
        let start = light.0 + 1 == race::START_LIGHTS;
        let newest = on && light.0 + 1 == lit;
        let color = if start { theme::GREEN } else { theme::AMBER };
        let (fill, edge) = if on {
            (color, color)
        } else {
            (theme::SURFACE, theme::PANEL_BORDER)
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
        let glow = match (on, start) {
            (false, _) => BoxShadow(Vec::new()),
            (true, false) => BoxShadow::new(color.with_alpha(0.75), px(0), px(0), px(4), px(22)),
            // The start blazes, then settles to a strong glow.
            (true, true) => {
                let blaze = fade(START_FLASH_SECONDS);
                BoxShadow::new(
                    color.with_alpha(0.9),
                    px(0),
                    px(0),
                    px(8.0 + 14.0 * blaze),
                    px(34.0 + 40.0 * blaze),
                )
            }
        };
        shadow.set_if_neq(glow);
        let pop = match (newest, start) {
            (false, _) => 0.0,
            (true, false) => 0.3 * fade(LIGHT_POP_SECONDS),
            (true, true) => 0.6 * fade(START_FLASH_SECONDS),
        };
        let scale = Vec2::splat(1.0 + pop);
        if transform.scale != scale {
            transform.scale = scale;
        }
    }
    // A ring bursting out of the start light.
    let burst = if lit == race::START_LIGHTS {
        (since / GO_SECONDS as f32).min(1.0)
    } else {
        1.0
    };
    for (mut transform, mut border) in &mut rings {
        let scale = Vec2::splat(1.0 + 2.5 * (1.0 - (1.0 - burst).powi(3)));
        if transform.scale != scale {
            transform.scale = scale;
        }
        border.set_if_neq(BorderColor::all(theme::GREEN.with_alpha(1.0 - burst)));
    }
    let (fill, fill_color) = model.gauge;
    for (mut node, mut color) in &mut gauge_fills {
        let width = percent(fill.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
        if color.0 != fill_color {
            color.0 = fill_color;
        }
    }
    for (block, mut node) in &mut blocks {
        let display = if model.shown.contains(block) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

fn build_model(
    membership: &Membership,
    view: &RaceView,
    tracks: &Tracks,
    track_length: f32,
    touchscreen: Touchscreen,
) -> HudModel {
    let mut model = HudModel::default();
    let settings = &membership.settings;
    model.text(Slot::LobbyName, settings.name.clone(), theme::TEXT);
    model.text(
        Slot::LobbyTrack,
        format!(
            "{}  ·  {} {}",
            tracks.name(&settings.track),
            settings.laps,
            if settings.laps == 1 { "lap" } else { "laps" }
        ),
        theme::TEXT_DIM,
    );

    if let Some(car) = view.own_car {
        model.shown.insert(Block::Speed);
        model.text(
            Slot::Speed,
            format!("{:.0}", car.velocity.length() * 3.6),
            theme::TEXT,
        );
        // Nothing answers the wheel between the finish line and the next grid: the only key left
        // is the one that leaves. A touchscreen has no keys at all, and says so by showing none.
        if !touchscreen.0 {
            let watching = matches!(membership.own_status(), Some(PlayerStatus::Finished { .. }));
            model.shown.insert(if watching {
                Block::WatchingHints
            } else {
                Block::DrivingHints
            });
        }
        if let Some(snapshot) = view.own_snapshot {
            if snapshot.car.is_drifting() {
                model.shown.insert(Block::Gauge);
                model.text(Slot::GaugeLabel, "DRIFT", theme::NEON);
                model.gauge = (snapshot.drift_gauge, theme::NEON);
            } else if snapshot.car.boost > 0.0 {
                model.shown.insert(Block::Gauge);
                model.text(Slot::GaugeLabel, "BOOST", theme::AMBER);
                model.gauge = (snapshot.boost_gauge, theme::AMBER);
            }
        }
    } else if !touchscreen.0 {
        model.shown.insert(Block::SpectatingHints);
    }

    let Some(state) = &membership.state else {
        model.text(Slot::Title, "JOINING…", theme::TEXT);
        return model;
    };
    // The player's own timeline while their car is predicted, so the lights turn green when their
    // car can go and the race time runs with it; the displayed race otherwise.
    let tick = view.own_tick.or(view.tick).unwrap_or(0.0);
    let seconds_until = |target: u32| (f64::from(target) - tick) / f64::from(TICK_RATE);
    let own_status = membership.own_status();

    match &state.phase {
        LobbyPhase::Waiting => {
            model.text(Slot::Title, "WAITING FOR PLAYERS", theme::TEXT);
            model.text(
                Slot::Subtitle,
                format!(
                    "{} of {} players  ·  drive around while you wait",
                    state.players.len(),
                    settings.min_players
                ),
                theme::TEXT_DIM,
            );
            player_list(&mut model, membership);
        }
        LobbyPhase::Racing { start_tick } => {
            let until_start = seconds_until(*start_tick);
            // Nothing for the first seconds on the grid, then one more light every second, the
            // last one at the start, and all of them for a moment after it.
            let (lit, since) = race::start_lights(until_start as f32);
            if lit > 0 && (until_start > 0.0 || -until_start < GO_SECONDS) {
                model.shown.insert(Block::StartLights);
                model.lights = (lit, since);
            }

            let race_seconds = (-until_start).max(0.0);
            let start = membership
                .player(membership.car)
                .and_then(|player| player.start);
            if let Some(grade) = start
                && until_start <= 0.0
                && race_seconds < START_GRADE_SECONDS
            {
                let (label, color) = start_grade_label(grade);
                model.shown.insert(Block::StartGrade);
                model.text(Slot::StartGrade, format!("{grade:?}"), color);
                model.text(Slot::StartLabel, label, color);
            }
            match own_status {
                Some(PlayerStatus::Spectating) => {
                    let watched = view
                        .followed
                        .and_then(|(car, _)| membership.player(car))
                        .map_or(String::new(), |player| player.nickname.to_uppercase());
                    model.text(Slot::Title, format!("WATCHING {watched}"), theme::MAGENTA);
                    model.text(Slot::Subtitle, "You race in the next one", theme::TEXT_DIM);
                }
                Some(PlayerStatus::Finished { time_ms }) => {
                    let position = standings(membership, view)
                        .iter()
                        .position(|(car, _)| *car == membership.car)
                        .map_or(String::new(), |index| ordinal(index + 1).to_uppercase());
                    model.text(Slot::Title, format!("FINISHED {position}"), theme::GREEN);
                    model.text(Slot::Subtitle, race_time(time_ms), theme::TEXT);
                }
                _ => {
                    let lap = view
                        .cars
                        .iter()
                        .find(|car| car.id == membership.car)
                        .map_or(0, |car| race::lap(car.progress, track_length))
                        .clamp(1, u32::from(settings.laps));
                    model.text(
                        Slot::Title,
                        format!("LAP {lap}/{}", settings.laps),
                        theme::TEXT,
                    );
                    model.text(
                        Slot::Subtitle,
                        race_time((race_seconds * 1000.0) as u32),
                        theme::TEXT_DIM,
                    );
                }
            }
            race_standings(&mut model, membership, view, track_length);
        }
        LobbyPhase::Results { results, end_tick } => {
            model.shown.insert(Block::Results);
            for (row, result) in results.iter().take(ROWS).enumerate() {
                model.shown.insert(Block::ResultRow(row));
                let own = result.car == membership.car;
                let position_color = if row == 0 { theme::AMBER } else { theme::TEXT };
                let name_color = if own { theme::OWN } else { theme::TEXT };
                model.text(
                    Slot::ResultPosition(row),
                    ordinal(row + 1).to_uppercase(),
                    position_color,
                );
                model.text(Slot::ResultName(row), result.nickname.clone(), name_color);
                match result.time_ms {
                    Some(time_ms) => {
                        model.text(Slot::ResultTime(row), race_time(time_ms), theme::TEXT)
                    }
                    None => model.text(Slot::ResultTime(row), "DID NOT FINISH", theme::TEXT_FAINT),
                }
            }
            let seconds = seconds_until(*end_tick).ceil().max(0.0);
            model.text(
                Slot::ResultsFooter,
                format!("NEXT RACE IN {seconds}"),
                theme::TEXT_DIM,
            );
        }
    }
    model
}

/// The players of the lobby, outside races.
fn player_list(model: &mut HudModel, membership: &Membership) {
    let Some(state) = &membership.state else {
        return;
    };
    model.shown.insert(Block::Standings);
    model.text(
        Slot::StandingsTitle,
        format!(
            "PLAYERS  {}/{}",
            state.players.len(),
            membership.settings.max_players
        ),
        theme::TEXT_FAINT,
    );
    for (row, player) in state.players.iter().take(ROWS).enumerate() {
        model.shown.insert(Block::StandingRow(row));
        let own = player.car == membership.car;
        model.text(Slot::StandingPosition(row), "·", theme::TEXT_FAINT);
        model.text(
            Slot::StandingName(row),
            player.nickname.clone(),
            if own { theme::OWN } else { theme::TEXT },
        );
        if own {
            model.text(Slot::StandingInfo(row), "YOU", theme::TEXT_FAINT);
        }
    }
}

/// Every racer, best first.
fn standings(membership: &Membership, view: &RaceView) -> Vec<(CarId, RaceStanding)> {
    let Some(state) = &membership.state else {
        return Vec::new();
    };
    let mut standings: Vec<_> = state
        .players
        .iter()
        .filter_map(|player| {
            let standing = match player.status {
                PlayerStatus::Finished { time_ms } => RaceStanding::Finished { time_ms },
                PlayerStatus::Racing => RaceStanding::Racing {
                    progress: view
                        .cars
                        .iter()
                        .find(|car| car.id == player.car)
                        .map_or(f32::MIN, |car| car.progress),
                },
                PlayerStatus::Driving | PlayerStatus::Spectating => return None,
            };
            Some((player.car, standing))
        })
        .collect();
    standings.sort_by(|(_, a), (_, b)| a.rank(b));
    standings
}

fn race_standings(
    model: &mut HudModel,
    membership: &Membership,
    view: &RaceView,
    track_length: f32,
) {
    model.shown.insert(Block::Standings);
    model.text(Slot::StandingsTitle, "STANDINGS", theme::TEXT_FAINT);
    let laps = u32::from(membership.settings.laps);
    for (row, (car, standing)) in standings(membership, view).iter().take(ROWS).enumerate() {
        model.shown.insert(Block::StandingRow(row));
        let own = *car == membership.car;
        let nickname = membership
            .player(*car)
            .map_or(String::new(), |player| player.nickname.clone());
        model.text(
            Slot::StandingPosition(row),
            (row + 1).to_string(),
            if row == 0 {
                theme::AMBER
            } else {
                theme::TEXT_DIM
            },
        );
        model.text(
            Slot::StandingName(row),
            nickname,
            if own { theme::OWN } else { theme::TEXT },
        );
        let (info, color) = match standing {
            RaceStanding::Finished { time_ms } => (race_time(*time_ms), theme::GREEN),
            RaceStanding::Racing { progress } => (
                format!("LAP {}", race::lap(*progress, track_length).clamp(1, laps)),
                theme::TEXT_DIM,
            ),
        };
        model.text(Slot::StandingInfo(row), info, color);
    }
}

/// What a start grade says, and its color.
fn start_grade_label(grade: StartGrade) -> (&'static str, Color) {
    match grade {
        StartGrade::S => ("PERFECT START", theme::AMBER),
        StartGrade::A => ("GREAT START", theme::GREEN),
        StartGrade::B => ("GOOD START", theme::NEON),
        StartGrade::C => ("FAIR START", theme::TEXT),
        StartGrade::D => ("SLOW START", theme::TEXT_DIM),
        StartGrade::E => ("NO START BOOST", theme::TEXT_FAINT),
    }
}

/// `m:ss.mmm`.
fn race_time(time_ms: u32) -> String {
    let minutes = time_ms / 60_000;
    let seconds = time_ms / 1000 % 60;
    let millis = time_ms % 1000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

/// "1st", "2nd", "3rd", "4th"... for a position counted from 1.
fn ordinal(position: usize) -> String {
    let suffix = match (position % 10, position % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{position}{suffix}")
}

#[derive(Component)]
struct Nameplate(CarId);

/// The nickname floating above every car but the followed one.
fn update_nameplates(
    mut commands: Commands,
    fonts: Res<Fonts>,
    current: Res<CurrentLobby>,
    view: Res<RaceView>,
    track: Option<Res<CurrentTrack>>,
    ui_scale: Res<UiScale>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut plates: Query<(Entity, &Nameplate, &mut Node, &mut Text)>,
) {
    let (Some(membership), Ok((camera, camera_transform)), Some(track)) =
        (&current.0, cameras.single(), track)
    else {
        return;
    };
    let followed = view.followed.map(|(car, _)| car);
    let mut wanted: HashMap<CarId, (Vec2, String)> = HashMap::new();
    for snapshot in &view.cars {
        if Some(snapshot.id) == followed {
            continue;
        }
        let height = track.0.surface(snapshot.car.position).height;
        let above = world::position(snapshot.car.position, height + 2.3);
        if camera_transform.translation().distance(above) > NAMEPLATE_DISTANCE {
            continue;
        }
        let Ok(on_screen) = camera.world_to_viewport(camera_transform, above) else {
            continue;
        };
        let nickname = membership
            .player(snapshot.id)
            .map_or(String::new(), |player| player.nickname.clone());
        wanted.insert(snapshot.id, (on_screen / ui_scale.0, nickname));
    }

    for (entity, plate, mut node, mut text) in &mut plates {
        match wanted.remove(&plate.0) {
            Some((position, nickname)) => {
                node.left = px(position.x - NAMEPLATE_WIDTH / 2.0);
                node.top = px(position.y - 22.0);
                if text.0 != nickname {
                    text.0 = nickname;
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (car, (position, nickname)) in wanted {
        commands.spawn((
            Nameplate(car),
            DespawnOnExit(Screen::Lobby),
            Text::new(nickname),
            fonts.text(15.0, 600),
            TextColor(theme::TEXT),
            theme::TEXT_SHADOW,
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                left: px(position.x - NAMEPLATE_WIDTH / 2.0),
                top: px(position.y - 22.0),
                width: px(NAMEPLATE_WIDTH),
                ..default()
            },
        ));
    }
}

fn leave_shortcuts(input: MenuInput, mut dialog: ResMut<LeaveDialog>) {
    if input.key(KeyCode::Escape) || input.gamepad(GamepadButton::Start) {
        dialog.open = !dialog.open;
    } else if dialog.open && input.gamepad(GamepadButton::East) {
        dialog.open = false;
    }
}

fn close_leave_dialog(mut dialog: ResMut<LeaveDialog>) {
    dialog.open = false;
}

/// The button a touchscreen leaves with, in place of Escape and Start.
#[derive(Component)]
struct LeaveButton;

fn ask_to_leave(_: On<Activate>, mut dialog: ResMut<LeaveDialog>) {
    dialog.open = true;
}

#[derive(Component)]
struct LeaveDialogRoot;

#[derive(Component)]
struct StayButton;

fn show_leave_dialog(
    mut commands: Commands,
    dialog: Res<LeaveDialog>,
    roots: Query<Entity, With<LeaveDialogRoot>>,
    stay: Query<Entity, With<StayButton>>,
    fonts: Res<Fonts>,
    mut focus: ResMut<InputFocus>,
) {
    match (dialog.open, roots.single()) {
        (true, Err(_)) => {
            let stay_button = commands
                .spawn((StayButton, button(&fonts, ButtonKind::Secondary, "STAY")))
                .observe(stay_in_lobby)
                .id();
            let leave_button = commands
                .spawn(button(&fonts, ButtonKind::Primary, "LEAVE"))
                .observe(leave)
                .id();
            commands
                .spawn((
                    LeaveDialogRoot,
                    DespawnOnExit(Screen::Lobby),
                    GlobalZIndex(10),
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(theme::BACKDROP),
                ))
                .with_children(|root| {
                    root.spawn((
                        Node {
                            width: px(420),
                            flex_direction: FlexDirection::Column,
                            padding: px(26).all(),
                            row_gap: px(10),
                            border: px(1).all(),
                            border_radius: BorderRadius::all(px(theme::RADIUS)),
                            ..default()
                        },
                        panel(),
                    ))
                    .with_children(|card| {
                        card.spawn((
                            Text::new("LEAVE THE LOBBY?"),
                            fonts.display(34.0),
                            TextColor(theme::TEXT),
                        ));
                        card.spawn((
                            Text::new("You will go back to the lobby list."),
                            fonts.text(15.5, 400),
                            TextColor(theme::TEXT_DIM),
                        ));
                        card.spawn(Node {
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(12),
                            margin: UiRect::top(px(12)),
                            ..default()
                        })
                        .add_children(&[stay_button, leave_button]);
                    });
                });
            focus.set(stay_button, FocusCause::Navigated);
        }
        (false, Ok(root)) => {
            commands.entity(root).despawn();
            if focus.get().is_some_and(|entity| stay.contains(entity)) {
                focus.clear();
            }
        }
        _ => {}
    }
}

fn stay_in_lobby(_: On<Activate>, mut dialog: ResMut<LeaveDialog>) {
    dialog.open = false;
}

fn leave(
    _: On<Activate>,
    network: Res<Network>,
    mut current: ResMut<CurrentLobby>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    leave_lobby(&network, &mut current, &mut next_screen);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn race_times() {
        assert_eq!(race_time(0), "0:00.000");
        assert_eq!(race_time(61_250), "1:01.250");
        assert_eq!(race_time(754_007), "12:34.007");
    }

    #[test]
    fn ordinals() {
        let names: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111]
            .map(ordinal)
            .into();
        assert_eq!(
            names,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st",
                "111th"
            ]
        );
    }
}
