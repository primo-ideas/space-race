# Protocol

Defined in `protocol/src/lib.rs` and `protocol/src/auth.rs`, so the client and the server compile
against the same definitions and cannot drift apart.

## Framing

One message per binary WebSocket frame, encoded with `bitcode`. WebSocket already delimits frames,
so the protocol adds no length prefix of its own.

- The server caps incoming frames at `MAX_MESSAGE_SIZE` (64 KiB) through axum's `max_message_size`,
  so a peer cannot make it allocate an arbitrary buffer with a single message.
- Text frames carry no meaning and are ignored on both sides.
- Ping and pong are handled by axum; the protocol never sends them itself.

## Versioning

`PROTOCOL_VERSION` is bumped on every incompatible change. The client sends it in `Hello`, and a
mismatch is answered with `Rejected(ProtocolMismatch { server_version })`. The reason carries the
server's version so the client can tell the player which of the two sides is out of date, instead
of just failing.

## Handshake

The server allows 5 seconds (`HANDSHAKE_TIMEOUT`) for the whole exchange.

```
Client                                        Server
  |                                              |
  |-- Hello { version, nickname, public_key } -->|
  |                                              |  check version, then nickname
  |<------------- Challenge { nonce } -----------|  32 fresh random bytes
  |                                              |
  |-- ChallengeResponse { signature } ---------->|
  |                                              |  verify signature against public_key,
  |                                              |  and that this identity is not connected
  |<---------------- Welcome { tracks } ---------|
```

At any of the four checks the server answers `Rejected(reason)`, sends a Close frame and drops the
connection. Anything other than the expected message at that point is a protocol error, and the
connection is dropped without a reply.

## Tracks

`Welcome` carries every track the server has, as `TrackInfo { key, description }`.

- **Tracks are downloaded, not installed.** The server sends the descriptions it loaded from its
  files, and the client builds the same geometry from them with the shared code. They are a few
  hundred bytes each, and a server can add tracks without any client update.
- Lobby settings name a track by its `key`, the file name without `.ron`.
- A description carries, along with the segments that draw the road, the bottlenecks that pinch it
  in (see [Tracks](tracks.md#bottlenecks)) and the scenery standing beside it (see
  [Tracks](tracks.md#scenery)). The bottlenecks are part of the road, and both sides build the same
  walls from them; props are decoration the client builds, which the server sends and never
  simulates.

## Browsing lobbies

Once welcomed, the player browses lobbies (see [Lobbies](lobbies.md)).

```
Client                                        Server
  |<---------------- Lobbies(list) --------------|  right after Welcome, then on every change
  |<---------------- Players(list) --------------|  right after Welcome, then on every change
  |-- CreateLobby(settings) or JoinLobby(id) --->|
  |<------ JoinedLobby { lobby, car, settings } -|  or JoinFailed(reason), and keep browsing
```

- **The list is sent whole** whenever a lobby changes: players joining or leaving, a phase
  changing, the leader starting a new lap. A slow connection gets the latest list, never a backlog.
- **`Players` says who is connected**, in the order they arrived, each with a `Presence`:
  `Browsing`, or `InLobby(id)`. It is sent to browsing players only, they being the only ones who
  see it, and it changes when a player connects, disconnects, enters a lobby or comes back to the
  list. A lobby is named by its id alone: the lobby list already carries its name and what it is
  doing, and the client looks it up there, so the two can never disagree. A player whose lobby is
  not in the list the client holds is shown as simply being in one.
- **Countdowns are durations** (`remaining_ms`), counted from when the list was sent: the client has
  no server clock while browsing. The server keeps deadlines and converts them at sending time.
- **Creating a lobby enters it**, as its first player.

## In a lobby

```
Client                                        Server
  |<------------- Lobby(state) ------------------|  right after JoinedLobby, then on every change
  |<------------- Snapshot { tick, cars } -------|  every tick, 60 per second
  |-- Input { input, tick, seq } -------------->|  whenever the input changes, for a tick ahead
  |-- Ping(n) ---------------------------------->|  twice a second, here and while browsing
  |<------------- Pong(n) -----------------------|  at once
  |-- LeaveLobby ------------------------------->|  back to browsing
  |<------------- Lobbies(list) -----------------|
```

- **The lobby state** is the phase and the players with their status (driving, racing, finished
  with a time, spectating) and how well they started the race (see
  [Lobbies](lobbies.md#the-start)). Phase deadlines are ticks, the clock of snapshots, so the
  client shows the start lights against the race it displays: the last, green one lights up when
  the cars on screen start moving. A slow connection skips to the latest state.
- **The server has full authority.** It simulates every car and sends the result; the client only
  sends what the player presses.
- **Inputs are sent when they change**, not on a timer. WebSocket runs over TCP, so nothing is lost
  and the server always knows the latest input. Inputs are quantized (a few bytes, see
  [Simulation](simulation.md#inputs)), which also keeps an analog stick from flooding the server
  with imperceptible changes. A player enters a lobby with nothing pressed.
- **The server disables Nagle's algorithm** (`TCP_NODELAY`) on every connection, so small messages
  leave at once instead of waiting to be batched. The native client cannot do the same yet:
  `tokio-tungstenite-wasm` does not expose the option. Measured locally, inputs still reach the
  server in under a millisecond, because the constant snapshot stream acknowledges them right
  away; it should be checked again over a real network.
- **An input carries the tick it takes effect on**: the one the client applied it on as it
  predicted the player's car, ahead of the server by a round trip and a margin (see
  [Latency](latency.md#prediction)). The server holds it until that tick, so both sides step the
  car with the same inputs on the same ticks, and grades the start on it (see
  [Lobbies](lobbies.md#the-start)). It clamps the tick: never before its own next tick -- the past
  is not rewritten, and an input arriving late takes effect at once -- never before an input sent
  earlier, never more than `MAX_INPUT_LEAD` (30 ticks, half a second) ahead. A player may have at
  most 256 inputs waiting.
- **The lobby state carries the car tuning** the lobby drives with, which the client predicts the
  player's car with. The server reloads `car.ron` while it runs; a lobby publishes its state again
  when it does.
- **Inputs are numbered, and snapshots say which one they include.** `seq` counts the player's
  inputs in the lobby from 1, and each car of a snapshot carries `input_seq`, the last of its
  driver's inputs in effect when the server took it; 0 before any. The client times its input lag
  from it (see [Latency](latency.md)). It rides in every car of
  the shared snapshot rather than in a message of each player's own, so the snapshot is still
  encoded once for the whole lobby.
- **A ping is answered at once**, by the connection's own task and not the lobby's, so it times
  the network and nothing of the game. It is the client's to time; the server keeps nothing.
- **A snapshot holds every car of the lobby**: its id, position, velocity, heading, slip angle, yaw
  rate, drift state and boost, race progress (see
  [Lobbies](lobbies.md#progress-laps-and-finishing)), and the drift and boost gauges the server
  works out for the HUD (see [Simulation](simulation.md#drift)), stamped with the tick it was
  reached at. Spectators have no car. A car missing from a snapshot has left or is spectating.
- **Snapshots may be skipped.** Each player has a small queue on the server; when a connection
  cannot keep up, new snapshots are dropped rather than slowing down the lobby for everyone. The
  next snapshot supersedes the missing ones, and the tick numbers show the gap.
- **Leaving is immediate on the client**, which ignores the lobby messages still on their way. The
  server processes messages in order, so everything about the old lobby arrives before anything
  about a lobby joined later.

## Nicknames

`is_valid_nickname` accepts 1 to `MAX_NICKNAME_CHARS` (24) characters. Lobby names follow the same
rules, up to `MAX_LOBBY_NAME_CHARS` (32). The count is in characters,
not bytes, so non-ASCII nicknames are not unfairly shortened. Leading and trailing whitespace is
refused rather than trimmed — silently changing what a player typed would let two nicknames look
identical while differing. Control characters are refused so a nickname cannot corrupt logs or
in-game text.

## Authentication

There are no accounts, no e-mail and no password in this alpha. **A player is their Ed25519 public
key.** The client generates a key on first launch and keeps it; on native it lives at
`%APPDATA%\Space Race\data\identity.key`. Losing that file means losing the identity.

The signed message is not the bare nonce but:

```
b"space-race/auth/v1" || nonce
```

That context prefix is domain separation: a signature produced for this game is worthless in any
other protocol that might one day ask the same key to sign something, and vice versa.

Verification uses `verify_strict`, which rejects the malleable and small-order edge cases that
plain `verify` accepts. The nonce is freshly generated for each connection, so a captured
`ChallengeResponse` cannot be replayed on a later one.

In logs, a player is identified by the first 8 bytes of their public key in hex — enough to tell
players apart without dumping a full key on every line.

## Disconnecting

A client that quits sends a WebSocket Close frame. The server treats every end of the connection
after `Welcome` as the player leaving and logs the reason — a clean close, but also a reset
connection or an undecodable message. A crash or a lost network is an ordinary way to leave, so
the session removes the player from their lobby on all of these paths, not only on a clean close.

## Rejection reasons

| Reason | Meaning |
| --- | --- |
| `ProtocolMismatch { server_version }` | Client and server speak different protocol versions. |
| `InvalidNickname` | The nickname failed `is_valid_nickname`. |
| `AuthenticationFailed` | The signature did not verify against the announced public key. |
| `AlreadyConnected` | This identity is already connected, from another connection. The first connection stays. |

## Join errors

`JoinFailed` leaves the player browsing. The client shows the reason for a few seconds.

| Error | Meaning |
| --- | --- |
| `NoSuchLobby` | The lobby does not exist, or closed since the list was sent. |
| `LobbyFull` | The lobby holds its maximum number of players. |
| `InvalidSettings` | The settings of a new lobby fail `LobbySettings::is_valid`. |
| `UnknownTrack` | No track has the key of a new lobby's settings. |

## Known gaps

- No rate limiting on connections, failed handshakes or inputs.
- The input tick is the client's word. Bounded, a cheating client gains at most 250 ms on its
  start: enough to turn a D into an S.
- No limit on how many lobbies a player can create one after the other.
- Snapshots always carry every car in full. That is about 35 bytes per car per tick, fine for a
  handful of players; delta compression can come when races get crowded or bandwidth matters.
