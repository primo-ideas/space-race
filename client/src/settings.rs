//! What the client is asked to do: from the command line on native, from the page address on the
//! web (`?server=…&guest&lobby=…&track=…&identity=…&volume=…&autopilot`, or
//! `&account=…&password=…&create` in place of `&guest`, `&autopilot_drift`, `&quick_play`,
//! `&ranked`, `&settings`, `&garage`, `&touch`, `&safe_area=…` or `&latency`).
//!
//! Without `--guest` or `--account` the game opens on the first screen. The other options skip
//! screens, so the game can reach a lobby and drive with nobody at the keyboard.

use star_racer_protocol::{DEFAULT_PORT, WS_PATH};
use star_racer_sim::autopilot::Style;

use crate::net::identity::IdentityStore;
use crate::preferences::{DEFAULT_LAPS, DEFAULT_MIN_PLAYERS};
use crate::ui::safe_area::SafeArea;

pub struct Settings {
    /// The server asked for by `--server` or `?server=`, as a WebSocket URL. `None` leaves it to
    /// the host the player set in the settings, or to the default (see [`server_to_use`]).
    pub server: Option<String>,
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
    /// Insets standing for a phone's notch, rounded corners and home bar, in logical pixels, to
    /// look at the interface laid out around them. `None` reads them from the browser, which is
    /// where they come from on a phone (docs/ui.md).
    pub safe_area: Option<SafeArea>,
}

/// Screens the client goes through on its own, for unattended runs.
#[derive(bevy::prelude::Resource, Clone, Default)]
pub struct Script {
    /// Connect at once, skipping the first screen.
    pub login: Option<ScriptedLogin>,
    /// Once connected, enter the lobby of this name, creating it if there is none.
    pub lobby: Option<ScriptedLobby>,
    /// Once connected, open the lobby creation dialog.
    pub create_dialog: bool,
    /// Once connected, go straight into a race, as the QUICK PLAY button does.
    pub quick_play: bool,
    /// Once connected, enter the ranked queue, as the QUEUE FOR RANKED button does.
    pub ranked: bool,
    /// Open the settings over the first screen, to look at them.
    pub settings: bool,
    /// Open the garage, to look at it.
    pub garage: bool,
    /// Let the autopilot drive, in this style.
    pub autopilot: Option<Style>,
}

/// How an unattended run comes in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptedLogin {
    /// As a guest, under a generated name.
    Guest,
    /// To an account, created first with `create`.
    Account {
        name: String,
        password: String,
        create: bool,
    },
}

impl ScriptedLogin {
    fn of(guest: bool, account: Option<String>, password: Option<String>, create: bool) -> Option<Self> {
        match account {
            Some(name) => Some(Self::Account {
                name,
                password: password.unwrap_or_default(),
                create,
            }),
            None => guest.then_some(Self::Guest),
        }
    }
}

#[derive(Clone)]
pub struct ScriptedLobby {
    pub name: String,
    /// Used if the lobby has to be created. No track picks the skyway.
    pub track: Option<String>,
    pub laps: u8,
    pub min_players: u8,
}

const DEFAULT_VOLUME: f32 = 0.7;

/// Where the game plays when nobody says otherwise: the deployed server (the user's choice,
/// 2026-11-11). A server on the same machine is `--server localhost`.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_HOST: &str = "star-racer.xyz";

/// The host the settings screen offers, and the game connects to, until the player names
/// another.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_host() -> String {
    DEFAULT_HOST.to_owned()
}

/// The host the settings screen offers, and the game connects to, until the player names
/// another: on the web, the one the page came from, with the page's directory, so the game
/// served at `https://…/star-racer/` reaches its server at `wss://…/star-racer/ws`, which the
/// reverse proxy forwards (see `docs/deploy.md`), and a deployed page needs no `?server=` at all.
///
/// A page served from a development server on the local machine is the exception: there is no
/// proxy there, so it names the local machine, whose server listens on its own port.
#[cfg(target_arch = "wasm32")]
pub fn default_host() -> String {
    const LOCAL: &str = "127.0.0.1";
    let Some(location) = web_sys::window().map(|window| window.location()) else {
        return LOCAL.to_owned();
    };
    let (Ok(host), Ok(path)) = (location.host(), location.pathname()) else {
        return LOCAL.to_owned();
    };
    if host.starts_with("127.0.0.1") || host.starts_with("localhost") || host.is_empty() {
        return LOCAL.to_owned();
    }
    // The page's directory: everything up to its last slash, so `/star-racer/index.html` and
    // `/star-racer/` both give `/star-racer`.
    let directory = path.rsplit_once('/').map_or("", |(before, _)| before);
    format!("{host}{directory}")
}

/// The WebSocket address of the server a player names, or `None` if what they typed cannot be
/// one. They may type as little as a host:
///
/// - `star-racer.xyz` is `wss://star-racer.xyz/ws`: a host out on the network is reached over
///   TLS, at the path every deployment serves.
/// - `primo-ideas.xyz/star-racer` is `wss://primo-ideas.xyz/star-racer/ws`: a path is the
///   directory the game is served from.
/// - `localhost`, `127.0.0.1` or an address of the local network is `ws://…:8080/ws`: a server
///   run by hand has no certificate and listens on its own port.
/// - `https://…` and `http://…`, as pasted from a browser, say which of the two it is.
/// - `ws://…` and `wss://…` are taken exactly as written.
pub fn server_url(typed: &str) -> Option<String> {
    let typed = typed.trim();
    if typed.is_empty() || typed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    for scheme in ["ws://", "wss://"] {
        if let Some(rest) = typed.strip_prefix(scheme) {
            return valid_authority(rest.split('/').next()?).then(|| typed.to_owned());
        }
    }
    let (secure, rest) = if let Some(rest) = typed.strip_prefix("https://") {
        (Some(true), rest)
    } else if let Some(rest) = typed.strip_prefix("http://") {
        (Some(false), rest)
    } else {
        (None, typed)
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !valid_authority(authority) {
        return None;
    }
    let local = is_local(authority);
    let secure = secure.unwrap_or(!local);
    let path = path.trim_end_matches('/');
    let path = match path {
        "" => WS_PATH.to_owned(),
        path if format!("/{path}").ends_with(WS_PATH) => format!("/{path}"),
        path => format!("/{path}{WS_PATH}"),
    };
    let scheme = if secure { "wss" } else { "ws" };
    // A local server has no proxy in front of it: it is on the game's own port.
    let port = if local && !has_port(authority) {
        format!(":{DEFAULT_PORT}")
    } else {
        String::new()
    };
    Some(format!("{scheme}://{authority}{port}{path}"))
}

/// Whether this can be a host, with or without a port: something, made of what hosts are made of.
fn valid_authority(authority: &str) -> bool {
    !authority.is_empty()
        && authority
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']' | '_'))
        && authority.chars().any(|c| c.is_ascii_alphanumeric())
}

fn has_port(authority: &str) -> bool {
    // After the bracket of an IPv6 address, if there is one.
    let after = authority.rsplit_once(']').map_or(authority, |(_, after)| after);
    after.contains(':')
}

/// Whether the host is this machine or one beside it: reached in plain text, on the game's port.
fn is_local(authority: &str) -> bool {
    let host = if authority.starts_with('[') {
        authority.split(']').next().unwrap_or(authority)
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "[::1"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
}

/// The address the game connects to: the one asked for on the command line or in the page
/// address, then the host the player set in the settings, then the default.
pub fn server_to_use(asked: Option<String>, chosen: Option<&str>) -> String {
    asked
        .or_else(|| chosen.and_then(server_url))
        .or_else(|| server_url(&default_host()))
        .unwrap_or_else(|| format!("ws://127.0.0.1:{DEFAULT_PORT}{WS_PATH}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read() -> Settings {
    use std::path::PathBuf;
    use std::time::Duration;

    use clap::Parser;

    #[derive(Parser)]
    struct Args {
        /// The server: a host such as star-racer.xyz, `localhost` for one run on this machine, or
        /// a whole ws:// or wss:// address. Without it, the host set in the settings, or
        /// star-racer.xyz.
        #[arg(long, value_parser = server)]
        server: Option<String>,

        /// Connect at once as a guest, under a generated name, skipping the first screen.
        #[arg(long, group = "who")]
        guest: bool,

        /// Connect at once to this account, skipping the first screen. Goes with --password.
        #[arg(long, group = "who", requires = "password")]
        account: Option<String>,

        /// The password of --account.
        #[arg(long, requires = "account")]
        password: Option<String>,

        /// Create --account first, with --password.
        #[arg(long, requires = "account")]
        create: bool,

        /// With --guest or --account, enter the lobby of this name, creating it if there is none.
        #[arg(long, requires = "who")]
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

        /// With --guest or --account, open the lobby creation dialog once connected.
        #[arg(long, requires = "who", conflicts_with = "lobby")]
        create_dialog: bool,

        /// With --guest or --account, go straight into a race, as the QUICK PLAY button does.
        #[arg(long, requires = "who", conflicts_with_all = ["lobby", "create_dialog"])]
        quick_play: bool,

        /// With --account, enter the ranked queue, as the QUEUE FOR RANKED button does.
        #[arg(long, requires = "account", conflicts_with_all = ["lobby", "create_dialog", "quick_play"])]
        ranked: bool,

        /// Open the settings over the first screen.
        #[arg(long, conflicts_with = "who")]
        settings: bool,

        /// Open the garage, where the pod is chosen and painted.
        #[arg(long, conflicts_with_all = ["who", "settings"])]
        garage: bool,

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

        /// Keep the interface this far in from the window's edges, as TOP,RIGHT,BOTTOM,LEFT
        /// logical pixels, such as 0,47,21,47: an iPhone's notch, rounded corners and home bar,
        /// held in landscape. Goes with --touch and --window-size.
        #[arg(long)]
        safe_area: Option<SafeArea>,
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
            login: ScriptedLogin::of(args.guest, args.account, args.password, args.create),
            lobby: args.lobby.map(|name| ScriptedLobby {
                name,
                track: args.track,
                laps: args.laps,
                min_players: args.min_players,
            }),
            create_dialog: args.create_dialog,
            quick_play: args.quick_play,
            ranked: args.ranked,
            settings: args.settings,
            garage: args.garage,
            autopilot: autopilot_style(args.autopilot, args.autopilot_drift),
        },
        touch: args.touch,
        latency: args.latency,
        capture: args.capture.map(|path| (path, capture_delay)),
        window_size: args.window_size,
        safe_area: args.safe_area,
    }
}

/// A server named on the command line, as the address to connect to.
#[cfg(not(target_arch = "wasm32"))]
fn server(value: &str) -> Result<String, String> {
    server_url(value).ok_or_else(|| {
        format!("expected a host such as star-racer.xyz or localhost, or a ws:// address, not {value}")
    })
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
        server: get("server").and_then(|server| server_url(&server)),
        identity: IdentityStore::named(get("identity").as_deref()),
        log: log_level(get("log")),
        volume: number(get("volume"), DEFAULT_VOLUME).clamp(0.0, 1.0),
        script: Script {
            login: ScriptedLogin::of(has("guest"), get("account"), get("password"), has("create")),
            lobby: get("lobby").map(|name| ScriptedLobby {
                name,
                track: get("track"),
                laps: number(get("laps"), DEFAULT_LAPS),
                min_players: number(get("min_players"), DEFAULT_MIN_PLAYERS),
            }),
            create_dialog: has("create_dialog"),
            quick_play: has("quick_play"),
            ranked: has("ranked"),
            settings: has("settings"),
            garage: has("garage"),
            autopilot: autopilot_style(has("autopilot"), has("autopilot_drift")),
        },
        touch: has("touch") || touchscreen(),
        latency: has("latency"),
        window_size: None,
        safe_area: get("safe_area").and_then(|value| value.parse().ok()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_out_on_the_network_is_reached_over_tls() {
        assert_eq!(
            server_url("star-racer.xyz").as_deref(),
            Some("wss://star-racer.xyz/ws")
        );
        assert_eq!(
            server_url("  star-racer.xyz/ ").as_deref(),
            Some("wss://star-racer.xyz/ws")
        );
        // A path is the directory the game is served from.
        assert_eq!(
            server_url("primo-ideas.xyz/star-racer").as_deref(),
            Some("wss://primo-ideas.xyz/star-racer/ws")
        );
        assert_eq!(
            server_url("primo-ideas.xyz/star-racer/ws").as_deref(),
            Some("wss://primo-ideas.xyz/star-racer/ws")
        );
        assert_eq!(
            server_url("example.org:8443").as_deref(),
            Some("wss://example.org:8443/ws")
        );
    }

    #[test]
    fn a_local_server_is_reached_in_plain_text_on_the_games_port() {
        for (typed, url) in [
            ("localhost", "ws://localhost:8080/ws"),
            ("127.0.0.1", "ws://127.0.0.1:8080/ws"),
            ("127.0.0.1:9000", "ws://127.0.0.1:9000/ws"),
            ("192.168.1.20", "ws://192.168.1.20:8080/ws"),
            ("[::1]", "ws://[::1]:8080/ws"),
            ("[::1]:9000", "ws://[::1]:9000/ws"),
        ] {
            assert_eq!(server_url(typed).as_deref(), Some(url), "{typed}");
        }
    }

    #[test]
    fn an_address_pasted_from_a_browser_says_whether_it_is_secure() {
        assert_eq!(
            server_url("https://star-racer.xyz/").as_deref(),
            Some("wss://star-racer.xyz/ws")
        );
        assert_eq!(
            server_url("http://example.org:8081").as_deref(),
            Some("ws://example.org:8081/ws")
        );
    }

    #[test]
    fn a_websocket_address_is_taken_as_written() {
        for url in ["ws://127.0.0.1:8080/ws", "wss://example.org/anything"] {
            assert_eq!(server_url(url).as_deref(), Some(url));
        }
    }

    #[test]
    fn what_cannot_be_a_host_is_refused() {
        for typed in ["", "   ", "not an address", "ws://", "/ws", "...", "a b.xyz"] {
            assert_eq!(server_url(typed), None, "{typed:?}");
        }
    }

    /// The command line first, then the settings, then the default.
    #[test]
    fn the_server_asked_for_wins_over_the_one_chosen() {
        let asked = Some("ws://127.0.0.1:8080/ws".to_owned());
        assert_eq!(
            server_to_use(asked, Some("example.org")),
            "ws://127.0.0.1:8080/ws"
        );
        assert_eq!(
            server_to_use(None, Some("example.org")),
            "wss://example.org/ws"
        );
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert_eq!(server_to_use(None, None), "wss://star-racer.xyz/ws");
            // A stored host that is not one any more falls back to the default.
            assert_eq!(server_to_use(None, Some("no good")), "wss://star-racer.xyz/ws");
        }
    }
}
