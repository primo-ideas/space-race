//! Non-interactive test mode: takes a screenshot once the screen the command line leads to is
//! shown and fully rendered, then exits.
//!
//! Without `--nickname` that screen is the login screen; with it, the lobby list, or the creation
//! dialog with `--create-dialog`; with `--lobby`, the race. A connection that fails ends on the
//! login screen, which says why: the screenshot shows it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::lobby::{CurrentLobby, LobbyList};
use crate::net::Connection;
use crate::race::RaceView;
use crate::screen::Screen;
use crate::settings::Script;

/// Consecutive frames without any pipeline left to compile before the screenshot. Waiting for
/// pipelines rather than a fixed frame count matters on slow machines, where shader compilation
/// can take seconds and the first frames are not drawn at all.
const SETTLE_FRAMES: u32 = 10;

/// Gives up if the scene never finishes rendering, so a test run cannot hang. Counted on top of
/// the requested delay.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct CapturePlugin {
    pub path: PathBuf,
    /// Time to wait once the screen is shown, so the cars have moved.
    pub delay: Duration,
}

#[derive(Resource)]
struct CaptureSettings {
    path: PathBuf,
    delay: Duration,
    target: Target,
}

/// The screen the scripted run ends on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Login,
    Lobbies,
    Lobby,
}

/// Written by the render world, read by the main world: whether every render pipeline is ready.
#[derive(Resource, Clone, Default)]
struct PipelinesReady(Arc<AtomicBool>);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        let ready = PipelinesReady::default();
        let script = app.world().resource::<Script>();
        let target = if script.lobby.is_some() {
            Target::Lobby
        } else if script.nickname.is_some() {
            Target::Lobbies
        } else {
            Target::Login
        };
        app.insert_resource(CaptureSettings {
            path: self.path.clone(),
            delay: self.delay,
            target,
        })
        .insert_resource(ready.clone())
        .add_systems(Update, capture_when_rendered);

        app.sub_app_mut(RenderApp)
            .insert_resource(ready)
            .add_systems(Render, check_pipelines.in_set(RenderSystems::Cleanup));
    }
}

fn check_pipelines(pipelines: Res<PipelineCache>, ready: Res<PipelinesReady>) {
    let has_pipelines = pipelines.pipelines().next().is_some();
    let none_waiting = pipelines.waiting_pipelines().next().is_none();
    ready
        .0
        .store(has_pipelines && none_waiting, Ordering::Relaxed);
}

fn capture_when_rendered(
    mut commands: Commands,
    settings: Res<CaptureSettings>,
    pipelines: Res<PipelinesReady>,
    view: Res<RaceView>,
    connection: Res<Connection>,
    screen: Res<State<Screen>>,
    list: Res<LobbyList>,
    current: Res<CurrentLobby>,
    time: Res<Time<Real>>,
    mut displayed_since: Local<Option<Duration>>,
    mut settled_frames: Local<u32>,
    mut requested: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed() > CAPTURE_TIMEOUT + settings.delay {
        error!("scene not rendered after {CAPTURE_TIMEOUT:?}, no screenshot taken");
        exit.write(AppExit::error());
        return;
    }
    if *requested {
        return;
    }

    // The screen is shown, or will never be: then the screenshot shows why.
    let failed = matches!(
        *connection,
        Connection::Rejected(_) | Connection::Disconnected { .. }
    );
    let settled = match settings.target {
        Target::Login => *screen.get() == Screen::Login,
        Target::Lobbies => *screen.get() == Screen::Lobbies && list.loaded,
        Target::Lobby => {
            *screen.get() == Screen::Lobby
                && current
                    .0
                    .as_ref()
                    .is_some_and(|membership| membership.state.is_some())
                && view.followed.is_some()
        }
    };
    if !(settled || failed && *screen.get() == Screen::Login) {
        return;
    }
    let since = *displayed_since.get_or_insert(time.elapsed());
    if time.elapsed() - since < settings.delay {
        return;
    }

    *settled_frames = if pipelines.0.load(Ordering::Relaxed) {
        *settled_frames + 1
    } else {
        0
    };
    if *settled_frames < SETTLE_FRAMES {
        return;
    }

    *requested = true;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(settings.path.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}
