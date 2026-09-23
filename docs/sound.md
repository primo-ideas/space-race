# Sound

Sound is **written, not recorded**: there is no audio file in the repository, and none is
downloaded. Every sound is synthesized in `client/src/sound.rs`, which keeps the game in the public
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

## Volume

`GlobalVolume` is set once at startup from the client's `--volume` option (`?volume=` on the web),
from 0 to 1, 0.7 by default. Unattended captures pass `--volume 0`. A volume setting in the menus
is still to do.
