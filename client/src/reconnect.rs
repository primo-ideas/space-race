//! A connection lost is a connection tried again. A phone loses its connection all the time: it
//! moves from Wi-Fi to its mobile network, goes through a tunnel, puts the browser to sleep while
//! the player answers a message, and a server being updated restarts under everyone. Sending the
//! player back to the login screen to press a button and find their lobby again made every one
//! of those the end of their game.
//!
//! So once the server has welcomed the player, losing it starts trying again on its own: at once,
//! then less and less often, as long as the page is open. The login screen says so while it
//! waits. Once back, the player goes back into the lobby they were in, if it is still there --
//! racing again at the next start, as anyone joining does. Being turned away stops it: a server
//! running another version of the game will not change its mind, and a connection replaced by the
//! same player in another window must not take its place back, or the two would push each other
//! out forever.
//!
//! The server lets a player's new connection replace one it still holds, which is what lets a
//! player come back before the server has noticed they had gone (see docs/lobbies.md).

use bevy::prelude::*;
use space_race_protocol::{ClientMessage, LobbyId};

use crate::lobby::{CurrentLobby, LobbyList, LobbyUpdate, Notice};
use crate::net::{Connection, Network};

/// Seconds between the connection being lost and the first try, then between each try and the
/// next, which doubles up to the last.
const DELAYS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];

pub struct ReconnectPlugin;

impl Plugin for ReconnectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Reconnect>()
            .add_systems(PreUpdate, notice_loss.before(LobbyUpdate))
            .add_systems(Update, (try_again, go_back_to_the_lobby));
    }
}

/// Who the player was online as and where, and while the connection is being tried again, how
/// that is going.
#[derive(Resource, Default)]
pub struct Reconnect {
    /// The nickname the server last welcomed.
    online_as: Option<String>,
    attempt: Option<Attempt>,
}

#[derive(Debug, Clone, PartialEq)]
struct Attempt {
    nickname: String,
    /// The lobby to go back into once connected.
    lobby: Option<LobbyId>,
    /// How many tries have been made.
    tries: usize,
    /// When the next try is made, in real seconds since startup.
    next_at: f64,
    /// The server has welcomed the player back: what is left is the lobby.
    welcomed: bool,
}

impl Reconnect {
    /// What the login screen says while the connection is tried again, if it is: that it is lost,
    /// and when the next try comes, or that it is being made.
    pub fn status(&self, connection: &Connection, now: f64) -> Option<String> {
        let attempt = self.attempt.as_ref().filter(|attempt| !attempt.welcomed)?;
        Some(match connection {
            Connection::Connecting { .. } => "Connection lost. Reconnecting…".to_owned(),
            _ => {
                let seconds = (attempt.next_at - now).ceil().max(1.0);
                format!("Connection lost. Trying again in {seconds:.0} s…")
            }
        })
    }
}

/// How long to wait before try number `tries`, counted from 0.
fn delay(tries: usize) -> f64 {
    DELAYS[tries.min(DELAYS.len() - 1)]
}

/// Starts trying again when a connection the server had welcomed is lost, remembering the lobby
/// before it is forgotten.
fn notice_loss(
    connection: Res<Connection>,
    current: Res<CurrentLobby>,
    time: Res<Time<Real>>,
    mut reconnect: ResMut<Reconnect>,
) {
    if !connection.is_changed() {
        return;
    }
    let now = time.elapsed_secs_f64();
    match &*connection {
        Connection::Online { nickname } => reconnect.online_as = Some(nickname.clone()),
        Connection::Disconnected {
            was_online: true, ..
        } => {
            let Some(nickname) = reconnect.online_as.clone() else {
                return;
            };
            let lobby = current.0.as_ref().map(|membership| membership.lobby);
            reconnect.attempt = Some(Attempt {
                nickname,
                lobby,
                tries: 0,
                next_at: now + delay(0),
                welcomed: false,
            });
        }
        // A try that could not reach the server: the next one waits a little longer.
        Connection::Disconnected {
            was_online: false, ..
        } => {
            if let Some(attempt) = &mut reconnect.attempt {
                attempt.next_at = now + delay(attempt.tries);
            }
        }
        // Turned away, or disconnected on purpose: nothing to come back to.
        Connection::Rejected(_) | Connection::Offline => {
            reconnect.attempt = None;
            if matches!(*connection, Connection::Offline) {
                reconnect.online_as = None;
            }
        }
        Connection::Connecting { .. } => {}
    }
}

fn try_again(
    mut reconnect: ResMut<Reconnect>,
    mut network: ResMut<Network>,
    mut connection: ResMut<Connection>,
    time: Res<Time<Real>>,
) {
    let now = time.elapsed_secs_f64();
    let Some(attempt) = &mut reconnect.attempt else {
        return;
    };
    // The player may have pressed PLAY themselves meanwhile: that is a try too.
    if attempt.welcomed
        || now < attempt.next_at
        || !matches!(*connection, Connection::Disconnected { .. })
    {
        return;
    }
    attempt.tries += 1;
    attempt.next_at = now + delay(attempt.tries);
    let nickname = attempt.nickname.clone();
    info!(tries = attempt.tries, "trying to reconnect");
    network.connect(nickname, &mut connection);
}

/// Once welcomed back and listed, goes back into the lobby the player was in.
fn go_back_to_the_lobby(
    mut reconnect: ResMut<Reconnect>,
    connection: Res<Connection>,
    list: Res<LobbyList>,
    network: Res<Network>,
    mut notice: ResMut<Notice>,
    time: Res<Time<Real>>,
) {
    let Some(attempt) = &mut reconnect.attempt else {
        return;
    };
    if !matches!(*connection, Connection::Online { .. }) {
        return;
    }
    attempt.welcomed = true;
    if !list.loaded {
        return;
    }
    match rejoin(attempt.lobby, &list) {
        Rejoin::Join(lobby) => network.send(ClientMessage::JoinLobby(lobby)),
        Rejoin::Gone => notice.show(
            "Back online. Your lobby ended while you were away.",
            time.elapsed_secs_f64(),
        ),
        Rejoin::Nothing => {}
    }
    reconnect.attempt = None;
}

/// What becomes of the lobby the player was in, once they are back.
#[derive(Debug, PartialEq, Eq)]
enum Rejoin {
    Join(LobbyId),
    /// It ended while they were away.
    Gone,
    /// They were browsing the list.
    Nothing,
}

fn rejoin(lobby: Option<LobbyId>, list: &LobbyList) -> Rejoin {
    match lobby {
        Some(lobby) if list.lobbies.iter().any(|summary| summary.id == lobby) => {
            Rejoin::Join(lobby)
        }
        Some(_) => Rejoin::Gone,
        None => Rejoin::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use space_race_protocol::{CarId, LobbySettings, LobbyStatus, LobbySummary, RejectReason};

    use super::*;
    use crate::lobby::Membership;

    #[test]
    fn tries_come_quickly_then_less_often() {
        let delays: Vec<f64> = (0..8).map(delay).collect();
        assert_eq!(delays, [0.5, 1.0, 2.0, 4.0, 8.0, 8.0, 8.0, 8.0]);
    }

    fn settings() -> LobbySettings {
        LobbySettings {
            name: "Ada's lobby".into(),
            track: "skyway".into(),
            laps: 3,
            min_players: 2,
            max_players: 8,
        }
    }

    /// An app that only notices losses, so no test ever opens a connection.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Connection>()
            .init_resource::<CurrentLobby>()
            .init_resource::<Reconnect>()
            .add_systems(Update, notice_loss);
        app
    }

    fn set(app: &mut App, connection: Connection) {
        *app.world_mut().resource_mut::<Connection>() = connection;
        app.update();
    }

    fn attempt(app: &App) -> Option<Attempt> {
        app.world().resource::<Reconnect>().attempt.clone()
    }

    fn online() -> Connection {
        Connection::Online {
            nickname: "Ada".into(),
        }
    }

    fn lost(was_online: bool) -> Connection {
        Connection::Disconnected {
            reason: "reset".into(),
            was_online,
        }
    }

    /// Lost from a lobby: tried again as the same player, for the same lobby, and said so.
    #[test]
    fn a_lost_connection_is_tried_again_for_the_same_lobby() {
        let mut app = app();
        set(&mut app, online());
        app.world_mut().resource_mut::<CurrentLobby>().0 = Some(Membership {
            lobby: LobbyId(7),
            car: CarId(1),
            settings: settings(),
            state: None,
        });
        set(&mut app, lost(true));
        let first = attempt(&app).expect("a reconnection");
        assert_eq!(first.nickname, "Ada");
        assert_eq!(first.lobby, Some(LobbyId(7)));
        assert_eq!(first.tries, 0);
        let status = app
            .world()
            .resource::<Reconnect>()
            .status(&lost(true), first.next_at - 0.2);
        assert_eq!(
            status.as_deref(),
            Some("Connection lost. Trying again in 1 s…")
        );
        let connecting = Connection::Connecting {
            nickname: "Ada".into(),
        };
        let status = app.world().resource::<Reconnect>().status(&connecting, 0.0);
        assert_eq!(status.as_deref(), Some("Connection lost. Reconnecting…"));

        // A try that cannot reach the server leaves the attempt going.
        set(
            &mut app,
            Connection::Connecting {
                nickname: "Ada".into(),
            },
        );
        set(&mut app, lost(false));
        assert!(attempt(&app).is_some());
    }

    /// Turned away -- replaced by another window, or a server on another version -- or leaving on
    /// purpose: nothing is tried again.
    #[test]
    fn nothing_is_tried_again_after_being_turned_away() {
        let mut app = app();
        set(&mut app, online());
        set(&mut app, lost(true));
        assert!(attempt(&app).is_some());
        set(
            &mut app,
            Connection::Rejected(RejectReason::AlreadyConnected),
        );
        assert!(attempt(&app).is_none());

        set(&mut app, online());
        set(&mut app, Connection::Offline);
        set(&mut app, lost(true));
        assert!(attempt(&app).is_none(), "nobody is online to come back as");
    }

    /// A connection that never got in -- a wrong address, a server down -- is the login screen's
    /// to report, not to retry.
    #[test]
    fn a_connection_never_welcomed_is_not_tried_again() {
        let mut app = app();
        set(&mut app, lost(false));
        assert!(attempt(&app).is_none());
    }

    #[test]
    fn the_lobby_is_joined_again_only_if_it_is_still_there() {
        let summary = |id| LobbySummary {
            id: LobbyId(id),
            settings: settings(),
            players: Vec::new(),
            status: LobbyStatus::Waiting,
        };
        let list = LobbyList {
            lobbies: vec![summary(3), summary(7)],
            received_at: 0.0,
            loaded: true,
        };
        assert_eq!(rejoin(Some(LobbyId(7)), &list), Rejoin::Join(LobbyId(7)));
        assert_eq!(rejoin(Some(LobbyId(9)), &list), Rejoin::Gone);
        assert_eq!(rejoin(None, &list), Rejoin::Nothing);
    }
}
