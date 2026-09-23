//! What the client is asked to do: from the command line on native, from the page address on the
//! web (`?server=ws://…&nickname=…&lobby=…&track=…&identity=…&volume=…&autopilot`,
//! `&autopilot_drift`, `&touch` or `&latency`).
//!
//! Without a nickname the game opens on the login screen. The other options skip screens, so the
//! game can reach a lobby and drive with nobody at the keyboard.

use space_race_protocol::{DEFAULT_PORT, WS_PATH};
use space_race_sim::autopilot::Style;

use crate::net::identity::IdentityStore;
use crate::preferences::{DEFAULT_LAPS, DEFAULT_MIN_PLAYERS};

pub struct Settings {
    /// Server WebSocket URL.
    pub server: String,
    /// How much the game says for itself. Quiet by default: with nothing wrong, a browser
    /// console open on the game stays empty.
    pub log: bevy::log::Level,
    pub identity: IdentityStore,
    /// How loud the game is, from 0 (silent) to 1.
    pub volume: f32,
    pub script: Script,
    /// Whether to draw the interface for thumbs on a screen rather than for a pointer: the
    /// on-screen controls, and menus laid out for a phone held in landscape (docs/ui.md).
    pub touch: bool,
    /// Whether the latency figures are shown over the race from the start (docs/latency.md).
    pub latency: bool,
    /// Screenshot path, and how long to wait once the screen is ready. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub capture: Option<(std::path::PathBuf, std::time::Duration)>,
    /// The window size asked for, in logical pixels. Always `None` on the web, where the page
    /// decides how big the canvas is.
    pub window_size: Option<(u32, u32)>,
}

/// Screens the client goes through on its own, for unattended runs.
#[derive(bevy::prelude::Resource, Clone, Default)]
pub struct Script {
    /// Connect at once under this nickname, skipping the login screen.
    pub nickname: Option<String>,
    /// Once connected, enter the lobby of this name, creating it if there is none.
    pub lobby: Option<ScriptedLobby>,
    /// Once connected, open the lobby creation dialog.
    pub create_dialog: bool,
    /// Let the autopilot drive, in this style.
    pub autopilot: Option<Style>,
}

#[derive(Clone)]
pub struct ScriptedLobby {
    pub name: String,
    /// Used if the lobby has to be created. No track picks the esplanade.
    pub track: Option<String>,
    pub laps: u8,
    pub min_players: u8,
}

const DEFAULT_VOLUME: f32 = 0.7;

fn default_server() -> String {
    format!("ws://127.0.0.1:{DEFAULT_PORT}{WS_PATH}")
}

/// Where the web client connects when the page address says nothing: the same host the page came
/// from, over TLS, at `<the page's directory>/ws`. The deployed game is served at
/// `https://…/space-race/` and reaches its server at `wss://…/space-race/ws`, which the reverse
/// proxy forwards (see `docs/deploy.md`), so a deployed page needs no `?server=` at all.
///
/// A page served from a development server on the local machine is the exception: there is no
/// proxy there, so it falls back to the local server's own address.
#[cfg(target_arch = "wasm32")]
fn server_of_the_page() -> String {
    let Some(location) = web_sys::window().map(|window| window.location()) else {
        return default_server();
    };
    let (Ok(host), Ok(path)) = (location.host(), location.pathname()) else {
        return default_server();
    };
    if host.starts_with("127.0.0.1") || host.starts_with("localhost") || host.is_empty() {
        return default_server();
    }
    // The page's directory: everything up to its last slash, so `/space-race/index.html` and
    // `/space-race/` both give `/space-race`.
    let directory = path.rsplit_once('/').map_or("", |(before, _)| before);
    format!("wss://{host}{directory}{WS_PATH}")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read() -> Settings {
    use std::path::PathBuf;
    use std::time::Duration;

    use clap::Parser;

    #[derive(Parser)]
    struct Args {
        /// Server WebSocket URL.
        #[arg(long, default_value_t = default_server())]
        server: String,

        /// Connect at once under this nickname, skipping the login screen.
        #[arg(long)]
        nickname: Option<String>,

        /// With --nickname, enter the lobby of this name, creating it if there is none.
        #[arg(long, requires = "nickname")]
        lobby: Option<String>,

        /// Track of the lobby created by --lobby, by its key (the file name without .ron).
        #[arg(long, requires = "lobby")]
        track: Option<String>,

        /// Laps of the lobby created by --lobby.
        #[arg(long, default_value_t = DEFAULT_LAPS, requires = "lobby")]
        laps: u8,

        /// Players needed to start a race in the lobby created by --lobby.
        #[arg(long, default_value_t = DEFAULT_MIN_PLAYERS, requires = "lobby")]
        min_players: u8,

        /// With --nickname, open the lobby creation dialog once connected.
        #[arg(long, requires = "nickname", conflicts_with = "lobby")]
        create_dialog: bool,

        /// Player identity directory. Useful to run several clients on the same machine.
        #[arg(long)]
        identity: Option<PathBuf>,

        /// Take a screenshot to this PNG file once the screen the other options lead to is shown,
        /// then exit.
        #[arg(long)]
        capture: Option<PathBuf>,

        /// With --capture, seconds to wait once that screen is shown, so the cars have moved.
        #[arg(long, default_value_t = 0.0, requires = "capture")]
        capture_delay: f32,

        /// Let the autopilot drive, for unattended checks.
        #[arg(long)]
        autopilot: bool,

        /// Let the autopilot drive and drift through the turns.
        #[arg(long, conflicts_with = "autopilot")]
        autopilot_drift: bool,

        /// How loud the game is, from 0 (silent) to 1.
        #[arg(long, default_value_t = DEFAULT_VOLUME)]
        volume: f32,

        /// How much the game says: error, warn, info, debug or trace.
        #[arg(long, default_value = DEFAULT_LOG)]
        log: String,

        /// Draw the interface for a touchscreen: on-screen controls and menus laid out for a
        /// phone. The web client reads this from the browser; here it is for looking at that
        /// interface without one.
        #[arg(long)]
        touch: bool,

        /// Show the latency figures over the race from the start. F3 toggles them anyway.
        #[arg(long)]
        latency: bool,

        /// Window size, as WIDTHxHEIGHT logical pixels, such as 844x390: a window the shape of a
        /// phone held in landscape, to go with --touch.
        #[arg(long, value_parser = window_size)]
        window_size: Option<(u32, u32)>,
    }

    let args = Args::parse();
    let identity = match args.identity {
        Some(dir) => IdentityStore::in_dir(dir),
        None => IdentityStore::default_dir().expect("no user data directory, use --identity"),
    };
    let capture_delay = Duration::from_secs_f32(args.capture_delay.max(0.0));
    Settings {
        server: args.server,
        identity,
        log: log_level(Some(args.log)),
        volume: args.volume.clamp(0.0, 1.0),
        script: Script {
            nickname: args.nickname,
            lobby: args.lobby.map(|name| ScriptedLobby {
                name,
                track: args.track,
                laps: args.laps,
                min_players: args.min_players,
            }),
            create_dialog: args.create_dialog,
            autopilot: autopilot_style(args.autopilot, args.autopilot_drift),
        },
        touch: args.touch,
        latency: args.latency,
        capture: args.capture.map(|path| (path, capture_delay)),
        window_size: args.window_size,
    }
}

/// A window size written `WIDTHxHEIGHT`.
#[cfg(not(target_arch = "wasm32"))]
fn window_size(value: &str) -> Result<(u32, u32), String> {
    let complaint = || format!("expected WIDTHxHEIGHT, such as 844x390, not {value}");
    let (width, height) = value.split_once(['x', 'X']).ok_or_else(complaint)?;
    match (width.trim().parse(), height.trim().parse()) {
        (Ok(width), Ok(height)) => Ok((width, height)),
        _ => Err(complaint()),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn read() -> Settings {
    let params = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok());
    let get = |name: &str| params.as_ref().and_then(|params| params.get(name));
    let has = |name: &str| params.as_ref().is_some_and(|params| params.has(name));

    /// What a parameter says, as a number, or `default` when it is missing or unreadable. A
    /// closure would do, but the parameters are read as several types, and only a function can.
    fn number<T: std::str::FromStr>(value: Option<String>, default: T) -> T {
        value
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    Settings {
        server: get("server").unwrap_or_else(server_of_the_page),
        identity: IdentityStore::named(get("identity").as_deref()),
        log: log_level(get("log")),
        volume: number(get("volume"), DEFAULT_VOLUME).clamp(0.0, 1.0),
        script: Script {
            nickname: get("nickname"),
            lobby: get("lobby").map(|name| ScriptedLobby {
                name,
                track: get("track"),
                laps: number(get("laps"), DEFAULT_LAPS),
                min_players: number(get("min_players"), DEFAULT_MIN_PLAYERS),
            }),
            create_dialog: has("create_dialog"),
            autopilot: autopilot_style(has("autopilot"), has("autopilot_drift")),
        },
        touch: has("touch") || touchscreen(),
        latency: has("latency"),
        window_size: None,
    }
}

/// Whether the page is being touched rather than pointed at. A coarse pointer is what a finger
/// is: it is true of phones and tablets, and false of a laptop that happens to have a touch
/// screen as well as a mouse. `?touch` forces it on, to see that interface in any browser.
#[cfg(target_arch = "wasm32")]
fn touchscreen() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|query| query.matches())
}

/// Quiet unless asked otherwise. A game that prints nothing while it runs well is a console the
/// player can actually read when something breaks, so nothing below a warning is shown by
/// default; `--log info`, or `?log=info` on the web, brings back what the game has to say.
const DEFAULT_LOG: &str = "warn";

/// The level asked for. An unreadable name falls back to the quiet default rather than refusing
/// to start, as everything else read from the page address does.
fn log_level(name: Option<String>) -> bevy::log::Level {
    use bevy::log::Level;
    let name = name.unwrap_or_else(|| DEFAULT_LOG.to_owned());
    match name.trim().to_ascii_lowercase().as_str() {
        "error" => Level::ERROR,
        "info" => Level::INFO,
        "debug" => Level::DEBUG,
        "trace" => Level::TRACE,
        _ => Level::WARN,
    }
}

/// The autopilot asked for, if any: drifting wins over gripping.
fn autopilot_style(grip: bool, drift: bool) -> Option<Style> {
    if drift {
        Some(Style::Drift)
    } else if grip {
        Some(Style::Grip)
    } else {
        None
    }
}
