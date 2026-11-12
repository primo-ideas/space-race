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
`cargo test -p star-racer-sim` is all of the driving and none of the rest.

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

The pin does not hold everything by itself. `bitcode`, whose `glam` feature encodes those types
on the wire, asks for any `glam` from 0.21 up, so `cargo update` gives it the newest release
rather than the pinned one, and its encoders then serve another crate's `Vec2`. After an update,
`cargo tree -d | grep glam` must show nothing; when it shows a second version, bring `bitcode`
back onto the pinned one with `cargo update -p glam@<newer> --precise <pinned>`.

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
  tracks/skyway.ron         one file per track, named by its key
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

**A connection lost is tried again** (`client/src/reconnect.rs`). Once the server has welcomed
the player, losing it -- a phone moving from Wi-Fi to its mobile network, a browser put to sleep,
a server restarting for an update -- starts new sessions under the same nickname on their own,
0.5 s after the loss, then after 1, 2, 4 and every 8 s, for as long as the page is open, and once
welcomed back the client joins the lobby it was in again, if it is still listed (the next race,
as anyone joining). The login screen says it is reconnecting meanwhile, in amber, and pressing
PLAY is a try like any other. A rejection stops it: a server on another protocol version will not
change its mind, and a connection replaced by the same player's newer one must not take its place
back, or two windows would push each other out forever. It is the server letting the newest
connection of a key replace the one it holds (see [Lobbies](lobbies.md#on-the-server)) that lets
a player come back before the server has noticed they had gone.

When the game quits, a system in `Last` (after Bevy's own exit systems, so closing the window is
seen too) tells the network thread to send a WebSocket Close frame, and waits for it at most one
second. Without it the process dies with the socket open and the server sees a reset connection
instead of a player leaving.

## Displaying the race

The client module layout follows the path of the data:

| Module | Role |
| --- | --- |
| `net` | Sessions started on demand, `NetEvent` messages, `Connection`, `Network`. |
| `reconnect` | Tries a lost connection again, and goes back into the lobby the player was in. |
| `screen` | The `Screen` state (login, lobbies, lobby) and what moves the player between them. |
| `lobby` | The server's tracks, the lobby list and the lobby the player is in. |
| `track` | Builds the lobby's track with the shared code, and its meshes. |
| `race` | Snapshot buffer, interpolation, car entities, `RaceView` (the cars and the followed one). |
| `car_model` | What a car looks like: its hull, lights, flames and the light under it, in two liveries. |
| `weight` | The cars' bodies on their springs, and how the followed car is pushed, for the camera. |
| `drift_meter` | The drift charge drawn on the road around the back of the player's car, as Rocket Racing draws it. |
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
matter more than they look: on a plain road, speed is nearly invisible. Kerbs line its turns, the
neon of the walls throws pools of light across it, and piers carry it down into the dark (see
[Night](night.md)). It is vertex-colored meshes, one per material: lit surfaces, unlit neon that
stays bright whatever the lighting, and the pools of light added to what they fall on.
The road is cut into strips across its width so it follows the curve of banked turns, and it
climbs and dives with the relief. It is a slab 1.5 m thick with an underside, floating wherever the
relief carries it, and the walls stand on its edges with their outer face running down to that
underside, so a road seen from lower down the relief is solid rather than a sheet that vanishes
from below. Cars sit on the surface, leaning with its slope.
Quads derive their triangle winding from their intended normal, so no face can end up invisible
by mistake.

**Cars** are drawn with their body turned into a drift (`Car::body_heading`), an angle the
simulation does not drive along (see [Simulation](simulation.md#drift)).

Since v0.1.21 a car is a racing hovercraft in the spirit of F-Zero's machines rather than three
boxes (`client/src/car_model.rs`): a long wedge of a hull drawn out to a pointed nose, a cockpit
bubble on its back, two engine pods slung beside it on a rear wing, each wearing a fin, a splitter
under the nose and a white stripe down the middle. It is built from the same flat-shaded,
vertex-colored triangles as the circuit, from cross-sections of eight points lofted along the car,
about 4.5 m long and 2.8 m wide, a little more than the simulation's 2.4 m collision circle, and
its underside clears the road by a quarter of a meter: nothing holds it up. Its livery says whose
it is: the player's own orange, lined with orange neon, every other blue, lined with magenta. Three
meshes per livery, one shared by all, and four materials, shared by every car:

- **the hull**, lit, roughness 0.42, casting the car's shadow;
- **the lights**, unlit and brighter than white so the bloom haloes them: the thruster nozzles,
  white hot, and the neon lines along the hull's flanks and the pods' tops;
- **the shadow under the car**, one mesh for all, unlit, black, alpha-blended: an oval as long and
  as wide as the hull, darkest under its middle and gone by its edge (since v0.1.33). The hull casts
  a real shadow from the moon, but the moon is nearly overhead and dim against the night's ambient,
  and the light under the car was added on top of it: the car hovered over nothing. This is what
  sits it over the road;
- **the light under the car** and **the flames**, unlit and added to what is behind them. Since
  v0.1.33 the light is an oval spilling out from under the hull's edge, none in the middle, most
  just outside the outline and none 1.5 m beyond; it was a rectangle 5.2 by 3.2 m evenly lit to
  its middle, a slab of muddy orange wider than the car, whose straight edge cut across a banked
  road. The light and the shadow fade to nothing at their rim, where a flat shape would sink into
  a road that curves up on a bank, and both hang from the car rather than its body, so they stay
  on the road while the body sways. The flames, a pyramid per nozzle fading from blue to nothing
  at its tip, are stretched every frame to the car's speed, from 0.3 m at rest to 0.9 m at top speed, 0.85 m longer on a
  boost, flickering by 8 % to each car's own beat (`light_thrusters`).

The whole car is under 1,000 triangles (800 before the shadow), checked by a test, as is its size
against the collision circle, the clearance under it, and the lights being the only part bright enough to bloom.

**Weight.** The car drives heavy (see [Simulation](simulation.md#weight)), and since v0.1.15 it
looks it. A car entity is placed on the road, and its body, a child of it, rides on springs over
that place (`client/src/weight.rs`): it pitches nose up as the car gathers speed and down as it
loses it, rolls to the outside of a turn, sinks when the road presses into it and lifts when it
falls away, then settles slowly, overshooting a little. Nothing is scripted. What pushes the
springs is the acceleration the car really has, taken from how its displayed velocity changes
between frames, with gravity added, and turned into the car's own frame: a car standing on a bank
leans down it, a car taking a bank at speed leans less, and a wall jolts the body because it
changes the velocity in one tick. The velocity is used rather than the displayed position because
a prediction's correction glides the position and never the velocity, so a correction moves
nothing on the springs; a car whose position or speed jumps further than any driving takes it
was put somewhere else, a new grid, and starts at rest. The springs are stepped at 120 Hz at
least and eased into their limits, so they behave the same at any frame rate.

The numbers come from a lap of the skyway logged frame by frame. Pitch runs from -1 to 4 degrees,
the start boost and the drifts lifting the nose; roll reaches 4 degrees either way. The road
itself heaves the car hardest: crossing a banking transition off the centerline lifts it at four
or five times gravity (at 47 m/s, 1.7 m in half a second), so the body's heave is the stiffest of
its springs and held to 8 cm, where the first, softer one bobbed through every transition at its
15 cm limit.

**The camera** follows behind the player's car, or the watched one when spectating, at a heading
that lags slightly behind it, so a turn reads as the car rotating rather than the whole world
spinning around a fixed car. It follows the **direction of travel**, not the heading: in a drift
the car is sideways on screen, which is the whole point, and a camera locked to the heading would
hide the slide and swing the track instead. Below walking speed it falls back to the heading, which
has no direction of travel to follow. It hangs behind along the road rather than level with the
car, climbing and diving with the slope under it, lagging the same way the heading does: level, it
would sink into the road behind a car going down a hill and stare into the road ahead of one going
up. No camera, the finish shots included, is ever let below the road under it. The field of view
widens with speed. Tone mapping is off:
Bevy's default one desaturates bright colors, which turned the neon blue into a dull sky blue. The
camera renders in high dynamic range all the same, for the bloom that makes the neon glow, and sees
through a haze in races (see [Night](night.md)).

Chasing the car, the camera feels its weight too, the push the car's springs feel, on a mount of
its own: it hangs back as the car pulls away and closes in as it slows (a boost takes it about a
meter further back, a wall throws it toward the car), trails the car's rises and falls (by 0.3 m
at most: that is the gain of a mount following on a spring, and the banking transitions would
make anything stronger seasick), and shakes when the car hits something, by how much speed the
hit took. A hit is a change of velocity faster than any driving makes, 150 m/s², where a boost
pushes at 35 and the tightest drift turns the car at about 90. The finish shots, which no longer
ride with the car, feel none of it.

## Build profiles

`[profile.dev]` builds our own code at `opt-level = 1` and every dependency at `opt-level = 3`,
which is Bevy's recommendation: the engine is unusable when its dependencies are unoptimized.

Local work builds and runs this dev profile (see `CLAUDE.md`). Until September 2026 it built release
instead, the machines of the time being too slow to run even an optimized Bevy under an unoptimized
game; on the current one, the dev profile plays at well over a hundred frames per second and
rebuilds our own code in seconds. CI builds the same profile with `CARGO_PROFILE_DEV_DEBUG=0`,
since it only needs to know the code compiles and debug info is expensive to produce and cache.
Release is built by the deployment alone, the server in release itself and the web client in the
`web` profile that inherits from it and is built for size (see [Web build](web.md#size)), which
is why it builds before anything is replaced: a release-only compilation error would not be
caught anywhere else.

## Feature unification

Cargo enables a dependency's features as the union of what every crate *in the same build*
requests. `cargo build -p star-racer-server` and `cargo build --workspace` can therefore
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
  protocol version, an invalid nickname, a signature from another key, the same identity twice
  replacing its older connection, in the list and mid-race,
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
- `--guest` (or `--account` with `--password`), `--lobby`, `--track`, `--laps`, `--min-players`
  and `--create-dialog` skip the first screen and the lobby list, so a client reaches a race with nobody at the keyboard.
  Against a local server, they go with `--server 127.0.0.1`: the client's default is the deployed
  server ([Protocol](protocol.md#the-servers-address)), which a check must not race on by accident.
- `--autopilot` lets the shared autopilot drive the client's car, through the same input path as
  a player. With `--capture-delay`, it shows the car mid-lap, the camera behind it.
  `--autopilot-drift` uses the drifting style, so the capture catches a slide and the DRIFT gauge.
- `--identity <dir>` points a client at its own key, to run several players on one machine.
- Client unit tests cover the snapshot interpolation and clock estimate, the world mapping, quad
  winding, generated nicknames and HUD formatting, and the cars' springs: a body pitching up as
  the car gathers speed and settling where it should, rolling out of a turn, sinking when pressed
  into the road, staying within its limits at 15 and at 300 frames per second, a hit told apart
  from the tightest drift, and a car put on a new grid not thrown about.

Diagnostics for what tests cannot judge:

- The HUD shows the frame rate, top right.
- `RUST_LOG=info,star_racer_server::lobby=debug` makes the server log, once per second, each
  car's status, race progress, speed, heading and input. Reading those while a
  client drives is how the display latency bug described in [Displaying the race](#displaying-the-race) was found.

## Known gaps

- Cars do not collide with each other.
- The lobby list does not scroll yet: lobbies beyond the height of its panel are cut off.
- The native client leaves Nagle's algorithm on (see [Protocol](protocol.md#racing)).
