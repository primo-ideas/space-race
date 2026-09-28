# Sound

Sound is **written, not recorded**: there is no audio file in the repository, and none is
downloaded. Every sound is synthesized in `client/src/sound.rs` and, for the engine,
`client/src/sound/engine.rs`, which keeps the game in the public
domain without hunting for licenses, keeps the wasm build small, and suits a game made of plain
shapes and neon lines. The one file not in the public domain stays the Saira font.

## Tones

A `Tone` is a few sine partials sharing one envelope:

| Field | What it does |
| --- | --- |
| `partials` | frequency in Hz and share of the volume, one pair per partial |
| `seconds` | how long the tone lasts |
| `attack` | seconds to full volume: short, but never zero, or the sound clicks |
| `decay` | how quickly it dies away afterward, per second |

A single partial is a plain sine; multiples of the base frequency give it a body. The envelope is
the attack ramp, then an exponential fall, then a 20 ms release so the end is silent rather than
cut. `Tone` is a Bevy asset implementing `Decodable`, so `AudioPlayer<Tone>` plays it like any
sound file, but its samples are computed one by one as they are needed (`ToneSamples`), at 44.1 kHz
in mono. Nothing is loaded, and a new sound is four numbers.

The test `a_tone_starts_and_ends_silently_without_clipping` holds the envelope to what a sound card
takes: it starts at zero, ends at zero, never leaves `[-1, 1]`, and is actually audible.

## The start lights

The first sounds are the start countdown, one per light (see [Lobbies](lobbies.md)):

- **Each amber light**: a 0.24 s blip on G4 (392 Hz), its octave underneath and almost nothing
  above, dying away fast.
- **The green one, the start**: 1.3 s on C5 (523 Hz), a fourth higher, sitting on its lower octave
  so it lands rather than beeps.

The first try was an octave higher with a 4 ms attack, and the user found it too aggressive: these
sounds want to be round and low, so they are mostly a fundamental and its octave, with an attack
slow enough (about 15 ms) to take the edge off. Changing them means changing the numbers in
`add_sounds` and rebuilding; if tuning by ear becomes frequent, they should move to a file the
client reads, the way the car tuning does.

`sim::race::start_lights` says how many lights are lit at a moment of the countdown, and the HUD and
the sound both read it, so what the player hears is what they see: both follow the race as
displayed, 50 ms behind the server, not the moment a packet arrived. A player who joins the grid
late, or whose frame skipped a light, hears only the light that just came on, never a burst of
missed ones.

## The engine

The engine of the car the camera follows is heard whenever there is one: the player's own, the
car they spectate, or theirs driving itself once it has crossed the line. It is not a tone: an
engine is a train of explosions, and so is this one.

- **Firings.** Each firing is a knock, a single sample, rung through three resonances: the body at
  110 Hz, broad and always there; the exhaust pipe at 340 Hz, narrower, so it rings longer, and
  louder under load; and the intake's rasp at 1300 Hz, which comes with the load. The firings come
  faster as the engine turns faster, 30 a second at idle and 135 at the car's top speed (a
  four-cylinder at 900 and 4,050 rpm), and more on a boost: that rate is the pitch the player hears
  climb with the speed. The resonances stay where they are, which is what makes it one engine at
  every speed rather than one note played higher or lower.
- **Irregularity.** No two firings are alike: each is up to a fifth stronger or weaker, up to 2%
  early or late, and every other one is 30% weaker, the uneven beat that makes an engine burble
  rather than buzz. Under load, each knock also carries a short hiss of noise, the gas behind it,
  dying away in 4 ms.
- **Output.** The three resonances are summed and rounded off by a `tanh`, gently: it takes the
  edge off the knocks and keeps the sound within its range whatever the resonances do. Coasting,
  the engine is at 55% of its loudness under load, and its pipe and rasp fall back.

A resonance is a band-pass biquad after Robert Bristow-Johnson's cookbook, scaled so that a knock
rings at its own strength however narrow the band, which keeps the three shares readable. A random
generator of three shifts (xorshift) gives the jitter and the hiss.

**What the game says.** Three numbers, set once a frame by `run_engine`:

| | What it is | Where it comes from |
| --- | --- | --- |
| `revs` | 0 at idle, 1 at top speed, 1.2 at the top of a boost | the car's speed over `top_speed`, and at least 0.45 with the accelerator pressed: the clutch slipping, so the engine revs on the grid and pulls away from a standstill |
| `load` | 0 coasting, 1 working | the accelerator the player presses while they drive their car; any other car, and theirs once it drives itself, has it down, as every car does in a game with no brake |
| `level` | 0 silent, 1 heard | 1 while the camera follows a car, 0 on the menus |

The engine follows them smoothly rather than jumping: the revs with the engine's inertia (10 per
second), the load almost at once (25), and the level so that the engine fades in and out over
about half a second rather than starting or stopping dead.

**How it plays.** One `EngineSound` plays from startup to the end, silent until there is a car to
hear, and the game changes how it runs instead of starting and stopping it, so it never clicks.
The numbers go from the game to the sound through three atomics (`Controls`): the game writes them
once a frame, the sound reads them at every sample, on a thread of its own natively. The synthesis
costs next to nothing: 43 seconds of it take under 0.2 s at the debug build's optimization.

The engine is kept apart from Bevy, in `sound/engine.rs`, so it can be tested and listened to on
its own. Its tests hold it silent until it runs, fading in and out rather than cutting, within its
range flat out on a boost, firing as often as its revs say, and louder under load. A scratch tool
outside the repository that includes the file as it is renders it to a WAV file, which is how its
levels were set: a spectrogram of a race start, a boost, a lift of the foot and a wall shows the
firings' harmonics climbing with the speed under resonances that stay put.

To be judged by ear, above all: the character (a combustion engine, round and low like the start
lights; a futuristic turbine would suit the look too), its loudness against the start lights,
and the pitch at top speed. There are no gears: the revs climb with the speed in one go.

## Volume

`GlobalVolume` is set once at startup from the client's `--volume` option (`?volume=` on the web),
from 0 to 1, 0.7 by default. Unattended captures pass `--volume 0`. A volume setting in the menus
is still to do.
