# Documentation

The reasoned technical decisions behind Space Race. `CLAUDE.md` holds the working rules and
the current status; these pages explain *why* the code looks the way it does.

- [Architecture](architecture.md) — workspace layout, transport choice, deployment constraints.
- [Protocol](protocol.md) — wire format, versioning, handshake, authentication.
- [Simulation](simulation.md) — conventions, the car model and its tuning, drift, walls, autopilot.
- [Tracks](tracks.md) — the track file format, transitions, scenery, validation, built geometry.
- [Lobbies](lobbies.md) — lobby settings and phases, race rules, how the server runs them.
- [Interface](ui.md) — screens, look, widgets, keyboard and gamepad navigation.
- [Sound](sound.md) — sounds synthesized in code, and when they play.
- [Latency](latency.md) — where the time between a press and the car's answer goes, and how it is measured.
- [Web build](web.md) — building and serving the browser client, what differs from native.
- [Deployment](deploy.md) — setting the machine up once, and the two ways a build goes online.
- [Continuous integration](ci.md) — the four checks, the deployment, and why it could not exist before.
