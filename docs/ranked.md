# Quick play and ranked

Two buttons on the first screen, the two ways to be racing within seconds of opening the game.

## Quick play

`ClientMessage::QuickPlay` asks the directory for the lobby that will race soonest. It is the
server that picks, in the one task that owns the list, so two players pressing the button at once
cannot both open a lobby of their own. Every lobby open to joining is scored
(`directory.rs`, `quick_play_score`), and ties go to the fuller one:

1. already on the grid, where joining still races;
2. waiting, and one more player away from starting;
3. showing its results, the next race moments away;
4. on its last lap.

A lobby in the middle of a race is not offered: the player would only watch it. A lobby asking for
more players than one more would make is not offered either. If nothing suits, a lobby named
*Quick Play N* opens: 3 laps, 8 places, two players to start.

**A lobby opened by quick play does not wait forever.** Two players pressing the button a minute
apart should meet, so a lone player is not sent straight into a race nobody can join: they drive
freely for 15 seconds (`Timings::quick_solo`, sent as `LobbyPhase::Waiting { solo_tick }`, and
counted down on screen), and a second player arriving starts the race at once. Past that it races
with whoever is there, and waits the same way after each race. A lobby the player created waits
for its players, as it always did.

## Ranked

### The queue

`QueueRanked` puts the player in a queue held by the directory, `LeaveQueue` takes them out, and so
does entering any lobby or losing the connection: the session holds the receiving end of a oneshot
channel, and a place whose receiver is gone counts for nothing. The player stays on the first
screen while they wait, the button counting the time and offering to cancel, the line under it
saying how many are queueing. Quick play leaves the queue; browsing the lobbies does too, since a
match would pull the player out of the list.

### Who plays whom

`matchmaking.rs`, four times a second, on a list ordered by who has waited longest. From the
longest waiting, it takes the players closest in rating, up to 4:

- A player accepts ratings within 150 points, and 15 further for every second waited, so after
  about a minute they accept anyone. It is enough for either of two players to have looked that
  far. With few players online an uneven race beats no race, and the ratings allow for it.
- **4 players start at once.** Fewer, down to 2, once the one who has waited longest has waited 15
  seconds (`Timings::gather`), which gives others time to turn up.

### The match

One race of 3 laps, on the tracks in turn, among the players the matchmaker chose: the lobby is not
listed and refuses anyone else. They are put on the grid at once, for 12 seconds rather than the 7
of a lobby (`Timings::ranked_grid`): nobody chose the moment a match was found, and a browser that
has not raced yet can hold its page for many seconds compiling the race's shaders, which must not
cost a start that counts.

**Leaving comes last**, whether by the button, the browser's back or a lost connection, or
disconnecting would be the way out of a race going badly. The order is the finishers by time, then
those still racing by how far they got, then those who left, the last to go ahead of the first.
Leaving after crossing the line costs nothing. When only one racer is left and nobody has finished
the race ends there: they have won, and need not drive three laps alone to be told so. A race
everyone has walked out of is still settled.

A client that loses its connection in the race comes back to the first screen, is told it counts
as coming last, and is not put back in the match. One that was in the queue goes back in it.

After the results (8 seconds, with each rating change beside each name) the lobby closes: every
session gets `LobbyEnded`, and the player is back on the first screen, their new rating under the
button.

### Ratings

`ranking.rs`. Elo played pairwise: a race among several players counts as one duel between every
two of them, won by whoever finished ahead, and a player's change is `K` times their average
surprise over those duels. A race of eight therefore moves a rating no more than a duel does, and
beating the favourite pays more than beating the outsider.

| | |
| --- | --- |
| Start | 1000 |
| `K` | 48 for the first 5 games, 24 afterwards |
| Odds | 400 points is ten to one |
| Floor | 0 |

A win among equals is worth 12 points once placed. Since v0.1.38 **ranked is for accounts**: the
ladder is kept by the account's identity, drawn when it was created, so the rating follows the
player from device to device, and what the leaderboard shows is the account's name. A guest asking
to queue is answered `JoinFailed(AccountRequired)`, and the first screen greys the button out for
one, saying SIGN IN TO PLAY RANKED (see [Protocol](protocol.md#accounts-and-guests)). Before
accounts, identity was the player's public key, and a player could start over with a new one.

| Rating | Tier |
| --- | --- |
| below 950 | Bronze |
| 950 to 1099 | Silver |
| 1100 to 1249 | Gold |
| 1250 to 1399 | Platinum |
| 1400 to 1599 | Diamond |
| 1600 and up | Champion |

The tiers and the number of placement games live in `protocol::ranked`, the two sides having to
agree on them. How ratings move is the server's alone. For the first 5 games the screen says
*Placement 2 / 5* rather than a tier.

### Where it is kept

The ladder is one RON file, `--ratings`, by default `ratings.ron` in the working directory: outside
the data directory, which a deployment replaces (see
[Deployment](deploy.md#what-a-deployment-replaces-and-what-it-never-touches)). It is written whole
after each match, by a task of its own so a slow disk never holds up a lobby, to a temporary file
renamed over the old one, so a crash leaves the previous ladder rather than half of one. A file
that cannot be read is moved aside as `ratings.ron.corrupt` and a new ladder starts: the game stays
up, and the old one can be repaired by hand.

The directory task owns the ladder, as it owns the lobbies. A lobby sends it what a race did to
every rating (`LobbyEvent::MatchFinished`) and applies changes rather than absolute values, so a
player who forfeited one match and is already in another loses nothing to the overlap.

The leaderboard is the ten best, sent to players on the menus when it changes
(`ServerMessage::Leaderboard`), behind the LEADERBOARD button on the first screen.
