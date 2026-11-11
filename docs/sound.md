# Sound

Sound is **written, not recorded**: there is no audio file in the repository, and none is
downloaded. Every sound is synthesized in `client/src/sound.rs` and the two modules under it,
which keeps the game in the public domain without hunting for licenses, keeps the wasm build
small, and suits a game made of plain shapes and neon lines. The one file not in the public domain
stays the Saira font.

There are two instruments: the **tone**, played once, and the **drone**, which never stops.

## Tones

A `Tone` (`sound/tone.rs`) is a few sine partials and a band of noise sharing one envelope:

| Field | What it does |
| --- | --- |
| `partials` | frequency in Hz and share of the volume, one pair per partial |
| `seconds` | how long the tone lasts |
| `attack` | seconds to full volume: short, but never zero, or the sound clicks |
| `decay` | how quickly it dies away afterward, per second |
| `slide` | what the pitch is multiplied by when the tone ends, sliding there all the way: 2 rises an octave, 1 holds the note |
| `noise` | a band of noise under the same envelope: its level, where the band sits as the tone starts and as it ends, and how wide it is |

A single partial is a plain sine; multiples of the base frequency give it a body. A slide makes a
note rise or fall, and each partial keeps its place in its cycle rather than working it out from
the time, so the wave stays smooth while its pitch moves. The noise is white noise from a seeded
generator (`Hiss`), the same on every machine, heard through a filter retuned on every sample
(`Filter`, a state-variable one, kept under 6 kHz where it is stable): sweeping the band up is a
whoosh, sweeping it down is a thud's grit. The envelope is the attack ramp, then an exponential
fall, then a 20 ms release so the end is silent rather than cut.

`Tone` is a Bevy asset implementing `Decodable`, so `AudioPlayer<Tone>` plays it like any sound
file, but its samples are computed one by one as they are needed (`ToneSamples`), at 44.1 kHz in
mono. Nothing is loaded, and a new sound is a few numbers in `add_sounds`. Each is played at a
volume of its own (`Sounds::play`), so a hard hit is louder than a tap and a long boost louder
than a short one.

Changing a sound means changing its numbers and rebuilding; if tuning by ear becomes frequent,
they should move to a file the client reads, the way the car tuning does.

## The drone

`sound/drone.rs` is one stream, started with the game and never ended, that plays silence until
there is a car to hear. It has three voices, each following a number the game sets every frame
(`Drive`):

- **Thrusters.** A hum from 55 Hz standing still to 130 Hz at top speed, with two harmonics so a
  laptop or a phone can say it at all, and a thin whine seven times higher that comes in with the
  speed. A boost raises all of it, the turbo by nearly a fifth.
- **Wind.** Noise under a filter that opens with the square of the speed: nothing on the grid, a
  low rush at top speed, a roar on a boost.
- **The drift.** A narrow band of noise, scrubbing 23 times a second, while the car drifts. Its
  band rises from 1.5 to 2.8 kHz as the drift charges, so a drift worth letting go of is heard as
  well as seen.

The game writes those numbers into five atomics the audio thread reads every 64 samples, and each
voice glides to what was asked over 60 ms, so nothing crackles at 60 frames a second and a car
appearing fades in rather than clicking.

This is not the engine of v0.1.6, a knock on every firing rung through three resonances, which the
user had taken out in v0.1.8 ("We'll do better"). The car has since become a hovercraft on
thrusters, and this is a turbine under wind: nothing in it fires or knocks. It is the next
attempt, and the user's to judge. It is quiet on purpose, a fifth of full scale at most where a
tone reaches a third, since it never stops.

## What plays when

| Sound | When | What it is |
| --- | --- | --- |
| Start light | each amber light | a 0.24 s blip on G4 (392 Hz), its octave underneath |
| Start | the green light | 1.3 s on C5 (523 Hz), a fourth higher, on its lower octave |
| Lap | the player starts lap 2 and later | E5 and the fifth over it, 0.5 s |
| Finish | the player crosses the line for good | a C major chord with its octave, left to ring 1.8 s |
| Match found | landing in a ranked match | G4 and D5 together, 0.7 s, soft |
| Drift | the car breaks into a drift | 0.2 s of noise falling from 2.6 to 1.1 kHz |
| Boost | a boost fires, a drift's or the start's | 0.7 s: noise rising from 0.5 to 3.2 kHz over a note climbing an octave from A2; louder for a longer boost |
| Turbo | a charge of turbo fires | 1.3 s: the same gesture from an octave lower, wider and louder |
| Turbo charged | a charge of turbo fills | a small bell, D6 and the fifth over it |
| Wall | the car hits a wall | a knock falling an octave from 85 Hz with noise falling under it; louder the harder the hit, silent under 1.5 m/s, at most every 0.15 s |
| Focus | the focus moves in a menu | a 50 ms tick on C6, quiet |
| Press | anything is pressed in a menu | a 0.12 s click rising a fifth from C5 |

The car's sounds are worked out from the car itself, one frame against the last (`play_car`): a
drift that was not there, a boost that grew by more than 0.2 s, a turbo meter that crossed a whole
charge, and for a hit, the speed the car lost at once, which the springs of its body already
measure for the camera and the rumble (`FollowedPush`). The car heard is **the player's own as it
is predicted**, so a press is heard on the frame it is seen, not a round trip later; a spectator
hears the car they watch, and changing car plays nothing for the change.

The menus need no list of buttons: every press, by mouse, key, gamepad or finger, ends in the one
`Activate` event, which an observer answers, and the focus is one resource to watch.

## The start lights

`sim::race::start_lights` says how many lights are lit at a moment of the countdown, and the HUD and
the sound both read it, on the same clock, so what the player hears is what they see: the player's
own timeline while their car is predicted, ahead of the server, and the race as displayed, 50 ms
behind it, for a spectator. Until v0.1.20 the sound followed the displayed race while the lights
followed the prediction, and the start was heard a lead and three ticks after it was seen, about
85 ms locally and 180 ms at a ping of 100 ms, against a window of 50 ms for a perfect start. A
player who joins the grid late, or whose frame skipped a light, hears only the light that just
came on, never a burst of missed ones.

The first try at these two was an octave higher with a 4 ms attack, and the user found it too
aggressive: they want to be round and low, so they are mostly a fundamental and its octave, with
an attack slow enough (about 15 ms) to take the edge off. The sounds added since follow the same
rule where they can: nothing starts in under 3 ms, and nothing but the bell and the menu tick sits
above 1 kHz.

## Tests

- A tone starts at zero, ends at zero, never leaves `[-1, 1]` and is audible; a slide ends on the
  pitch it slides to in one smooth wave; a band of noise is brighter tuned high than low.
- Every sound of `add_sounds` is built and played out: each starts and ends silent, is heard, and
  peaks under full scale.
- The drone is silent with no car, grows from standing to racing to flat out on the turbo in a
  drift, never clips, and fades in when a car appears.
- A lap chimes from the second to the last and not at the finish; a hit is as loud as it was hard.

**Nothing here has been heard by Claude**, who can measure a sound and not listen to it: the
levels, the pitches and whether the drone is pleasant over a whole race are the user's to judge.

## Volume

`GlobalVolume` is set once at startup from the client's `--volume` option (`?volume=` on the web),
from 0 to 1, 0.7 by default. Unattended captures pass `--volume 0`. A volume setting in the menus,
and one for the drone apart from the rest, are still to do. Music is still to do.
