//! The screens of the game, and what the server's answers move the player to.
//!
//! ```text
//! Login ──connected──▶ Lobbies ──entered a lobby──▶ Lobby
//!   ▲                    │  ▲                          │
//!   └─change nickname────┘  └────────leave─────────────┘
//! ```
//!
//! Any disconnection or rejection goes back to the login screen, which says why. What the player
//! does (leaving a lobby, changing nickname) moves them from the screen's own systems.

use bevy::prelude::*;

use crate::net::NetEvent;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    /// Choosing a nickname and connecting.
    #[default]
    Login,
    /// Browsing lobbies, creating one.
    Lobbies,
    /// In a lobby: driving, racing or spectating.
    Lobby,
}

pub struct ScreenPlugin;

impl Plugin for ScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Screen>()
            .add_systems(PreUpdate, follow_server);
    }
}

fn follow_server(mut events: MessageReader<NetEvent>, mut next: ResMut<NextState<Screen>>) {
    for event in events.read() {
        match event {
            NetEvent::Welcome { .. } => next.set(Screen::Lobbies),
            NetEvent::JoinedLobby { .. } => next.set(Screen::Lobby),
            NetEvent::Rejected(_) | NetEvent::Disconnected(_) => next.set(Screen::Login),
            _ => {}
        }
    }
}
