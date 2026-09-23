# Architecture

## Workspace layout

| Crate | Depends on Bevy | Role |
| --- | --- | --- |
| `client/` | yes (0.19) | The game itself, native and wasm. |
| `server/` | no | Authoritative server: connections, sessions, lobbies and their races. |
| `protocol/` | no | The wire: the messages, their encoding, and the authentication they carry. |
| `sim/` | no | The driving simulation: tracks, car, race progress, autopilot (see [Simulation](simulation.md)). |
| `logger/` | no | How the binaries log, and one rule that has already been got wrong once. |

The last three were a single `shared/` crate until September 2026. Splitting it followed the
sibling project on the same machine, which had kept its protocol and its logger apart from the
start; the simulation came out with them because it had earned it on its own, being five sixths
of `shared/` and knowing nothing of the wire.

The dependencies run one way and only one way. `protocol` uses `sim` — a snapshot carries the
simulation's own `Car`, the welcome carries its `TrackDescription` — and `sim` never reaches back.
So the handling can be read, tested and changed with the wire format out of view, and
`cargo test -p space-race-sim` is all of the driving and none of the rest.

Authentication stayed inside `protocol`, a module rather than a crate. A public key, a nonce and a
signature are three of the fields the handshake messages carry, and nothing in the game signs
anything without also speaking this protocol: a crate of its own would have drawn a boundary
nobody ever crosses.

None of them depends on Bevy, so the server never pulls in the engine. The consequence is a
constraint rather than a preference: the simulation takes its math types from `glam` directly, and
that `glam` must be the exact version Bevy re-exports. A mismatch would compile on both sides and
silently produce two incompatible `Vec2` types. So the version is pinned once, in the workspace's
`[workspace.dependencies]`, and every crate that wants `glam` takes it from there rather than
naming a version of its own.

## Why the server has no ECS

The server owns the race state and nothing else. An ECS earns its place when a large, open-ended
set of components is composed at runtime, which is the client's situation, not the server's. Plain
Rust data structures driven by tokio tasks keep the server small and make its latency easier to
reason about.

## Lobbies and the race loop

Players race in lobbies they create (see [Lobbies](lobbies.md)). The server runs **one task per
lobby, owning all of its state**: its players, their cars and latest inputs, and its phase. It
advances the simulation at `TICK_RATE` and encodes one snapshot per tick, shared by every member.
A directory task owns the list of lobbies and publishes it to browsing players.

Sessions (`server/src/session.rs`, one task per connection) never touch that state. They send
commands through channels — create, join, input, leave — and receive the encoded snapshots through
a queue of their own and the lobby state through a watch channel. The only lock is the set of
connected players, held while one of them is added, moved between the lobby list and a lobby, or
removed. A slow connection cannot stall a lobby: when its queue is full, it skips snapshots.

The ticks use `MissedTickBehavior::Burst`: if a lobby task stalls, the missed ticks are simulated
right away, so race time keeps pace with real time instead of slowing down.

## Game data

The server reads its content from a data directory, `server/data` by default (`--data-dir`):

```text
server/data/
  car.ron                   car tuning, shared by every lobby
  tracks/esplanade.ron      one file per track, named by its key
```

Every track in `tracks/` is loaded and offered to lobby creators. All files are validated at
startup, and the server refuses to start on an invalid one. `car.ron` is then **checked every
second and reloaded when it changes**, so the handling can be tuned while driving without
restarting anything; an invalid edit is logged and ignored. Tracks are not reloaded, since
connected clients received them when they connected.

The default path is relative to the working directory, which fits `cargo run` from the repository
root. A deployed server passes `--data-dir` explicitly.

## Why WebSocket

The deployment target decides this, and rules out the alternatives outright:

- The server runs on a Scaleway machine behind an Apache reverse proxy. Apache terminates TLS with
  a Let's Encrypt certificate and forwards plain text to `127.0.0.1:8080`.
- That proxy speaks TCP. QUIC and WebTransport need UDP and their own TLS termination, so they are
  not available.
- Because TLS is terminated before the server, the server never sees the client's TLS session.
  **TLS client certificates cannot be used for identity**, which is why authentication happens at
  the application layer (see [Protocol](protocol.md)).

The same choice also serves the wasm build, where WebSocket is the only bidirectional transport
browsers offer without extra infrastructure.

## Why bitcode

`bitcode` packs messages far tighter than JSON or MessagePack, and derives `Encode`/`Decode` the
same way `serde` derives its traits. Frames are binary and already length-delimited by WebSocket,
so the protocol needs no framing of its own.

## Client networking

`client/src/net.rs` runs the connection on a dedicated `std::thread` holding a current-thread tokio
runtime. The `Network` resource starts a session when the player presses Play on the login screen,
and ends it to change nickname; the next session gets fresh channels, so nothing from the old one
leaks into it. What the server says reaches Bevy as `NetEvent` messages, forwarded in `First` so every
system sees them the same frame, and a `Connection` resource tracks where the player stands.
Systems send to the server through `Network::send`.

Snapshots are timestamped on the network thread the moment they arrive. Timestamping them when Bevy
reads them would add the frame timing to their arrival jitter, and the interpolation below relies
on arrival times.

On the web there are no threads: the same session runs on the page's event loop instead, and the
identity key lives in local storage (see [Web build](web.md)).

When the game quits, a system in `Last` (after Bevy's own exit systems, so closing the window is
seen too) tells the network thread to send a WebSocket Close frame, and waits for it at most one
second. Without it the process dies with the socket open and the server sees a reset connection
instead of a player leaving.

## Displaying the race

The client module layout follows the path of the data:

| Module | Role |
| --- | --- |
| `net` | Sessions started on demand, `NetEvent` messages, `Connection`, `Network`. |
| `screen` | The `Screen` state (login, lobbies, lobby) and what moves the player between them. |
| `lobby` | The server's tracks, the lobby list and the lobby the player is in. |
| `track` | Builds the lobby's track with the shared code, and its meshes. |
| `race` | Snapshot buffer, interpolation, car entities, `RaceView` (the cars and the followed one). |
| `controls` | Gamepad, keyboard, touch or autopilot input (accelerate, drift, steer), sent on change and numbered. |
| `latency` | Times the ping and every input to the server and to the screen (see [Latency](latency.md)). |
| `prediction` | The player's own car, driven on a timeline ahead of the server and put right by its snapshots. |
| `camera` | Chase camera and light. |
| `ui` | Every screen, the HUD and their widgets (see [Interface](ui.md)). |
| `nickname` | Generated nicknames. |
| `world` | The mapping from the simulation plane to Bevy's `y`-up world. |
| `settings` | Command line on native, page address on the web. |
| `capture` | The screenshot test mode, native only. |

**Interpolation.** Snapshots come 60 times per second but never exactly on time, and frames do not
line up with ticks. The client estimates the server clock from arrival times — adopting at once a
snapshot that arrives earlier than expected, following late ones only slowly, so a single delayed
snapshot barely moves the estimate — and displays the race **three ticks (50 ms) in the past**.
There is then almost always a snapshot on each side of the displayed moment, and cars move
smoothly at any frame rate. If snapshots stop, the last state is held.

That delay is kept for the other cars, which it makes move smoothly. The player's own car is not
displayed from the snapshots but predicted, ahead of the server, and put right by them (see
[Latency](latency.md#prediction)).

Arrival times and the current frame time must be measured from the same origin. Bevy's
`Time::elapsed` counts from the first frame, which starts most of a second after the app on a slow
machine; mixing it with arrival instants once displayed the race 770 ms late instead of 50. Both
now go through a single conversion, `seconds_since_startup`.

**The track** floats alone on a black background: a dark road, walls striped dark gray and neon
blue every 4 m, a neon blue dashed centerline and a checkered start line. The stripes and dashes
matter more than they look: on a plain road, speed is nearly invisible. It is two vertex-colored
meshes, one per material: lit surfaces, and unlit neon that stays bright whatever the lighting.
The road is cut into strips across its width so it follows the curve of banked turns, walls stand
on its edges with their outer face down to the ground, and cars sit on the surface, leaning with
its slope. The camera follows the car's height but stays level.
Quads derive their triangle winding from their intended normal, so no face can end up invisible
by mistake.

**Cars** are drawn with their body turned into a drift (`Car::body_heading`), an angle the
simulation does not drive along (see [Simulation](simulation.md#drift)).

**The camera** follows behind the player's car, or the watched one when spectating, at a heading
that lags slightly behind it, so a turn reads as the car rotating rather than the whole world
spinning around a fixed car. It follows the **direction of travel**, not the heading: in a drift
the car is sideways on screen, which is the whole point, and a camera locked to the heading would
hide the slide and swing the track instead. Below walking speed it falls back to the heading, which
has no direction of travel to follow. The field of view widens with speed. Tone mapping is off:
Bevy's default one desaturates bright colors, which turned the neon blue into a dull sky blue.

## Build profiles

`[profile.dev]` builds our own code at `opt-level = 1` and every dependency at `opt-level = 3`,
which is Bevy's recommendation: the engine is unusable when its dependencies are unoptimized.

Local work builds and runs this dev profile (see `CLAUDE.md`). Until September 2026 it built release
instead, the machines of the time being too slow to run even an optimized Bevy under an unoptimized
game; on the current one, the dev profile plays at well over a hundred frames per second and
rebuilds our own code in seconds. CI builds the same profile with `CARGO_PROFILE_DEV_DEBUG=0`,
since it only needs to know the code compiles and debug info is expensive to produce and cache.
Release is built by the deployment alone, which is why it builds before anything is replaced: a
release-only compilation error would not be caught anywhere else.

## Feature unification

Cargo enables a dependency's features as the union of what every crate *in the same build*
requests. `cargo build -p space-race-server` and `cargo build --workspace` can therefore
produce two different server binaries: in the second, Bevy's requirements leak into the server's
dependencies.

This already bit once. Bevy turns on `tracing-subscriber`'s `env-filter` feature, which changes
`tracing_subscriber::fmt::init()` to default to the `error` level when `RUST_LOG` is unset. Built
alone the server logged normally; built with the workspace it was silent. That is now `logger/`'s
whole reason for being: it turns `env-filter` on itself and sets the level explicitly, in one
place, so every binary behaves the same however it was built.

Rule of thumb: never rely on a dependency's defaults in a way a feature flag can change. Set the
behavior explicitly, and turn on the features that behavior needs.

It also costs build time: switching between `-p <crate>` and `--workspace` builds changes the
feature sets, which invalidates part of the cache and recompiles shared dependencies, Bevy's
included. On a slow machine, always build the same way, with `--workspace`.

## Testing

- `protocol/` unit tests cover message encoding, nickname rules and signatures. `sim/` covers
  track geometry, car behavior, and an autopilot lap proving a circuit is drivable (see
  [Simulation](simulation.md#tests)). `logger/` covers what a typo in its default filter would
  cost, which is every line of every log.
- `server/src/lobby.rs` tests step a lobby tick by tick, without a network: cars lining up as soon as
  there are enough players, cars held on the grid until the start, a full race driven by the
  autopilot ending with results and a new race, stragglers timed out, players joining on the grid
  racing, players joining during a race spectating it then getting a car, a full lobby refusing players, starts
  graded from the tick players saw, bounded, with their boost.
- `server/src/tests.rs` starts a real server on a free port, on the shipped data, and drives
  it with raw WebSocket clients. It covers what the game client never does on purpose (a wrong
  protocol version, an invalid nickname, a signature from another key, the same identity twice,
  invalid lobby settings) and a player's path: the track list, creating a lobby and seeing it
  listed, joining it, the race starting with enough players, snapshots,
  accelerating moving the car, a second player seen and removed when leaving, the lobby
  disappearing with its last player.
  These tests never count snapshots or assume timing: snapshots queue up while a test is busy, and
  CI runners are slower than a desktop, so a test waits for the state it expects, with a timeout.
  Counting once passed locally and failed on GitHub.
- `server/src/content.rs` tests load every shipped track and let the autopilot lap it with the
  shipped car tuning, gripping and drifting, live and 6 ticks late. The drifting lap must not touch
  a wall, must boost and must beat the gripping one, and some style must stay clean when late. An
  edit that makes a track undrivable, or drifting pointless, fails CI.
- All of these run in the "Test server" workflow, and again in "Deploy" before anything goes
  online (see [Continuous integration](ci.md)).
- The client's `--capture <file.png>` mode waits until the screen the other options lead to is
  shown (login, lobby list, creation dialog or lobby, see [Interface](ui.md#testing)), or the
  connection has failed so the screenshot shows why, saves a screenshot and exits, so a change can be
  checked without anyone at the keyboard. `--capture-delay <seconds>` waits longer, so the cars
  have moved. It waits until the render world reports no pipeline left to compile rather than for
  a fixed number of frames: on a slow GPU, shaders take seconds to compile and the first frames are
  not drawn at all, which gave pure black screenshots. It gives up with an error after two minutes
  rather than hanging.
- `--nickname`, `--lobby`, `--track`, `--laps`, `--min-players` and `--create-dialog` skip the
  login screen and the lobby list, so a client reaches a race with nobody at the keyboard.
- `--autopilot` lets the shared autopilot drive the client's car, through the same input path as
  a player. With `--capture-delay`, it shows the car mid-lap, the camera behind it.
  `--autopilot-drift` uses the drifting style, so the capture catches a slide and the DRIFT gauge.
- `--identity <dir>` points a client at its own key, to run several players on one machine.
- Client unit tests cover the snapshot interpolation and clock estimate, the world mapping, quad
  winding, generated nicknames and HUD formatting.

Diagnostics for what tests cannot judge:

- The HUD shows the frame rate, top right.
- `RUST_LOG=info,space_race_server::lobby=debug` makes the server log, once per second, each
  car's status, race progress, speed, heading and input. Reading those while a
  client drives is how the display latency bug described in [Displaying the race](#displaying-the-race) was found.

## Known gaps

- The native client cannot reach `wss://` yet: it needs a TLS feature of `tokio-tungstenite-wasm`
  that verifies the real Let's Encrypt certificate. No self-signed certificates — if native TLS is
  ever needed for local testing, it gets a proper local fake CA.
- Cars do not collide with each other.
- The lobby list does not scroll yet: lobbies beyond the height of its panel are cut off.
- The native client leaves Nagle's algorithm on (see [Protocol](protocol.md#racing)).
