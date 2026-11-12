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

## The server's address

The player names a server by its host, in the settings on the first screen or with `--server`, and
the client works out the WebSocket address (`settings::server_url`):

| Typed | Connects to | Why |
| --- | --- | --- |
| `star-racer.xyz` | `wss://star-racer.xyz/ws` | a host out on the network is behind the proxy, over TLS, at the path every deployment serves |
| `primo-ideas.xyz/star-racer` | `wss://primo-ideas.xyz/star-racer/ws` | a path is the directory the game is served from |
| `localhost`, `127.0.0.1`, `192.168.…`, `10.…`, `[::1]` | `ws://…:8080/ws` | a server run by hand has no proxy and no certificate, and listens on its own port |
| `https://…`, `http://…` | `wss://…`, `ws://…` | pasted from a browser, the scheme says which |
| `ws://…`, `wss://…` | as written | |

Natively the default is **`star-racer.xyz`** (the user's choice, 2026-11-11): the game plays where
everyone else does. What wins: `--server` (or `?server=`), then the host typed in the settings,
then the default. A host typed in the settings is remembered with the other menu choices, and
changing it disconnects from the server before: the next button pressed connects to the new one.
The login screen's bottom line says which server the game is on.

**Local development keeps reaching its own server by saying so**: `run.ps1` passes
`--server 127.0.0.1` unless it is given a `--server` of its own, and every unattended check against
a local server names it (`--server 127.0.0.1`). `127.0.0.1` rather than `localhost`: on Windows
`localhost` tries `::1` first, where the server does not listen, before falling back. The tests do
not go through the address at all; the integration tests start the server and connect to its port.

**The web page** keeps finding its server from its own address (see [Deployment](deploy.md#the-web-client-finds-its-own-server)):
the host and directory it was served from, which is also what its settings show. Its server field
is not offered on a phone, whose page already came from its server; on a desktop browser it is,
the host typed there being reached as above.

### TLS natively

A browser does its own TLS; the native client brings its own: `tokio-tungstenite-wasm` with its
`rustls-tls-webpki-roots` feature, native only, so nothing of it reaches the web build. The
server's certificate is checked against Mozilla's list of roots compiled into the game
(`webpki-roots`), so it asks nothing of the machine and behaves the same on Windows, Linux and a
fresh install, and Let's Encrypt is in it. rustls is named once in `client/Cargo.toml` only to pick
its arithmetic, `ring`, which builds with the toolchain Rust already needs; `net.rs` installs it as
the process's provider before the first connection. No certificate is ever accepted unchecked, and
there is no self-signed certificate anywhere: a server run locally is reached in plain `ws://`.

Measured on 2026-11-12, the native client in debug on the Windows machine against the deployed
server, on a home connection: welcomed, then a quick play race on the drifting autopilot at ping
34 ms (27 to 51), input to screen 33 to 37 ms, 0.0 cm corrected.

## Handshake

The server allows 2 minutes (`HANDSHAKE_TIMEOUT`) for the whole exchange. That is far more than
the exchange takes, on purpose: a web client runs its session on the page's main thread, between
frames, and the first frames of a page can hold that thread for several seconds each while the
browser compiles shaders. With 5 seconds, a page opened with `?nickname=` (now `?guest`), which connects at once,
timed out on its first load even on a desktop GPU. 20 seconds was not enough either against the
deployed site: a first visit with empty caches (Edge, ANGLE on Direct3D 11, a GTX 1050 Ti) opened
the socket 5 seconds in and then held the thread until 34 seconds, so the client only saw the
server's answer once the server had given up, and the page said it could not reach the server. A
second visit, with the shaders cached, connected in 50 ms. A phone is slower still, and a
connection that never answers only costs the server a task until the deadline.

```
Client                                          Server
  |                                                |
  |-- Hello { version, public_key, login } ------->|
  |                                                |  check version, then what can be refused
  |                                                |  without work: a guest name that is not
  |                                                |  a generated one, a bad account name, a
  |                                                |  password too short to create with
  |<------------- Challenge { nonce } -------------|  32 fresh random bytes
  |                                                |
  |-- ChallengeResponse { signature } ------------>|
  |                                                |  verify signature against public_key,
  |                                                |  then the login: the guest comes in, or
  |                                                |  the account is found, checked or created
  |<---------- Welcome { tracks, identity } -------|
```

`login` is one of three (see [Accounts](#accounts-and-guests)): `Guest { name }`, `Device`, the
account this device is signed in to, or `Password { name, password, create }`. `identity` says who
the server took the player for: a name, and whether it is a guest's.

At any of the checks the server answers `Rejected(reason)`, sends a Close frame and drops the
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

`QuickPlay`, `QueueRanked` and `LeaveQueue` are the other ways out of browsing, and
`Profile`, `Leaderboard` and `Queue` what the player is told about the ladder and the queue (see
[Quick play and ranked](ranked.md)):

```
Client                                        Server
  |<---------------- Profile(standing) ----------|  after Welcome, and on coming back from a lobby
  |<---------------- Leaderboard(best) ----------|  on arriving, and whenever it changes
  |-- QuickPlay ------------------------------->|
  |<------ JoinedLobby { lobby, car, settings } -|  the soonest race, or a new lobby
  |-- QueueRanked ----------------------------->|
  |<---------------- Queue(Some(status)) --------|  on entering it, and whenever its size changes
  |<------ JoinedLobby { lobby, car, settings } -|  when a match is found
  |<---------------- Lobby(state) ---------------|  `ranked`, then `Results { ratings, .. }`
  |<---------------- LobbyEnded -----------------|  the match is over: browsing again
```

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
- **A snapshot holds every car of the lobby**: its id, position, velocity, heading, steering, slip and drift angles and how fast the body swings, yaw
  rate, drift state (side, charge, how long the button has been held, a press still remembered),
  boost, turbo (the meter, what is left of a charge being fired, the button), race progress (see
  [Lobbies](lobbies.md#progress-laps-and-finishing)), and the drift, boost and turbo gauges the
  server works out for the HUD (see [Simulation](simulation.md#drift)), stamped with the tick it was
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

## Accounts and guests

Since v0.1.38 (protocol 26, 2026-11-12, the user's request "Fais des comptes, et login par comptes,
sauf un mode invité avec qqch qui les distingue et un nom généré obligatoire"; asked, they chose a
name and a password, no e-mail, and guests playing everything but ranked):

- **An account** is a name and a password. The name is unique whatever its case, follows the
  nickname rules below, and is what the player is seen as. There is no e-mail, so no way to get a
  forgotten password back: the account is lost with it (an e-mail can be added later).
- **The device signs in, not the session.** Every connection still proves it holds the device's
  Ed25519 key (below). Signing in with the password, or creating the account, links that key to the
  account on the server, so the device comes back with `Login::Device`, without the password, run
  after run and through every reconnection. `SignOut`, sent while browsing, unlinks it and the
  server closes the connection; a device signed out while it cannot reach the server makes itself a
  new key instead, which no account knows. A device is signed in to one account at most; an
  account to any number of devices.
- **A guest** plays under a generated name, an adjective and a noun from the game's world
  (`protocol::names`), which the dice on the first screen roll again but nobody can type: the
  server refuses a guest name that is not one of them. That, and a `guest` flag on every listed
  player (`OnlinePlayer`, `LobbyPlayer`), is what tells a guest apart; the client writes a guest's
  name dimmer and tags them GUEST in the list of players online. A guest's identity is the device
  key, as every player's was before accounts. **Ranked is for accounts**: a guest's `QueueRanked`
  is answered `JoinFailed(AccountRequired)`, and a guest is sent no profile.
- **The server keeps the accounts** in `accounts.ron` (`--accounts`, beside `ratings.ron` and for
  the same reason: deployments replace the data directory and leave these alone). Each account has
  an identity, 32 random bytes drawn when it is created, which the ladder is kept by, so an
  account's rating survives any change of device; its name; its password as an **Argon2id** hash in
  the PHC string format (the `argon2` crate's defaults: 19 MiB, 2 passes, a random 16-byte salt),
  never the password itself; and the keys of its devices. Hashing and checking take tens of
  milliseconds on purpose, on a blocking thread. A file the server cannot read stops it rather
  than being replaced, unlike the ladder: accounts cannot be started over without locking every
  player out.
- **The password crosses the wire once**, in `Hello`, inside the TLS of `wss://` in production.
  Locally, against `ws://127.0.0.1`, it is in the clear on the loopback. Nothing limits how many
  passwords a client may try, which is in keeping with how light the user wants the security of
  this alpha to be; an account name and password do not say which of the two was wrong.
- **The client remembers** the account the device is signed in to and the name last typed
  (`preferences.ron`), never the password. Bevy's text field cannot mask yet, so the password field
  draws its text in no color under a row of dots (`widgets::secret_field`); on a phone, typing goes
  through the browser's own box, which shows what is typed, and starts empty for a password.

The ladder of before accounts was kept by public keys: it is not carried over (it was empty).

## Authentication

**Every connection proves it holds the device's Ed25519 key.** The client generates a key on first
launch and keeps it; on native it lives at `%APPDATA%\Star Racer\data\identity.key`. It is a
guest's identity, and what keeps a device signed in to its account. Losing that file means
signing in again, or, for a guest, being a new guest.

The signed message is not the bare nonce but:

```
b"star-racer/auth/v1" || nonce
```

That context prefix is domain separation: a signature produced for this game is worthless in any
other protocol that might one day ask the same key to sign something, and vice versa.

Verification uses `verify_strict`, which rejects the malleable and small-order edge cases that
plain `verify` accepts. The nonce is freshly generated for each connection, so a captured
`ChallengeResponse` cannot be replayed on a later one.

In logs, a player is identified by the first 8 bytes of their identity in hex, a guest's public key or an account's identity, — enough to tell
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
| `InvalidNickname` | The name failed `is_valid_nickname`, or, for a guest, is not a generated one. |
| `AuthenticationFailed` | The signature did not verify against the announced public key. |
| `NotSignedIn` | `Login::Device` from a device no account knows, or no longer does: the client forgets the account and shows the sign-in tab. |
| `WrongPassword` | No account has this name, or its password is another. |
| `NameTaken` | Creating an account under a name another account already has, whatever its case. |
| `InvalidPassword` | Creating an account with a password shorter than 8 characters or longer than 128. |
| `AlreadyConnected` | Sent to a connection that another one of the same identity has just replaced, before it is closed: the newest connection wins (see [Lobbies](lobbies.md#on-the-server)). |

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
