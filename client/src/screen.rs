//! The screens of the game, and what the server's answers move the player to.
//!
//! ```text
//!         ┌────── quick play, a ranked match ──────┐
//!         │                                        ▼
//!       Login ──browse lobbies──▶ Lobbies ──join──▶ Lobby
//!         ▲ ▲                        │               │
//!         │ └──────────back──────────┘               │
//!         └─────────────── leave ────────────────────┘
//! ```
//!
//! Any disconnection or rejection goes back to the login screen, which says why. It is the screen
//! every way into a race starts from, so being connected does not move the player off it: they
//! chose what to do by pressing a button, and [`Intent`] is that choice while the connection is
//! made. What the player does elsewhere (leaving a lobby, going back) moves them from the screen's
//! own systems.

use bevy::prelude::*;

use crate::net::NetEvent;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    /// The first screen: a nickname, and the ways into a race.
    #[default]
    Login,
    /// Browsing lobbies, creating one.
    Lobbies,
    /// In a lobby: driving, racing or spectating.
    Lobby,
}

/// What the player asked for on the login screen, to carry out once connected.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Intent {
    /// Nothing is waiting: connected, the player stays where they are.
    #[default]
    None,
    Browse,
    QuickPlay,
    Ranked,
    Leaderboard,
}

/// Where a player who leaves a lobby goes back to: the menu they came from.
#[derive(Resource)]
pub struct AfterLobby(pub Screen);

impl Default for AfterLobby {
    fn default() -> Self {
        Self(Screen::Login)
    }
}

pub struct ScreenPlugin;

impl Plugin for ScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Screen>()
            .init_resource::<Intent>()
            .init_resource::<AfterLobby>()
            .add_systems(PreUpdate, follow_server);
    }
}

fn follow_server(mut events: MessageReader<NetEvent>, mut next: ResMut<NextState<Screen>>) {
    for event in events.read() {
        match event {
            NetEvent::JoinedLobby { .. } => next.set(Screen::Lobby),
            NetEvent::LobbyEnded | NetEvent::Rejected(_) | NetEvent::Disconnected(_) => {
                next.set(Screen::Login)
            }
            _ => {}
        }
    }
}
