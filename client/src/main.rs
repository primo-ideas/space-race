mod camera;
#[cfg(not(target_arch = "wasm32"))]
mod capture;
mod controls;
mod geometry;
mod latency;
mod lobby;
mod net;
mod nickname;
mod prediction;
mod preferences;
mod race;
mod scenery;
mod screen;
mod settings;
mod sound;
mod track;
mod ui;
mod world;

use bevy::prelude::*;

/// Behind everything: the track floats in the dark.
const BACKGROUND: Color = Color::BLACK;

/// How often the app updates. Continuous on the web, where Bevy paces a continuous app with the
/// browser's animation frames; the default elsewhere, where an unfocused window can slow down
/// without harm since the connection lives on a thread of its own.
fn web_frame_pacing() -> bevy::winit::WinitSettings {
    if cfg!(target_arch = "wasm32") {
        bevy::winit::WinitSettings::continuous()
    } else {
        bevy::winit::WinitSettings::default()
    }
}

fn main() {
    let settings = settings::read();

    let mut window = Window {
        title: "Space Race".into(),
        // On the web, fill the page. No effect on native.
        fit_canvas_to_parent: true,
        ..default()
    };
    // A window the shape of a phone's screen, to look at the touch interface on a desktop. The
    // scale factor goes with it, so the size asked for is the size the interface is laid out in.
    if let Some((width, height)) = settings.window_size {
        window.resolution =
            bevy::window::WindowResolution::new(width, height).with_scale_factor_override(1.0);
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            // Quiet, and quiet means quiet: with the game running well, a browser console opened
            // on it shows nothing at all. `--log info`, or `?log=info` on the web, brings back
            // what the game has to say.
            //
            // The rendering crates are held at error whatever the level is, because their
            // warnings are about the machine and not about this game. On WebGL2 they are
            // guaranteed and they are four: order-independent transparency, background motion
            // vectors, screen space ambient occlusion and the atmosphere, each announcing it
            // cannot load. The game uses none of them, so the only thing a player could do with
            // those lines is mistake them for something being wrong.
            .set(bevy::log::LogPlugin {
                level: settings.log,
                filter: "wgpu=error,naga=error,bevy_core_pipeline=error,bevy_pbr=error".into(),
                ..default()
            }),
    )
    .insert_resource(ClearColor(BACKGROUND))
    // On the web, a frame on every animation frame, focused or not. Bevy's default slows an
    // unfocused window to a 60 Hz timer, and a frame that overruns that timer makes the next one
    // start at once: on a page that is a loop of back-to-back tasks, and the page's other tasks --
    // the WebSocket's among them -- barely get a turn. The canvas only counts as focused once it
    // has been clicked, so that was every page load: the Hello went out, the page never finished
    // handing it to the network, and the server gave up on the handshake. See docs/web.md.
    .insert_resource(web_frame_pacing())
    .insert_resource(settings.script)
    .insert_resource(ui::touch::Touchscreen(settings.touch))
    .insert_resource(latency::LatencyOverlay(settings.latency))
    .add_plugins((
        preferences::PreferencesPlugin {
            store: settings.identity.clone(),
        },
        net::NetPlugin {
            server: settings.server,
            identity: settings.identity,
        },
        screen::ScreenPlugin,
        lobby::LobbyPlugin,
        track::TrackPlugin,
        race::RacePlugin,
        controls::ControlsPlugin,
        latency::LatencyPlugin,
        camera::CameraPlugin,
        ui::UiPlugin,
        sound::SoundPlugin {
            volume: settings.volume,
        },
    ));

    #[cfg(not(target_arch = "wasm32"))]
    if let Some((path, delay)) = settings.capture {
        app.add_plugins(capture::CapturePlugin { path, delay });
    }

    app.run();
}
