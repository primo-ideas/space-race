//! The best players on the ladder, in a dialog over the first screen.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use space_race_protocol::ranked::{LeaderboardEntry, Tier};

use super::navigation::MenuInput;
use super::theme::{self, Fonts};
use super::widgets::{ButtonKind, button, panel};
use crate::lobby::Ladder;
use crate::screen::Screen;

pub struct LeaderboardPlugin;

impl Plugin for LeaderboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LeaderboardDialog>()
            .add_systems(OnExit(Screen::Login), close)
            .add_systems(
                Update,
                (close_on_back, show)
                    .chain()
                    .run_if(in_state(Screen::Login)),
            );
    }
}

/// Whether the dialog is up.
#[derive(Resource, Default)]
pub struct LeaderboardDialog {
    pub open: bool,
}

#[derive(Component)]
struct LeaderboardRoot;

fn close(mut dialog: ResMut<LeaderboardDialog>) {
    dialog.open = false;
}

fn close_on_back(input: MenuInput, mut dialog: ResMut<LeaderboardDialog>) {
    if dialog.open && input.back() {
        dialog.open = false;
    }
}

fn close_button(_: On<Activate>, mut dialog: ResMut<LeaderboardDialog>) {
    dialog.open = false;
}

/// Draws the dialog while it is open, again whenever the leaderboard changes.
fn show(
    mut commands: Commands,
    dialog: Res<LeaderboardDialog>,
    ladder: Res<Ladder>,
    fonts: Res<Fonts>,
    roots: Query<Entity, With<LeaderboardRoot>>,
    mut focus: ResMut<InputFocus>,
    mut shown: Local<Option<Vec<LeaderboardEntry>>>,
) {
    if !dialog.open {
        if shown.take().is_some() {
            focus.clear();
            for root in &roots {
                commands.entity(root).despawn();
            }
        }
        return;
    }
    if shown.as_ref() == Some(&ladder.leaderboard) {
        return;
    }
    *shown = Some(ladder.leaderboard.clone());
    for root in &roots {
        commands.entity(root).despawn();
    }

    let done = commands
        .spawn(button(&fonts, ButtonKind::Secondary, "CLOSE"))
        .observe(close_button)
        .id();
    commands
        .spawn((
            LeaderboardRoot,
            DespawnOnExit(Screen::Login),
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
                    width: px(460),
                    flex_direction: FlexDirection::Column,
                    padding: px(16).all(),
                    row_gap: px(2),
                    border: px(1).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS)),
                    ..default()
                },
                panel(),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new("LEADERBOARD"),
                    fonts.display(28.0),
                    TextColor(theme::TEXT),
                    Node {
                        margin: UiRect::bottom(px(6)),
                        ..default()
                    },
                ));
                if ladder.leaderboard.is_empty() {
                    card.spawn((
                        Text::new("No ranked race has been played yet."),
                        fonts.text(15.5, 400),
                        TextColor(theme::TEXT_DIM),
                    ));
                }
                // The player's own line stands out, in the color their name has everywhere else.
                let own_place = ladder.profile.and_then(|profile| profile.place);
                for entry in &ladder.leaderboard {
                    let own = own_place == Some(entry.place);
                    card.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(entry.place.to_string()),
                            fonts.display(16.0),
                            TextColor(theme::TEXT_DIM),
                            Node {
                                width: px(26),
                                ..default()
                            },
                        ));
                        row.spawn((
                            Text::new(entry.nickname.clone()),
                            fonts.text(15.0, 600),
                            TextColor(if own { theme::OWN } else { theme::TEXT }),
                            Node {
                                flex_grow: 1.0,
                                flex_basis: px(0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            Text::new(Tier::of(entry.rating).name()),
                            fonts.caption(12.0),
                            theme::CAPTION_SPACING,
                            TextColor(theme::TEXT_DIM),
                        ));
                        row.spawn((
                            Text::new(entry.rating.to_string()),
                            fonts.text(15.0, 700),
                            TextColor(theme::TEXT),
                            TextLayout::justify(Justify::Right),
                            Node {
                                width: px(50),
                                ..default()
                            },
                        ));
                    });
                }
                card.spawn(Node {
                    justify_content: JustifyContent::FlexEnd,
                    margin: UiRect::top(px(6)),
                    ..default()
                })
                .add_child(done);
            });
        });
    focus.set(done, FocusCause::Navigated);
}
