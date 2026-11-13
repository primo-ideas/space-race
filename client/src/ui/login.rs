//! The first screen: the game's title, who the player comes in as, and the ways into a race.
//!
//! A player comes in one of three ways, picked on tabs at the top of the card: as a **guest**,
//! under a generated name the dice can roll again but nobody can type, playing everything but
//! ranked; by **signing in** to their account, with its name and password; or by creating a **new
//! account**. A device that signed in stays signed in, the server knowing it by its key: the card
//! then only says who the player is, with a way to sign out. Whichever it is, a race is one press
//! away: QUICK PLAY, or QUEUE FOR RANKED beside it. Everything else waits under them.
//!
//! It is also the screen a player comes back to, still connected: pressing a button as someone
//! else connects again, and otherwise acts at once.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, LetterSpacing};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use star_racer_protocol::ranked::{PLACEMENT_GAMES, Tier};
use star_racer_protocol::{
    ClientMessage, Login, MAX_NICKNAME_CHARS, MAX_PASSWORD_CHARS, MIN_PASSWORD_CHARS,
    PROTOCOL_VERSION, RejectReason, is_valid_nickname, is_valid_password,
};

use super::leaderboard::LeaderboardDialog;
use super::navigation::MenuInput;
use super::safe_area::SafePadding;
use super::settings::SettingsDialog;
use super::theme::{self, Fonts};
use super::touch::{FullScreenButton, Touchscreen, full_screen_wanted};
use super::widgets::{
    ButtonKind, ButtonLabel, button, button_of_size, hint, hint_row, panel, secret_field,
    set_text, text_field,
};
use crate::lobby::{Ladder, Notice};
use crate::net::{Connection, NetEvent, Network};
use crate::nickname;
use crate::preferences::StoredPreferences;
use crate::reconnect::Reconnect;
use crate::screen::{AfterLobby, Intent, Screen};
use crate::settings::{Script, ScriptedLogin};

pub struct LoginPlugin;

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoginMode>()
            .init_resource::<GuestName>()
            .add_systems(
                OnEnter(Screen::Login),
                (spawn_login, connect_scripted).chain(),
            )
            // Wherever the player is when the server says who they are.
            .add_systems(Update, remember_account)
            .add_systems(
                Update,
                (carry_out_intent, reroll_shortcut, update_login).run_if(in_state(Screen::Login)),
            );
    }
}

/// How a player who is not signed in comes in.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LoginMode {
    /// Not chosen yet: the first screen picks guest, or signing in for a player who once typed an
    /// account name.
    #[default]
    Unset,
    Guest,
    SignIn,
    Create,
}

/// The name a guest plays under this run, rolled by the dice.
#[derive(Resource, Default)]
struct GuestName(String);

/// A tab choosing how to come in.
#[derive(Component)]
struct ModeTab(LoginMode);

#[derive(Component)]
struct Tabs;

#[derive(Component)]
struct SignedInPanel;

#[derive(Component)]
struct SignedInName;

#[derive(Component)]
struct GuestPanel;

#[derive(Component)]
struct GuestNameText;

#[derive(Component)]
struct AccountPanel;

#[derive(Component)]
struct NameField;

#[derive(Component)]
struct PasswordField;

#[derive(Component)]
struct PasswordCaption;

/// A button that needs someone to come in as, and not to be connecting already.
#[derive(Component)]
struct ActionButton;

#[derive(Component)]
struct QuickPlayButton;

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

/// The server the game plays on, which the settings can change while this screen is up.
#[derive(Component)]
struct ServerLine;

fn server_line(network: &Network) -> String {
    format!("{}  ·  protocol {PROTOCOL_VERSION}", network.server())
}

/// A caption over a field or a panel.
fn caption(fonts: &Fonts, text: &str) -> impl Bundle {
    (
        Text::new(text),
        fonts.caption(13.0),
        theme::CAPTION_SPACING,
        TextColor(theme::TEXT_FAINT),
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn_login(
    mut commands: Commands,
    fonts: Res<Fonts>,
    network: Res<Network>,
    connection: Res<Connection>,
    preferences: Res<StoredPreferences>,
    touchscreen: Res<Touchscreen>,
    mut mode: ResMut<LoginMode>,
    mut guest_name: ResMut<GuestName>,
    mut focus: ResMut<InputFocus>,
    mut after: ResMut<AfterLobby>,
) {
    after.0 = Screen::Login;
    // A phone screen is short: the title and the card give up the room they can spare, and the
    // card lays out its two halves side by side.
    let pick = |desk: f32, phone: f32| touchscreen.pick(desk, phone);
    if *mode == LoginMode::Unset {
        *mode = if preferences.get().nickname.is_some() {
            LoginMode::SignIn
        } else {
            LoginMode::Guest
        };
    }
    // The guest name of the run being played, or one the dice roll now.
    if let Connection::Online {
        nickname,
        guest: true,
    } = &*connection
    {
        guest_name.0.clone_from(nickname);
    }
    if guest_name.0.is_empty() {
        guest_name.0 = nickname::generate(None);
    }
    let typed = preferences.get().nickname.clone().unwrap_or_default();

    let name_field = commands
        .spawn((
            NameField,
            text_field(&fonts, "Account name", &typed, MAX_NICKNAME_CHARS),
        ))
        .id();
    let (password_box, password_field) =
        secret_field(&mut commands, &fonts, "Password", MAX_PASSWORD_CHARS);
    commands.entity(password_field).insert(PasswordField);

    let tabs = commands
        .spawn((
            Tabs,
            Node {
                column_gap: px(6),
                ..default()
            },
        ))
        .with_children(|row| {
            for (tab, label) in [
                (LoginMode::Guest, "GUEST"),
                (LoginMode::SignIn, "SIGN IN"),
                (LoginMode::Create, "NEW ACCOUNT"),
            ] {
                row.spawn((ModeTab(tab), button(&fonts, ButtonKind::Quiet, label)))
                    .insert(Node {
                        flex_grow: 1.0,
                        padding: UiRect::axes(px(8), px(pick(7.0, 5.0))),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: px(1.5).all(),
                        border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                        ..default()
                    })
                    .observe(select_mode);
            }
        })
        .id();

    let signed_in = commands
        .spawn((
            SignedInPanel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(pick(8.0, 5.0)),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel.spawn(caption(&fonts, "SIGNED IN AS"));
            panel
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        SignedInName,
                        Text::default(),
                        fonts.text(26.0, 700),
                        TextColor(theme::TEXT),
                        TextLayout::no_wrap(),
                    ));
                    row.spawn(button(&fonts, ButtonKind::Secondary, "SIGN OUT"))
                        .observe(sign_out);
                });
        })
        .id();

    let guest = commands
        .spawn((
            GuestPanel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(pick(8.0, 5.0)),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel.spawn(caption(&fonts, "GUEST NAME"));
            panel
                .spawn(Node {
                    column_gap: px(10),
                    align_items: AlignItems::Stretch,
                    ..default()
                })
                .with_children(|row| {
                    // Not a field: a guest's name is rolled, never typed.
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            padding: UiRect::axes(px(14), px(9)),
                            border: px(1.5).all(),
                            border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
                            ..default()
                        },
                        BackgroundColor(theme::SURFACE),
                        BorderColor::all(theme::PANEL_BORDER),
                    ))
                    .with_child((
                        GuestNameText,
                        Text::default(),
                        fonts.text(21.0, 500),
                        TextColor(theme::TEXT_DIM),
                    ));
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
        })
        .id();

    let account = commands
        .spawn((
            AccountPanel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(pick(6.0, 4.0)),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::End,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(caption(&fonts, "ACCOUNT NAME"));
                    row.spawn((CharCount, caption(&fonts, "")));
                });
        })
        .add_child(name_field)
        .with_children(|panel| {
            panel.spawn((
                PasswordCaption,
                caption(&fonts, "PASSWORD"),
                Node {
                    margin: UiRect::top(px(pick(4.0, 2.0))),
                    ..default()
                },
            ));
        })
        .add_child(password_box)
        .id();

    let identity_half = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_basis: touchscreen.pick(Val::Auto, px(0)),
            justify_content: JustifyContent::Center,
            row_gap: px(pick(10.0, 6.0)),
            ..default()
        })
        .add_children(&[tabs, signed_in, guest, account])
        .with_children(|card| {
            // Texts that come and go sit in a box of fixed minimum height, so the card does not
            // jump when they change.
            card.spawn(Node {
                min_height: px(pick(20.0, 16.0)),
                ..default()
            })
            .with_child((
                FieldHelp,
                Text::default(),
                fonts.text(14.0, 400),
                TextColor(theme::TEXT_DIM),
            ));
            card.spawn(Node {
                min_height: px(pick(22.0, 18.0)),
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
    let mut quick_play_button = None;
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
            quick_play_button = Some(
                actions
                    .spawn((
                        ActionButton,
                        QuickPlayButton,
                        button_of_size(&fonts, ButtonKind::Primary, "QUICK PLAY", pick(30.0, 28.0)),
                    ))
                    .insert(big(pick(78.0, 72.0)))
                    .observe(quick_play)
                    .id(),
            );
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
                padding: px(pick(24.0, 12.0)).all(),
                row_gap: px(pick(14.0, 7.0)),
                border: px(1).all(),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                ..default()
            },
            panel(),
        ))
        .add_children(&[identity_half, actions_half])
        .id();

    // Enter in a field is QUICK PLAY.
    commands.entity(name_field).observe(quick_play);
    commands.entity(password_field).observe(quick_play);

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
                fonts.display(pick(72.0, 40.0)),
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
                    margin: UiRect::bottom(px(pick(14.0, 6.0))),
                    // On a phone the card needs the room more than the line does.
                    display: touchscreen.pick(Display::Flex, Display::None),
                    ..default()
                },
            ));
        })
        .add_child(card)
        .with_children(|root| {
            // Under the card, everything that is not getting straight into a race.
            root.spawn(Node {
                column_gap: px(10),
                margin: UiRect::top(px(pick(14.0, 6.0))),
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
                // Needs nobody to come in as and no server: always there, as the settings are.
                row.spawn(button(&fonts, ButtonKind::Secondary, "GARAGE"))
                    .insert(small(120.0))
                    .observe(open_garage);
                row.spawn(button(&fonts, ButtonKind::Secondary, "SETTINGS"))
                    .insert(small(130.0))
                    .observe(open_settings);
                // A page cannot close the tab it is in, so only the native game has a way out.
                if cfg!(not(target_arch = "wasm32")) {
                    row.spawn(button(&fonts, ButtonKind::Secondary, "QUIT"))
                        .insert(small(120.0))
                        .observe(quit);
                }
                // In its place on the web, the page that offers the game to download (see
                // docs/releases.md); a phone has no use for it.
                if cfg!(target_arch = "wasm32") && !touchscreen.0 {
                    row.spawn(button(&fonts, ButtonKind::Secondary, "DOWNLOAD"))
                        .insert(small(150.0))
                        .observe(open_downloads);
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
                        ServerLine,
                        Text::new(server_line(&network)),
                        fonts.text(13.0, 400),
                        TextColor(theme::TEXT_FAINT),
                    ),
                    (
                        hint_row(),
                        children![
                            hint(&fonts, "Enter", "A", "Quick play"),
                            hint(&fonts, "F2", "Y", "Other guest name"),
                        ],
                    ),
                ],
            ));
        });

    // Typing first for a player who has to sign in; otherwise, the race.
    let signed_in_already = preferences.get().account.is_some();
    let target = match *mode {
        LoginMode::SignIn | LoginMode::Create if !signed_in_already => name_field,
        _ => quick_play_button.expect("spawned above"),
    };
    focus.set(target, FocusCause::Navigated);
}

/// A die, drawn with nodes, for the button that rolls a new guest name.
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
    mut guest_name: ResMut<GuestName>,
    mut mode: ResMut<LoginMode>,
) {
    let Some(login) = script.login.take() else {
        return;
    };
    // Should it fail, the card shows the way it was tried.
    *mode = match &login {
        ScriptedLogin::Guest => LoginMode::Guest,
        ScriptedLogin::Account { create: true, .. } => LoginMode::Create,
        ScriptedLogin::Account { .. } => LoginMode::SignIn,
    };
    *intent = if script.quick_play {
        Intent::QuickPlay
    } else if script.ranked {
        Intent::Ranked
    } else {
        Intent::Browse
    };
    let (nickname, login) = match login {
        ScriptedLogin::Guest => {
            let name = guest_name.0.clone();
            (name.clone(), Login::Guest { name })
        }
        ScriptedLogin::Account {
            name,
            password,
            create,
        } => (
            name.clone(),
            Login::Password {
                name,
                password,
                create,
            },
        ),
    };
    if guest_name.0.is_empty() {
        guest_name.0 = nickname::generate(None);
    }
    network.connect(nickname, login, &mut connection);
}

/// Keeps what the server says about who the player is: the account this device is signed in
/// to once welcomed to one, nothing once the server no longer knows the device.
fn remember_account(
    mut events: MessageReader<NetEvent>,
    mut preferences: ResMut<StoredPreferences>,
    mut network: ResMut<Network>,
    mut passwords: Query<&mut EditableText, With<PasswordField>>,
) {
    for event in events.read() {
        match event {
            NetEvent::Welcome { identity, .. } if !identity.guest => {
                let name = identity.name.clone();
                if preferences.get().account.as_ref() != Some(&name) {
                    preferences.update(|preferences| {
                        preferences.account = Some(name.clone());
                        preferences.nickname = Some(name);
                    });
                }
                // Signed in: the password has done its work.
                for mut field in &mut passwords {
                    set_text(&mut field, "");
                }
            }
            NetEvent::Rejected(RejectReason::NotSignedIn) => {
                preferences.update(|preferences| preferences.account = None);
                network.forget();
            }
            _ => {}
        }
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

/// How the card would have the player come in, if it can: the name to show while connecting, the
/// login, and whether it is as a guest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    nickname: String,
    login: Login,
    guest: bool,
}

/// What the card asks for, if it is enough to come in with.
fn request(
    account: Option<&str>,
    mode: LoginMode,
    guest_name: &str,
    name: &str,
    password: &str,
) -> Option<Request> {
    if let Some(account) = account {
        return Some(Request {
            nickname: account.to_owned(),
            login: Login::Device,
            guest: false,
        });
    }
    match mode {
        LoginMode::Unset | LoginMode::Guest => Some(Request {
            nickname: guest_name.to_owned(),
            login: Login::Guest {
                name: guest_name.to_owned(),
            },
            guest: true,
        }),
        LoginMode::SignIn | LoginMode::Create => {
            let create = mode == LoginMode::Create;
            let password_ok = if create {
                is_valid_password(password)
            } else {
                !password.is_empty()
            };
            (is_valid_nickname(name) && password_ok).then(|| Request {
                nickname: name.to_owned(),
                login: Login::Password {
                    name: name.to_owned(),
                    password: password.to_owned(),
                    create,
                },
                guest: false,
            })
        }
    }
}

/// Whether the player is online already the way `request` would bring them in.
fn online_as(connection: &Connection, request: &Request) -> bool {
    let Connection::Online { nickname, guest } = connection else {
        return false;
    };
    *guest == request.guest && nickname.to_lowercase() == request.nickname.to_lowercase()
}

/// What a button on this screen does first: makes sure the player is connected, the way the card
/// says.
#[derive(SystemParam)]
struct Start<'w, 's> {
    names: Query<'w, 's, &'static EditableText, With<NameField>>,
    passwords: Query<'w, 's, &'static EditableText, With<PasswordField>>,
    mode: Res<'w, LoginMode>,
    guest_name: Res<'w, GuestName>,
    preferences: ResMut<'w, StoredPreferences>,
    network: ResMut<'w, Network>,
    connection: ResMut<'w, Connection>,
    intent: ResMut<'w, Intent>,
}

impl Start<'_, '_> {
    /// What the card asks for now, if it is enough to come in with.
    fn request(&self) -> Option<Request> {
        let name = self
            .names
            .single()
            .map(|field| field.value().to_string())
            .unwrap_or_default();
        let password = self
            .passwords
            .single()
            .map(|field| field.value().to_string())
            .unwrap_or_default();
        request(
            self.preferences.get().account.as_deref(),
            *self.mode,
            &self.guest_name.0,
            &name,
            &password,
        )
    }

    /// Connects if need be, and asks for `wanted` once connected.
    fn go(&mut self, wanted: Intent) {
        if matches!(*self.connection, Connection::Connecting { .. }) {
            return;
        }
        let Some(request) = self.request() else {
            return;
        };
        if let Login::Password { name, .. } = &request.login {
            // The name typed is offered again next time; the password never is.
            let name = name.clone();
            self.preferences
                .update(|preferences| preferences.nickname = Some(name));
        }
        *self.intent = wanted;
        if !online_as(&self.connection, &request) {
            self.network
                .connect(request.nickname, request.login, &mut self.connection);
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

fn select_mode(
    activate: On<Activate>,
    tabs: Query<&ModeTab>,
    connection: Res<Connection>,
    mut mode: ResMut<LoginMode>,
    names: Query<Entity, With<NameField>>,
    mut focus: ResMut<InputFocus>,
) {
    if matches!(*connection, Connection::Connecting { .. }) {
        return;
    }
    let Ok(tab) = tabs.get(activate.entity) else {
        return;
    };
    *mode = tab.0;
    // Choosing to sign in is choosing to type.
    if matches!(tab.0, LoginMode::SignIn | LoginMode::Create)
        && let Ok(field) = names.single()
    {
        focus.set(field, FocusCause::Navigated);
    }
}

/// Signs this device out: the server forgets it, if it can be told, and otherwise the device
/// becomes a new one it never knew.
fn sign_out(
    _: On<Activate>,
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
    mut preferences: ResMut<StoredPreferences>,
    mut mode: ResMut<LoginMode>,
    ladder: Res<Ladder>,
) {
    if ladder.queue.is_some() {
        network.send(ClientMessage::LeaveQueue);
    }
    if matches!(*connection, Connection::Online { guest: false, .. }) {
        network.sign_out(&mut connection);
    } else {
        network.disconnect(&mut connection);
        network.forget();
        network.new_device();
    }
    preferences.update(|preferences| preferences.account = None);
    *mode = LoginMode::SignIn;
}

fn reroll(
    _: On<Activate>,
    connection: Res<Connection>,
    mode: Res<LoginMode>,
    preferences: Res<StoredPreferences>,
    mut guest_name: ResMut<GuestName>,
) {
    if matches!(*connection, Connection::Connecting { .. })
        || *mode != LoginMode::Guest
        || preferences.get().account.is_some()
    {
        return;
    }
    guest_name.0 = nickname::generate(Some(&guest_name.0));
}

fn open_garage(_: On<Activate>, mut next: ResMut<NextState<Screen>>) {
    next.set(Screen::Garage);
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

/// Opens the download page beside the game, in a tab of its own.
fn open_downloads(_: On<Activate>) {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window()
        && window
            .open_with_url_and_target("download.html", "_blank")
            .is_err()
    {
        warn!("the download page could not be opened");
    }
}

/// Closes the game. The window's own close button does the same, but a gamepad cannot reach it.
fn quit(_: On<Activate>, mut exit: MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
}

/// Which part of the card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    SignedIn,
    Guest,
    Account,
}

fn shown(account: Option<&str>, mode: LoginMode) -> Shown {
    match (account, mode) {
        (Some(_), _) => Shown::SignedIn,
        (None, LoginMode::SignIn | LoginMode::Create) => Shown::Account,
        (None, _) => Shown::Guest,
    }
}

/// What the line under the fields says, and in which color.
fn help(
    shown: Shown,
    mode: LoginMode,
    name: &str,
    password: &str,
) -> (String, Color) {
    match shown {
        Shown::SignedIn => ("Signed in on this device.".into(), theme::TEXT_FAINT),
        Shown::Guest => (
            "Guests play everything but ranked.".into(),
            theme::TEXT_FAINT,
        ),
        Shown::Account => {
            let chars = name.chars().count();
            if chars == 0 {
                let text = if mode == LoginMode::Create {
                    "Pick the name other players will see."
                } else {
                    "Your account name and password."
                };
                (text.into(), theme::TEXT_DIM)
            } else if name.trim() != name {
                ("A name can't start or end with a space.".into(), theme::AMBER)
            } else if !is_valid_nickname(name) {
                ("A name can't contain control characters.".into(), theme::AMBER)
            } else if mode == LoginMode::Create
                && !password.is_empty()
                && password.chars().count() < MIN_PASSWORD_CHARS
            {
                (
                    format!("A password needs at least {MIN_PASSWORD_CHARS} characters."),
                    theme::AMBER,
                )
            } else if mode == LoginMode::Create {
                ("This is how other players will see you.".into(), theme::TEXT_FAINT)
            } else {
                (String::new(), theme::TEXT_DIM)
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_login(
    mut commands: Commands,
    fields: Query<(&EditableText, Has<PasswordField>), Or<(With<NameField>, With<PasswordField>)>>,
    connection: Res<Connection>,
    network: Res<Network>,
    reconnect: Res<Reconnect>,
    ladder: Res<Ladder>,
    notice: Res<Notice>,
    mode: Res<LoginMode>,
    guest_name: Res<GuestName>,
    preferences: Res<StoredPreferences>,
    time: Res<Time<Real>>,
    action_buttons: Query<(Entity, Has<InteractionDisabled>, Has<RankedButton>), With<ActionButton>>,
    ranked_buttons: Query<&Children, With<RankedButton>>,
    mut tabs: Query<(&ModeTab, &mut ButtonKind)>,
    mut panels: Query<(
        &mut Node,
        Has<Tabs>,
        Has<SignedInPanel>,
        Has<GuestPanel>,
        Has<AccountPanel>,
    )>,
    mut texts: ParamSet<(
        Query<&mut Text, With<CharCount>>,
        Query<(&mut Text, &mut TextColor), With<FieldHelp>>,
        Query<(&mut Text, &mut TextColor), With<Status>>,
        Query<&mut Text, With<ButtonLabel>>,
        Query<(&mut Text, &mut TextColor), With<RankLine>>,
        Query<&mut Text, With<ServerLine>>,
        Query<&mut Text, With<SignedInName>>,
        Query<&mut Text, With<GuestNameText>>,
    )>,
) {
    for mut text in &mut texts.p5() {
        set(&mut text, server_line(&network));
    }
    let (mut name, mut password) = (String::new(), String::new());
    for (field, secret) in &fields {
        if secret {
            password = field.value().to_string();
        } else {
            name = field.value().to_string();
        }
    }
    let account = preferences.get().account.clone();
    let shown = shown(account.as_deref(), *mode);
    let connecting = matches!(*connection, Connection::Connecting { .. });

    for (mut node, tabs_row, signed_in, guest, account_panel) in &mut panels {
        let visible = if tabs_row {
            shown != Shown::SignedIn
        } else if signed_in {
            shown == Shown::SignedIn
        } else if guest {
            shown == Shown::Guest
        } else if account_panel {
            shown == Shown::Account
        } else {
            continue;
        };
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (tab, mut kind) in &mut tabs {
        let wanted = if tab.0 == *mode {
            ButtonKind::Secondary
        } else {
            ButtonKind::Quiet
        };
        if *kind != wanted {
            *kind = wanted;
        }
    }
    for mut text in &mut texts.p6() {
        set(&mut text, account.clone().unwrap_or_default());
    }
    for mut text in &mut texts.p7() {
        set(&mut text, guest_name.0.clone());
    }
    for mut text in &mut texts.p0() {
        set(
            &mut text,
            format!("{} / {MAX_NICKNAME_CHARS}", name.chars().count()),
        );
    }
    let (help_text, help_color) = help(shown, *mode, &name, &password);
    for (mut text, mut color) in &mut texts.p1() {
        set(&mut text, help_text.clone());
        color.0 = help_color;
    }

    // A guest, or a player about to come in as one, has no ranked: the line says why.
    let as_guest = match &*connection {
        Connection::Online { guest, .. } if !connecting => *guest && shown == Shown::Guest,
        _ => shown == Shown::Guest,
    };
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
    let (line, line_color) = if as_guest {
        ("SIGN IN TO PLAY RANKED".to_owned(), theme::TEXT_FAINT)
    } else {
        rank_line(&ladder)
    };
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

    let ready = request(
        account.as_deref(),
        *mode,
        &guest_name.0,
        &name,
        &password,
    )
    .is_some();
    for (button, disabled, ranked) in &action_buttons {
        // Leaving the queue is always possible.
        let enabled = (ready && !connecting && !(ranked && as_guest)) || (ranked && ladder.queue.is_some());
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
        RejectReason::InvalidNickname => "The server refused this name.".into(),
        RejectReason::AuthenticationFailed => "The server could not verify your identity.".into(),
        // Said to the connection that was replaced: the newest one of a player wins.
        RejectReason::AlreadyConnected => {
            "You connected again from another window, which took over from this one.".into()
        }
        RejectReason::NotSignedIn => "This device is signed out: sign in again.".into(),
        RejectReason::WrongPassword => "Wrong account name or password.".into(),
        RejectReason::NameTaken => "This name is taken: pick another, or sign in.".into(),
        RejectReason::InvalidPassword => format!(
            "A password needs {MIN_PASSWORD_CHARS} to {MAX_PASSWORD_CHARS} characters."
        ),
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

    /// A device signed in to an account comes back as it, whatever the tabs say.
    #[test]
    fn a_signed_in_device_comes_back_as_its_account() {
        let request = request(Some("Ada"), LoginMode::Guest, "Neon Falcon", "", "").unwrap();
        assert_eq!(request.login, Login::Device);
        assert_eq!(request.nickname, "Ada");
        assert!(!request.guest);
        assert_eq!(shown(Some("Ada"), LoginMode::Create), Shown::SignedIn);
    }

    /// A guest comes in under the name the dice rolled, and only that.
    #[test]
    fn a_guest_comes_in_under_the_rolled_name() {
        let request = request(None, LoginMode::Guest, "Neon Falcon", "Typed", "secret").unwrap();
        assert_eq!(
            request.login,
            Login::Guest {
                name: "Neon Falcon".into()
            }
        );
        assert!(request.guest);
        assert_eq!(shown(None, LoginMode::Guest), Shown::Guest);
    }

    /// Signing in needs a name and a password; creating an account, a password long enough.
    #[test]
    fn signing_in_needs_a_name_and_a_password() {
        // In order: the guest name, which signing in ignores, the account name, the password.
        assert_eq!(request(None, LoginMode::SignIn, "x", "", "secret"), None);
        assert_eq!(request(None, LoginMode::SignIn, "x", "Ada", ""), None);
        assert_eq!(request(None, LoginMode::SignIn, "x", " Ada", "secret"), None);
        let request_in = request(None, LoginMode::SignIn, "x", "Ada", "short").unwrap();
        assert_eq!(
            request_in.login,
            Login::Password {
                name: "Ada".into(),
                password: "short".into(),
                create: false
            }
        );

        assert_eq!(request(None, LoginMode::Create, "x", "Ada", "short"), None);
        let created = request(None, LoginMode::Create, "x", "Ada", "long enough").unwrap();
        assert!(matches!(created.login, Login::Password { create: true, .. }));
    }

    /// Already online the way the card asks: nothing to connect again for.
    #[test]
    fn online_as_the_same_player_needs_no_new_connection() {
        let online = Connection::Online {
            nickname: "Ada".into(),
            guest: false,
        };
        let device = request(Some("ada"), LoginMode::Guest, "Neon Falcon", "", "").unwrap();
        assert!(online_as(&online, &device));
        let as_guest = request(None, LoginMode::Guest, "Ada", "", "").unwrap();
        assert!(!online_as(&online, &as_guest), "a guest is someone else");
    }

    #[test]
    fn the_help_line_says_what_is_missing() {
        assert_eq!(
            help(Shown::Account, LoginMode::Create, "Ada", "short").0,
            format!("A password needs at least {MIN_PASSWORD_CHARS} characters.")
        );
        assert_eq!(
            help(Shown::Account, LoginMode::SignIn, " Ada", "").1,
            theme::AMBER
        );
    }
}
