# Interface

The client's screens and what they are built from, in `client/src/ui/`. Everything is Bevy UI,
with the widgets of `bevy_ui_widgets` (buttons, editable text) and Bevy's directional navigation:
no extra crate.

## Screens

```text
Login ──connected──▶ Lobbies ──entered a lobby──▶ Lobby (race view and HUD)
```

`screen::Screen` is a Bevy state. The server's answers move the player forward (`Welcome`,
`JoinedLobby`), any disconnection or rejection goes back to the login screen, which says why, and
the player's own actions go back (leaving a lobby, changing nickname). Each screen's entities
despawn when it is left.

- **Login** (`login.rs`): the title, and a nickname field already filled in, with the nickname last
  played under or a generated one. Playing is one press away, even with a gamepad, which cannot
  type. The dice button rolls another nickname. The last nickname is saved next to the identity key
  when the player connects.
- **Lobbies** (`lobbies.rs`): every lobby, live, with its track, players and a status badge; the
  focused lobby's details beside the list (track thumbnail, what joining means right now, who is
  in). A header shows the player's nickname and the button to create a lobby. Under the list,
  everyone connected to the server, as a chip each: a dot in the color of where they are, their
  name (the player's own in orange), and the lobby they are in or "on the list". It is the one
  place that shows a player who is in no lobby, and its caption counts them all, including those
  past the two rows of chips it has room for.
- **Create a lobby** (`create_lobby.rs`): a dialog over the browser, with a name and four steppers
  (track, laps, players to start, maximum players), all with defaults. The browser is hidden while
  it is open, so navigation cannot wander behind it.
- **Lobby** (`hud.rs`, over the 3D race): the lobby name, the phase ("waiting for players", the
  lap), the four start lights over the grid (a row of circles on a dark pill, lighting up one a
  second: three amber that pop as they light, then the start itself, green, blazing larger with a
  ring bursting out of it, each with its own sound, see [Sound](sound.md)), the start grade (S to E) for three seconds
  after it, the lap and race time, the standings or the player list, the speed with the drift or
  boost gauge over it, the results between races, the other players' names above their cars, a
  confirmation before leaving, and, on F3, the latency figures (see [Latency](latency.md)).
  Spectators see whom they watch and switch cars with left and right.

Generated nicknames (`nickname.rs`) are an adjective and a noun from the game's world ("Neon
Falcon", "Drifting Hexagon"); a test checks every combination is a valid nickname.

## Look

Dark translucent panels over the dark scene, with one neon blue accent shared with the track.
Status colors always mean the same thing: amber for a start to come, magenta for a race on, green for
results and finishes, orange for the player themselves (their car's color). Behind the menus, a
neon grid rushes toward the camera and fades into fog (`backdrop.rs`).

The font is **Saira** (SIL Open Font License, `client/assets/fonts/OFL.txt`), a variable font with
weight and width axes: titles and big numbers use its heavy condensed italic, which gives the
motorsport feel, and the rest its regular upright. Both files are embedded in the executable with
`include_bytes!`, so no asset path can go missing, native or web. It is the one file of the
repository not in the public domain.

The interface is laid out for a 720-pixel-high window and scaled with the window height through
`UiScale`, so it keeps its proportions from a small window to a 4K screen.

Thumbnails of tracks are drawn on the CPU into an image (`thumbnail.rs`) when the track list
arrives: the road, as wide as it is at each point so a bottleneck shows, neon edges with a glow,
and the start line.

## Widgets

`widgets.rs` spawns widgets that only say what they are (a primary button, a text field, a
selectable row, a stepper); systems style them every frame from their state: hovered, pressed,
focused, disabled. Screens never restyle anything, they add or remove `InteractionDisabled`.

The HUD follows the same idea at a larger scale: each frame builds a model of what it should say,
and one pass writes it into the text entities and shows or hides blocks.

## What the menus remember

The player's choices come back next time (`client/src/preferences.rs`), stored beside their
identity key, so `--identity` gives each player on a machine their own: a file
(`preferences.ron`) on native, local storage in the browser.

**What the dice offered is not a choice.** A generated nickname is remembered only once the player
has edited it; someone who plays under what was offered gets a new one at the next launch, which is
the point of generating one. The lobby name works the same way, since it is offered as
"*nickname*'s lobby". Everything else in the creation dialog — track, laps, players to start,
maximum players — is remembered as soon as a lobby is created with it.

Anything read back is checked before it is used: a nickname that is no longer valid, a lap count
outside what a lobby allows, a track the server no longer has, all fall back to the default instead
of reaching the server. The file can be edited by hand, and an older version may have written
something this one refuses.

## Navigation

Every screen works with the mouse, the keyboard and a gamepad (`navigation.rs`):

| Action | Keyboard | Gamepad |
| --- | --- | --- |
| Move the focus | arrows, Tab / Shift+Tab | D-pad, left stick |
| Change a stepper | left / right | D-pad, left stick |
| Activate | Enter | A |
| Back, cancel | Escape | B |
| Other nickname (login) | F2 | Y |
| Create a lobby (lobbies) | C | Y |
| Leave the lobby (race) | Escape | Start |

Moving the focus uses Bevy's automatic directional navigation, which picks the nearest widget in
the direction from the layout, so no screen declares its navigation order. Tab is not "down": it
goes through every visible widget in reading order (rows from the top, each from the left) and
wraps around, so a widget sitting beside another, like the confirm button next to Cancel, is
still reached. Left and right stay
inside a focused text field. Buttons handle Enter themselves; the navigation system sends
`Activate` for A, and for Enter on anything else (a lobby row joins, a text field submits).
Clicking a widget focuses it, so the keyboard carries on from there. Key hints at the bottom of
each screen follow the device used last.

## On a touchscreen

A phone in a browser is a different machine: a short screen held close, no keyboard, no pointer,
and two thumbs already resting on the bottom corners. The client asks the browser rather than the
player -- `(pointer: coarse)` in `matchMedia`, which is true of phones and tablets and false of a
laptop that merely has a touch screen as well as a mouse -- and everything follows from the one
`Touchscreen` resource it puts that answer in. `?touch` on the web, `--touch` natively, force it
on to look at that interface anywhere.

**The same screens, laid out for a window 440 logical pixels high** instead of 720, and allowed
to shrink to 0.55 rather than 0.75 before they stop fitting. Nothing is a separate phone screen:
each one passes the two sizes it cares about through `Touchscreen::pick`, so the difference lives
in the line that draws the thing rather than in a copy of the screen. What changes beyond sizes:

- **The lobby browser drops the details panel.** There is no room beside the list for it, and no
  need: a row already gives the track, the players and what the lobby is doing, and tapping the
  row joins. The players connected to the server stay under the list.
- **The race HUD moves out of the thumbs' way.** The speed and its gauge leave the bottom left
  corner for under the lobby's name; the frame rate leaves the bottom right for the bottom edge,
  between the two thumbs; the standings move down to make room for a **LEAVE** button, which a
  screen with no Escape key and no Start button needs.
- **No key hints anywhere.** `hint_row` hides itself on a touchscreen, and the HUD does not ask
  for its hint blocks at all.
- **The creation dialog** gives up its padding and the line explaining each setting.

### The controls over the race

`ui/touch.rs` draws them and reads them. Two buttons in the bottom left steer, left and right,
which is all the car is ever told. In the bottom right is a **cross**: touching it anywhere
accelerates -- there is no brake, so the throttle is simply wherever the thumb lands -- and
sliding from its hub onto a branch calls that branch's function without letting go of the
throttle. **Drift is the first branch, straight up**: the shortest slide for a thumb on the hub.
The other three are drawn as the empty slots they are, and light up grey rather than neon when a
thumb finds them, so it is plain that they are there and that they are waiting for the turbo and
whatever comes after it.

They are hit-tested against `Touches` rather than picked as interface nodes. A thumb sliding from
the hub onto a branch is one gesture on one control, and picking has no event for "the finger
that pressed there is now here". The geometry is written once, in interface units, and both the
nodes and the hit-testing are built from it; a touch is divided by `UiScale` to meet them. They
are read before the input is sent, so a press reaches the server on the frame it was made,
carrying the tick the player saw (see [Lobbies](lobbies.md#the-start)).

## Pitfalls met

Worth knowing before changing the interface:

- **The first screen is entered before startup systems run.** Anything its `OnEnter` needs (the
  fonts) must exist when plugins are built, and the camera it expects must be looked for every
  frame, not once.
- **A minimum height on a text node inflates its parent**: the text's intrinsic size is measured
  in a way that adds to the panel's height. Put `min_height` on a box around the text.
- **An editable text without `visible_lines` measures as tall as the space it is offered**, which
  also inflates the panel around it.
- **Every `Node` has a `UiTransform`**: a query on `UiTransform` needs a marker to target one node.
- **A bundle must not contain the same component twice**, which panics at spawn. Style helpers
  return fonts only, not colors or spacing, so they can be combined freely; to change a widget's
  `Node`, insert a new one after spawning.
- **Blending is linear**: 4 % of white over black reads as a clear grey. Surfaces use about 1 %.

## Testing

`--capture <file.png>` takes a screenshot of whichever screen the other options lead to, once it is
ready (see [Architecture](architecture.md#testing)):

| Options | Screenshot |
| --- | --- |
| none | the login screen |
| `--nickname N` | the lobby list, once received |
| `--nickname N --create-dialog` | the creation dialog |
| `--nickname N --lobby L` | the lobby, once its state and a car are displayed |

`--touch --window-size 844x390` shows any of them as a phone held in landscape does, which is how
the touch interface is looked at without a phone. The window size is taken as logical pixels,
scale factor and all, so the interface is laid out in exactly the size asked for. What a capture
cannot show is a thumb: the pads only light up under a real finger, and the gestures themselves
are covered by the tests in `ui/touch.rs`.

A lobby created by `--lobby` races on the esplanade, or on the track `--track` names.

The autopilot waits for the start on screen before pressing the accelerator, so its start is
graded S, and a capture about 14 s after entering a lobby of one shows the grade over "GO!".

With `--autopilot`, `--laps 1 --min-players 1` and a `--capture-delay` of about 33 s, the capture
shows the results of a race. `--autopilot-drift` drives the same lap drifting through the turns, so
a capture mid-lap shows a car sliding with the DRIFT gauge filling. The game feel of the menus
(animation, focus, gamepad) is left for a human to judge.
