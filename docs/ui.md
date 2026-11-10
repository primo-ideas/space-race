# Interface

The client's screens and what they are built from, in `client/src/ui/`. Everything is Bevy UI,
with the widgets of `bevy_ui_widgets` (buttons, editable text) and Bevy's directional navigation:
no extra crate.

## Screens

```text
        ┌────── quick play, a ranked match ──────┐
        │                                        ▼
      Login ──browse lobbies──▶ Lobbies ──join──▶ Lobby (race view and HUD)
```

`screen::Screen` is a Bevy state. The first screen is the hub: everything starts from it and
ends back on it, still connected, a player who left a lobby going back to the menu they came from
(`AfterLobby`). A button pressed there says what the player wants (`Intent`), which is carried out
once connected, at once if they already are. `JoinedLobby` moves them into a lobby, any
disconnection or rejection goes back to the first screen, which says why, and so does the end of a
ranked match. Each screen's entities despawn when it is left.

- **Login** (`login.rs`): the title, a nickname field already filled in, with the nickname last
  played under or a generated one, and the two ways into a race as big as the screen allows:
  **QUICK PLAY**, and **QUEUE FOR RANKED** under it with where the player stands on the ladder
  (see [Quick play and ranked](ranked.md)). A race is one press away from the moment the game
  opens, even with a gamepad, which cannot type; Enter in the field is quick play. While queued,
  the ranked button counts the time and cancels. The dice button rolls another nickname. The last
  nickname is saved next to the identity key when the player presses a button. Under the card:
  BROWSE LOBBIES, LEADERBOARD (a dialog with the ten best, `leaderboard.rs`) and QUIT, which closes
  the game: the window's close button does the same, but a gamepad cannot reach it. QUIT has no
  shortcut, Escape and B being "back" on every other screen, where pressing them once too often
  must not close the game; and the web has no such button, since a page cannot close the tab it is
  in.
- **Lobbies** (`lobbies.rs`): every lobby, live, with its track, players and a status badge; the
  focused lobby's details beside the list (track thumbnail, what joining means right now, who is
  in). A header shows the player's nickname, HOME back to the first screen, and the button to
  create a lobby. Under the list,
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
  boost gauge over it and the drift charge as an arc of green segments on the road around the back
  of the car (see [Simulation](simulation.md#drift)), the results between races, the other players' names above their cars, a
  confirmation before leaving, and, on F3, the latency figures (see [Latency](latency.md)).
  Spectators see whom they watch and switch cars with left and right.

Generated nicknames (`nickname.rs`) are an adjective and a noun from the game's world ("Neon
Falcon", "Drifting Hexagon"); a test checks every combination is a valid nickname.

## Look

Dark translucent panels over the dark scene, with one neon blue accent shared with the track.
Status colors always mean the same thing: amber for a start to come, magenta for a race on, green for
results and finishes, orange for the player themselves (their car's color). Behind the menus, a
neon grid rushes toward the camera and fades into fog (`backdrop.rs`), under the starry sky the
races have too, which the fog leaves alone (see [Sky](sky.md)).

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
| Other nickname (first screen) | F2 | Y |
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

**The same screens, laid out for a window 400 logical pixels high** instead of 720, and allowed
to shrink to 0.55 rather than 0.75 before they stop fitting. A phone's browser in landscape
leaves the page 330 to 360 pixels of height while its address bar shows (915 by 356 on a common
Android phone, 667 by 325 on a small iPhone), so at 400 a caption is still 9 to 10 pixels high;
the first touch layout, at 440, came out about a tenth smaller. Nothing is a separate phone
screen: each one passes the two sizes it cares about through `Touchscreen::pick`, so the
difference lives in the line that draws the thing rather than in a copy of the screen. What
changes beyond sizes:

- **Everything a thumb presses is a thumb high.** `size_for_thumbs` gives every button and every
  row that can be picked a minimum height of 46 units, and a stepper's arrows 46 square, about
  7 mm on a phone, against the 34 a mouse is happy with. It does it once, as they appear, so no
  screen has to.
- **Text is typed in the browser's own box.** A phone shows its keyboard only for a text box of
  the page, and the game draws its own fields, so tapping one opens the browser's prompt, named
  after the field and filled with what it holds; what comes back replaces the field's text, and
  cancelling leaves it alone (`type_on_touch`).
- **A FULL SCREEN button**, on the login screen where the native game has QUIT and in the lobby
  browser's header, while the page is not full screen and the browser can make it so. The
  browser's bars take a seventh of the height; going full screen also turns the page to
  landscape, the only time a browser lets a page lock its orientation. It is offered rather than
  done on the first touch: taking the screen over without being asked is not the game's to do.
  An iPhone's Safari cannot put a page full screen at all and never shows the button; there,
  the game added to the home screen opens full screen and in landscape by itself (see
  [Web](web.md#on-a-phone)).
- **The notch, the rounded corners and the home bar are kept clear.** The page fills the whole
  screen, notch to notch, so the race and the menus' backdrop reach into the corners; but
  everything read or pressed stays inside the safe area the browser gives through CSS's
  `env(safe-area-inset-*)` (`ui/safe_area.rs`). An iPhone held in landscape takes about 47
  pixels from each side and 21 from the bottom, which put the left steering button half under
  the notch and the cross on the bar that sends the phone home. `index.html` pads an invisible
  element by those insets and the game reads its padding back twice a second: they change when
  the phone is turned over, which resizes nothing, and when the page goes full screen. Two kinds
  of root follow them: the HUD and the touch controls have their edges moved onto the safe
  area's (`SafeInset`), so everything pinned to their corners comes in with them; the login
  screen and the lobby browser add the insets to their padding (`SafePadding`), their
  backgrounds still filling the screen. Centered dialogs are clear of the edges already.
- **The lobby browser drops the details panel.** There is no room beside the list for it, and no
  need: a row already gives the track, the players and what the lobby is doing, and **tapping the
  row joins** (`tap_to_join`). A click only selects a row, for the panel and its JOIN button; the
  first touch layout dropped the panel and kept that, so a phone could select a lobby and never
  enter it. The players connected to the server stay under the list, on one row, and the line
  under it only exists while there is a notice to read: three rows of lobbies fit instead of two.
  In the header, CREATE LOBBY is just CREATE, under a title that already says what is being
  created, the buttons keep their one line, and it is a long nickname that gives way, cut off.
- **Lists scroll under a finger**, the content following it and stopping at either end, as a
  phone's own lists do (`scroll_on_drag`): whichever ancestor of what the finger touched scrolls
  vertically moves by the finger's travel, divided by the interface's scale. A finger that slid
  more than 12 pixels on its way was scrolling, and lifting it taps nothing (`TouchTravel`).
- **The race HUD moves out of the thumbs' way.** The speed and its gauge leave the bottom left
  corner for under the lobby's name; the frame rate leaves the bottom right for the bottom edge,
  between the two thumbs; the standings move down to make room for a **LEAVE** button, which a
  screen with no Escape key and no Start button needs.
- **No key hints anywhere.** `hint_row` hides itself on a touchscreen, and the HUD does not ask
  for its hint blocks at all.
- **The creation dialog** gives up its padding and the line explaining each setting, grows wide
  instead of tall, and puts laps, players to start and maximum players side by side, each with
  its name above its arrows: one row each with arrows a thumb can hit does not fit 400 units.

### The controls over the race

`ui/touch.rs` draws them and reads them. Two buttons in the bottom left steer, and **where the
thumb rests on them is how hard**, as a stick's angle is on a gamepad: nothing on the gap between
them, three quarters on the middle of a button, full lock from 66 units out, on its outer part,
and evenly in between. A mark follows the thumb along the buttons, and each lights up as hard as
the car turns its way. The first version told the car left or right and nothing between, and at
top speed full lock held for 0.4 s breaks the car into a drift (measured with the shipped tuning,
which grips up to 80 % of full lock and breaks away from 85 %): on a phone every turn held a
moment became a drift. Three quarters, where a thumb lands on a button, is a hard turn that
still grips; pressing outward is the deliberate way into a drift, as is the drift branch.

In the bottom right is a **cross**: touching it anywhere accelerates -- there is no brake, so the
throttle is simply wherever the thumb lands -- and sliding from its hub onto a branch calls that
branch's function without letting go of the throttle. **Drift is the first branch, straight up**:
the shortest slide for a thumb on the hub, and on Android a 14 ms tick of the vibration motor
says the thumb got there, so the eyes can stay on the road. The other three are drawn as the empty
slots they are, and light up grey rather than neon when a thumb finds them, so it is plain that
they are there and that they are waiting for the turbo and whatever comes after it.

**Neither thumb has to find its control again.** Anywhere in the bottom of the left half steers,
up to 70 units above the buttons, as hard as the place it rests at; anywhere in the bottom of the
right half accelerates, up to 70 units above the cross, and only within reach of the cross does it
choose a branch. A thumb slipping off a button while the eyes are on the road used to let go of
it. Two thumbs on the steering: the one put down last steers, as the last key pressed would.

**A wall hit is felt.** On Android, a hit that takes more than 1.5 m/s from the player's own car
knocks the phone for 4 ms per m/s, from 12 to 45 ms, at most every 0.3 s so that scraping along a
wall is not one long buzz. It is measured as the camera's shake is, from the speed the car lost
at once beyond anything driving does (see [Architecture](architecture.md#displaying-the-race)).
A boost firing buzzes for 30 ms, softer than a hit. iOS lets no page vibrate.

**A spectator picks the car to watch** with two round buttons where the thumbs rest, the left
one watching the car ahead and the right one the car behind, as left and right do on a keyboard.
They are ordinary buttons, tapped rather than held, and show only while there is more than one
car to watch and nothing of the player's own to drive.

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
- **A list only scrolls if it cannot grow.** A column's flex items are as tall as their content
  at least, unless told otherwise: the lobby list's panel grew with its rows and pushed everything
  under it off the screen, and the list inside it never overflowed. Every box between the screen
  and the list needs `min_height: px(0)`, and those that share what is left a `flex_basis` of 0.
- **A node clips its children, not its own text.** To cut a long text off, put it in a box with
  `overflow: clip` and `min_width: px(0)`, and give the text `TextLayout::no_wrap()`.
- **A full-screen root stands in front of other roots unless it is `Pickable::IGNORE`**, even
  with nothing drawn on it: the HUD's root swallowed the touches meant for the spectator's
  buttons, which are in a root of their own, and which root comes first depends on the order
  systems happened to run in.
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
| `--nickname N --quick-play` | the lobby quick play puts the player in |
| `--nickname N --ranked` | the ranked match, once the queue has found one |
| `--nickname N --lobby L` | the lobby, once its state and a car are displayed |

`--touch --window-size 844x390` shows any of them as a phone held in landscape does, which is how
the touch interface is looked at without a phone. `--safe-area 0,47,21,47` (top, right, bottom,
left, as CSS writes padding) adds an iPhone's notch, corners and home bar to it; `?safe_area=`
does the same on the web, where the insets otherwise come from the browser. The window size is taken as logical pixels,
scale factor and all, so the interface is laid out in exactly the size asked for. What a capture
cannot show is a thumb: the pads only light up under a real finger, and the gestures themselves
are covered by the tests in `ui/touch.rs`.

A lobby created by `--lobby` races on the skyway, or on the track `--track` names.

The autopilot waits for the start on screen before pressing the accelerator, so its start is
graded S, and a capture about 14 s after entering a lobby of one shows the grade over "GO!".

With `--autopilot`, `--laps 1 --min-players 1` and a `--capture-delay` of about 33 s, the capture
shows the results of a race. `--autopilot-drift` drives the same lap drifting through the turns, so
a capture mid-lap shows a car sliding with the DRIFT gauge filling. The game feel of the menus
(animation, focus, gamepad) is left for a human to judge.
