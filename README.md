# Space Race

[![Build server](https://github.com/primo-ideas/space-race/actions/workflows/build-server.yml/badge.svg)](https://github.com/primo-ideas/space-race/actions/workflows/build-server.yml)
[![Test server](https://github.com/primo-ideas/space-race/actions/workflows/test-server.yml/badge.svg)](https://github.com/primo-ideas/space-race/actions/workflows/test-server.yml)
[![Build client (native)](https://github.com/primo-ideas/space-race/actions/workflows/build-client-native.yml/badge.svg)](https://github.com/primo-ideas/space-race/actions/workflows/build-client-native.yml)
[![Build client (wasm)](https://github.com/primo-ideas/space-race/actions/workflows/build-client-wasm.yml/badge.svg)](https://github.com/primo-ideas/space-race/actions/workflows/build-client-wasm.yml)
[![Deploy](https://github.com/primo-ideas/space-race/actions/workflows/deploy.yml/badge.svg)](https://github.com/primo-ideas/space-race/actions/workflows/deploy.yml)

An online multiplayer racing game written in Rust with [Bevy](https://bevy.org).
The graphics are deliberately geometric.

> **All the code is written by an AI.** Claude (Anthropic), through Claude Code, writes the code and the documentation, and makes the technical choices.
> The human maintainer sets the direction, reviews and approves.

## Running locally

```sh
cargo build --workspace
target/debug/space-race-server
target/debug/space-race-client
```

A debug build is enough to play: the dev profile optimizes every dependency, Bevy included.

Building the whole workspace at once keeps a single set of dependency features: building the crates one by one recompiles part of Bevy each time.

On Windows, `./run.ps1` builds everything, starts the server in the background (its log goes to `target/server.log`), runs the client, and stops the server when the client closes. Arguments go to the client.

Run both from the repository root: the server loads its tracks and car tuning from `server/data` (see [Game data](docs/architecture.md#game-data)). It listens in plain text on `127.0.0.1:8080`; in production, a reverse proxy terminates TLS and forwards WebSocket connections to it.

## Playing

Pick a nickname (or keep the one offered), then create a lobby or join one. A lobby chooses its track, its number of laps and how many players it takes to start a race. Races follow one another: the grid and its start lights once enough players are in, the race, the results. Joining after the start lets you watch the race, and you race in the next one.

| | Gamepad | Keyboard |
| --- | --- | --- |
| Accelerate | R (right bumper) | W |
| Steer | Left stick or D-pad | A / D |
| Tighten a drift | X, held | Shift, held |
| Leave the lobby | Start | Escape |
| Menus | D-pad or left stick, A, B | arrows, Enter, Escape |

There is no brake: the game is full speed all the time. **You drift by turning hard.** Past a point the nose comes round further than the car travels, the tyres let go, and you are sliding -- no button needed. From there the stick is everything: into the slide to tighten it, centered to run straight, against it to catch it. Come back into line and the drift ends and pays a boost, the longer the slide the bigger it is. Holding the drift button tightens the turn further and slows you more, and held as you straighten it keeps the drift alive into the next corner. Press accelerate right as the race starts for a start boost, graded from S to E. The gauge over the speed shows it building up.

The car handling is tuned in `server/data/car.ron`. The server reloads it within a second of every save, so you can keep driving while adjusting it.

Useful client options (`--help` for all of them): `--nickname` to skip the login screen, `--lobby <name>` to enter a lobby (created with `--track`, `--laps` and `--min-players` if it does not exist), `--identity <dir>` to run several clients on one machine, `--autopilot` (or `--autopilot-drift`) to let the car drive itself, and `--capture <file.png>` with `--capture-delay <seconds>` to take a screenshot and exit. The game is quiet by default: `--log info` (or `?log=info` in the browser) makes it say what it is doing. For example, a race alone: `--nickname Primo --lobby Test --min-players 1`.

## In a browser

The client also runs in the browser (WebGL2). With the server running and `cargo install wasm-server-runner` done:

```sh
cargo run --release -p space-race-client --target wasm32-unknown-unknown
```

then open <http://127.0.0.1:1334>. Settings go in the page address, for example `?nickname=Primo`. Tools, deployment builds and `wasm-opt` are described in [docs/web.md](docs/web.md).

## Documentation

[`docs/`](docs/README.md) explains the technical decisions: the [architecture](docs/architecture.md),
the [protocol](docs/protocol.md), the [simulation](docs/simulation.md), the [track format and its scenery](docs/tracks.md), [lobbies and races](docs/lobbies.md), the [interface](docs/ui.md), the [web build](docs/web.md),
the [continuous integration](docs/ci.md) that checks it and the [deployment](docs/deploy.md) that puts it online.

## License

[The Unlicense](LICENSE): the project is in the public domain.
Since the code is AI-generated, it may not be covered by copyright at all. The Unlicense removes any doubt: you can reuse, modify and redistribute it freely, with no conditions.

The one exception is the Saira font in `client/assets/fonts/`, by the Saira Project Authors, under the [SIL Open Font License](client/assets/fonts/OFL.txt).
