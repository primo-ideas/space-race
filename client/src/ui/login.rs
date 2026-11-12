//! The first screen: the game's title, a nickname, and the ways into a race.
//!
//! The field comes filled in, with the nickname last played under or a generated one, so a race is
//! one press away from the moment the game opens, even with a gamepad: QUICK PLAY, or QUEUE FOR
//! RANKED beside it. Everything else waits under them. The dice button, or Y on a gamepad, rolls a
//! new nickname.
//!
//! It is also the screen a player comes back to, still connected: pressing a button under another
//! nickname connects again, and otherwise acts at once.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, LetterSpacing};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use star_racer_protocol::ranked::{PLACEMENT_GAMES, Tier};
use star_racer_protocol::{
    ClientMessage, MAX_NICKNAME_CHARS, PROTOCOL_VERSION, RejectReason, is_valid_nickname,
};

use super::leaderboard::LeaderboardDialog;
use super::navigation::MenuInput;
use super::safe_area::SafePadding;
use super::settings::SettingsDialog;
use super::theme::{self, Fonts};
use super::touch::{FullScreenButton, Touchscreen, full_screen_wanted};
use super::widgets::{
    ButtonKind, ButtonLabel, button, button_of_size, hint, hint_row, panel, set_text, text_field,
};
use crate::lobby::{Ladder, Notice};
use crate::net::{Connection, Network};
use crate::nickname;
use crate::preferences::{OfferedNickname, StoredPreferences};
use crate::reconnect::Reconnect;
use crate::screen::{AfterLobby, Intent, Screen};
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
            (carry_out_intent, reroll_shortcut, update_login).run_if(in_state(Screen::Login)),
        );
    }
}

#[derive(Component)]
struct NicknameField;

/// A button that needs a valid nickname, and not to be connecting already.
#[derive(Component)]
struct ActionButton;

#[derive(Component)]
struct RankedButton;

/// Where the player stands on the ladder, or how the queue is going.
#[derive(Component)]
struct RankLine;

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
    mut after: ResMut<AfterLobby>,
) {
    after.0 = Screen::Login;
    // A phone screen is short: the title and the card give up the room they can spare, and the
    // card lays out its two halves side by side.
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
            text_field(&fonts, "Nickname", &initial, MAX_NICKNAME_CHARS),
        ))
        .id();

    let nickname_half = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_basis: touchscreen.pick(Val::Auto, px(0)),
            justify_content: JustifyContent::Center,
            row_gap: px(pick(12.0, 7.0)),
            ..default()
        })
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
            card.spawn(Node {
                min_height: px(22),
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

    // The two ways into a race, as big as the screen allows: the reason anyone opened the game.
    let actions_half = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            width: touchscreen.pick(percent(100), px(310)),
            justify_content: JustifyContent::Center,
            row_gap: px(pick(10.0, 7.0)),
            ..default()
        })
        .with_children(|actions| {
            let big = |height: f32| Node {
                height: px(height),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: px(1.5).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                ..default()
            };
            actions
                .spawn((
                    ActionButton,
                    button_of_size(&fonts, ButtonKind::Primary, "QUICK PLAY", pick(30.0, 28.0)),
                ))
                .insert(big(pick(78.0, 72.0)))
                .observe(quick_play);
            actions
                .spawn((
                    ActionButton,
                    RankedButton,
                    button_of_size(&fonts, ButtonKind::Ranked, RANKED_LABEL, pick(20.0, 18.0)),
                ))
                .insert(big(pick(56.0, 52.0)))
                .observe(toggle_ranked);
            actions.spawn((
                RankLine,
                Text::default(),
                fonts.caption(13.0),
                theme::CAPTION_SPACING,
                TextColor(theme::TEXT_DIM),
                TextLayout::justify(Justify::Center),
            ));
        })
        .id();

    let card = commands
        .spawn((
            Node {
                width: px(pick(440.0, 680.0)),
                flex_direction: touchscreen.pick(FlexDirection::Column, FlexDirection::Row),
                column_gap: px(18),
                padding: px(pick(28.0, 14.0)).all(),
                row_gap: px(pick(14.0, 7.0)),
                border: px(1).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                ..default()
            },
            panel(),
        ))
        .add_children(&[nickname_half, actions_half])
        .id();

    commands.entity(field).observe(quick_play);

    commands
        .spawn((
            DespawnOnExit(Screen::Login),
            SafePadding::default(),
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
                Text::new("STAR RACER"),
                fonts.display(pick(92.0, 48.0)),
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
                    margin: UiRect::bottom(px(pick(46.0, 10.0))),
                    ..default()
                },
            ));
        })
        .add_child(card)
        .with_children(|root| {
            // Under the card, everything that is not getting straight into a race.
            root.spawn(Node {
                column_gap: px(10),
                margin: UiRect::top(px(pick(18.0, 8.0))),
                ..default()
            })
            .with_children(|row| {
                let small = |width: f32| Node {
                    width: px(width),
                    padding: UiRect::axes(px(12), px(9)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: px(1.5).all(),
                    border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                    ..default()
                };
                row.spawn((
                    ActionButton,
                    button(&fonts, ButtonKind::Secondary, "BROWSE LOBBIES"),
                ))
                .insert(small(190.0))
                .observe(browse);
                row.spawn((
                    ActionButton,
                    button(&fonts, ButtonKind::Secondary, "LEADERBOARD"),
                ))
                .insert(small(170.0))
                .observe(ask_for_leaderboard);
                // Needs no nickname and no server: it is always there.
                row.spawn(button(&fonts, ButtonKind::Secondary, "SETTINGS"))
                    .insert(small(130.0))
                    .observe(open_settings);
                // A page cannot close the tab it is in, so only the native game has a way out.
                if cfg!(not(target_arch = "wasm32")) {
                    row.spawn(button(&fonts, ButtonKind::Secondary, "QUIT"))
                        .insert(small(120.0))
                        .observe(quit);
                }
                // In its place on a phone's browser, the way to the whole screen.
                if full_screen_wanted(*touchscreen) {
                    row.spawn((
                        FullScreenButton,
                        button(&fonts, ButtonKind::Secondary, "FULL SCREEN"),
                    ))
                    .insert(small(170.0));
                }
            });
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(28),
                    right: px(28),
                    bottom: px(22),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    // The address is for whoever is testing the game; the hints are for keys.
                    display: touchscreen.pick(Display::Flex, Display::None),
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
                            hint(&fonts, "Enter", "A", "Quick play"),
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
    mut intent: ResMut<Intent>,
) {
    if let Some(nickname) = script.nickname.take() {
        *intent = if script.quick_play {
            Intent::QuickPlay
        } else if script.ranked {
            Intent::Ranked
        } else {
            Intent::Browse
        };
        network.connect(nickname, &mut connection);
    }
}

/// Does what the player asked for, as soon as they are connected: at once if they already were.
/// A connection that failed leaves nothing to do; the screen says why.
fn carry_out_intent(
    mut intent: ResMut<Intent>,
    connection: Res<Connection>,
    network: Res<Network>,
    mut leaderboard: ResMut<LeaderboardDialog>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    match *connection {
        Connection::Online { .. } => {}
        Connection::Connecting { .. } => return,
        _ => {
            *intent = Intent::None;
            return;
        }
    }
    match std::mem::take(&mut *intent) {
        Intent::None => {}
        Intent::Browse => next_screen.set(Screen::Lobbies),
        Intent::QuickPlay => network.send(ClientMessage::QuickPlay),
        Intent::Ranked => network.send(ClientMessage::QueueRanked),
        Intent::Leaderboard => leaderboard.open = true,
    }
}

/// What a button on this screen does first: makes sure the player is connected, under the
/// nickname in the field.
#[derive(SystemParam)]
struct Start<'w, 's> {
    fields: Query<'w, 's, &'static EditableText, With<NicknameField>>,
    offered: Res<'w, OfferedNickname>,
    preferences: ResMut<'w, StoredPreferences>,
    network: ResMut<'w, Network>,
    connection: ResMut<'w, Connection>,
    intent: ResMut<'w, Intent>,
}

impl Start<'_, '_> {
    /// Connects if need be, and asks for `wanted` once connected.
    fn go(&mut self, wanted: Intent) {
        if matches!(*self.connection, Connection::Connecting { .. }) {
            return;
        }
        let Ok(field) = self.fields.single() else {
            return;
        };
        let nickname = field.value().to_string();
        if !is_valid_nickname(&nickname) {
            return;
        }
        // A nickname the dice offered and the player simply played with is not remembered: they
        // get a new one next time. Editing it makes it theirs.
        let theirs = self.offered.0.as_deref() != Some(nickname.as_str());
        self.preferences
            .update(|preferences| preferences.nickname = theirs.then(|| nickname.clone()));

        *self.intent = wanted;
        let connected_as_them = matches!(&*self.connection, Connection::Online { nickname: online } if *online == nickname);
        if !connected_as_them {
            self.network.connect(nickname, &mut self.connection);
        }
    }
}

fn quick_play(_: On<Activate>, mut start: Start) {
    start.go(Intent::QuickPlay);
}

/// Enters the ranked queue, or leaves it for a player already in it.
fn toggle_ranked(_: On<Activate>, mut start: Start, ladder: Res<Ladder>) {
    if ladder.queue.is_some() {
        start.network.send(ClientMessage::LeaveQueue);
    } else {
        start.go(Intent::Ranked);
    }
}

fn browse(_: On<Activate>, mut start: Start, ladder: Res<Ladder>) {
    // The queue is for this screen: a match would pull the player out of the lobby list.
    if ladder.queue.is_some() {
        start.network.send(ClientMessage::LeaveQueue);
    }
    start.go(Intent::Browse);
}

fn ask_for_leaderboard(_: On<Activate>, mut start: Start) {
    start.go(Intent::Leaderboard);
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

fn open_settings(_: On<Activate>, mut settings: ResMut<SettingsDialog>) {
    settings.open = true;
}

fn reroll_shortcut(
    input: MenuInput,
    buttons: Query<Entity, With<RerollButton>>,
    settings: Res<SettingsDialog>,
    mut commands: Commands,
) {
    // Not behind the settings, where Y may be on its way to being bound.
    if settings.open {
        return;
    }
    if input.key(KeyCode::F2) || input.gamepad(GamepadButton::North) {
        for entity in &buttons {
            commands.trigger(Activate { entity });
        }
    }
}

/// Closes the game. The window's own close button does the same, but a gamepad cannot reach it.
fn quit(_: On<Activate>, mut exit: MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
}

fn update_login(
    mut commands: Commands,
    fields: Query<&EditableText, With<NicknameField>>,
    connection: Res<Connection>,
    network: Res<Network>,
    reconnect: Res<Reconnect>,
    ladder: Res<Ladder>,
    notice: Res<Notice>,
    time: Res<Time<Real>>,
    action_buttons: Query<(Entity, Has<InteractionDisabled>), With<ActionButton>>,
    ranked_buttons: Query<&Children, With<RankedButton>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<CharCount>>,
        Query<(&mut Text, &mut TextColor), With<FieldHelp>>,
        Query<(&mut Text, &mut TextColor), With<Status>>,
        Query<&mut Text, With<ButtonLabel>>,
        Query<(&mut Text, &mut TextColor), With<RankLine>>,
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

    let now = time.elapsed_secs_f64();
    let label = ladder.queue.map_or_else(
        || RANKED_LABEL.to_owned(),
        |queue| searching_label(now - queue.since),
    );
    for children in &ranked_buttons {
        for child in children {
            if let Ok(mut text) = texts.p3().get_mut(*child) {
                set(&mut text, label.clone());
            }
        }
    }
    let (line, line_color) = rank_line(&ladder);
    for (mut text, mut color) in &mut texts.p4() {
        set(&mut text, line.clone());
        color.0 = line_color;
    }

    // Lost, and being tried again: the screen says so rather than why it was lost.
    let reconnecting = reconnect
        .status(&connection, now)
        .map(|status| (status, theme::AMBER))
        .or_else(|| {
            notice
                .current(now)
                .map(|notice| (notice.to_owned(), theme::AMBER))
        });
    let (status, status_color) = reconnecting.unwrap_or_else(|| match &*connection {
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
    });
    for (mut text, mut color) in &mut texts.p2() {
        set(&mut text, status.clone());
        color.0 = status_color;
    }

    let enabled = valid && !connecting;
    for (button, disabled) in &action_buttons {
        if enabled && disabled {
            commands.entity(button).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(button).insert(InteractionDisabled);
        }
    }
}

const RANKED_LABEL: &str = "QUEUE FOR RANKED";

/// What the ranked button says to a player who has been in the queue for `seconds`.
fn searching_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("SEARCHING  {}:{:02}  ·  CANCEL", seconds / 60, seconds % 60)
}

/// The line under the ranked button: how the queue is going, or where the player stands.
fn rank_line(ladder: &Ladder) -> (String, Color) {
    if let Some(queue) = ladder.queue {
        let players = match queue.players {
            0 | 1 => "1 PLAYER".to_owned(),
            players => format!("{players} PLAYERS"),
        };
        return (format!("{players} IN THE QUEUE"), theme::MAGENTA);
    }
    let Some(profile) = ladder.profile else {
        return (String::new(), theme::TEXT_DIM);
    };
    let standing = if profile.placed() {
        format!("{}  {}", Tier::of(profile.rating).name(), profile.rating)
    } else {
        format!("PLACEMENT  {} / {PLACEMENT_GAMES}", profile.games)
    };
    let place = profile
        .place
        .map_or(String::new(), |place| format!("  ·  #{place}"));
    match ladder.last_change {
        Some(change) => {
            let gained = change.after - change.before;
            let color = if gained >= 0 {
                theme::GREEN
            } else {
                theme::RED
            };
            (format!("{standing}{place}  ·  {gained:+}"), color)
        }
        None => (format!("{standing}{place}"), theme::TEXT_DIM),
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
        // Said to the connection that was replaced: the newest one of a player wins.
        RejectReason::AlreadyConnected => {
            "You connected again from another window, which took over from this one.".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use star_racer_protocol::ranked::RankedProfile;
    use star_racer_protocol::{CarId, RatingChange};

    use super::*;
    use crate::lobby::Queueing;

    fn profile(rating: i32, games: u32, place: Option<u32>) -> RankedProfile {
        RankedProfile {
            rating,
            games,
            wins: 0,
            place,
        }
    }

    #[test]
    fn the_ranked_button_counts_the_wait() {
        assert_eq!(searching_label(0.0), "SEARCHING  0:00  ·  CANCEL");
        assert_eq!(searching_label(72.9), "SEARCHING  1:12  ·  CANCEL");
    }

    #[test]
    fn nothing_is_said_before_the_server_has() {
        assert_eq!(rank_line(&Ladder::default()).0, "");
    }

    #[test]
    fn a_new_player_is_being_placed() {
        let ladder = Ladder {
            profile: Some(profile(1024, 2, Some(7))),
            ..default()
        };
        assert_eq!(rank_line(&ladder).0, "PLACEMENT  2 / 5  ·  #7");
    }

    #[test]
    fn a_placed_player_sees_their_tier_and_what_the_last_race_did() {
        let mut ladder = Ladder {
            profile: Some(profile(1130, 12, Some(3))),
            ..default()
        };
        assert_eq!(rank_line(&ladder).0, "GOLD  1130  ·  #3");

        ladder.last_change = Some(RatingChange {
            car: CarId(1),
            before: 1142,
            after: 1130,
        });
        let (line, color) = rank_line(&ladder);
        assert_eq!(line, "GOLD  1130  ·  #3  ·  -12");
        assert_eq!(color, theme::RED);
    }

    /// In the queue, the queue is what matters.
    #[test]
    fn a_queueing_player_sees_the_queue() {
        let mut ladder = Ladder {
            profile: Some(profile(1130, 12, Some(3))),
            queue: Some(Queueing {
                players: 1,
                since: 0.0,
            }),
            ..default()
        };
        assert_eq!(rank_line(&ladder).0, "1 PLAYER IN THE QUEUE");
        ladder.queue = Some(Queueing {
            players: 3,
            since: 0.0,
        });
        assert_eq!(rank_line(&ladder).0, "3 PLAYERS IN THE QUEUE");
    }
}
