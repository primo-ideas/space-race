//! Playing with thumbs: the controls drawn over the race, and what tells every other screen that
//! it is being touched rather than pointed at.
//!
//! A phone is held in landscape, a thumb in each bottom corner. The left one steers on two
//! buttons, and how hard depends on where it rests: on the middle of a button the car turns hard
//! and still grips, pushed toward the button's outer edge it turns at full lock, which at speed
//! breaks it into a drift, and rolled in toward the gap between the two it turns gently. A thumb
//! cannot hold half a button down, so its place on the buttons is what a stick's angle is on a
//! gamepad. The right one holds the cross. Touching the cross anywhere accelerates -- the game has
//! no brake, so the throttle is simply wherever the thumb lands -- and sliding from its hub onto a
//! branch calls that branch's function without ever letting go of the throttle. Drift is the
//! first branch, straight up; the other three are drawn as the empty slots they are, waiting for
//! the turbo and whatever follows it.
//!
//! Neither thumb has to find its control again once it is down: anywhere in the bottom of the
//! left half steers, and anywhere in the bottom of the right half accelerates, so a thumb that
//! slips off a button while the eyes are on the road loses nothing.
//!
//! The controls are hit-tested against `Touches` rather than picked as interface nodes: a thumb
//! sliding from the hub onto a branch is one gesture on one control, and no picking event ever
//! says "the finger that pressed there is now here".

use bevy::color::Mix;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button};

use super::hud::LeaveDialog;
use super::safe_area::{SafeArea, SafeInset};
use super::theme::{self, Fonts};
use crate::race::{RaceUpdate, RaceView, Spectated};
use crate::screen::Screen;
use crate::weight::FollowedPush;

/// Between the controls and the edges of the screen.
const MARGIN: f32 = 24.0;

/// The steering buttons: two circles a thumb rocks between.
const STEER_RADIUS: f32 = 44.0;
const STEER_GAP: f32 = 14.0;
/// From the middle of the gap between the buttons to the middle of either.
const STEER_SPAN: f32 = STEER_RADIUS + STEER_GAP / 2.0;
/// Within this of the gap's middle, a thumb steers straight on.
const STEER_DEAD: f32 = 6.0;
/// From this far out, full lock: the outer part of a button. Its middle, `STEER_SPAN` out, steers
/// at three quarters, a hard turn that still grips at top speed: with the shipped tuning the car
/// breaks away into a drift from about 85 % of full lock, after 0.6 s of it. So a thumb resting
/// on a button turns, and a thumb pressed outward drifts.
const STEER_FULL: f32 = 66.0;
/// How far above the buttons a thumb still steers, and above the cross still accelerates: a
/// thumb wanders, and these corners have nothing else in them.
const ZONE_ABOVE: f32 = 70.0;
/// The mark that follows the steering thumb along the buttons.
const KNOB: f32 = 18.0;

/// The cross: from its center to the tip of a branch, the hub in the middle, and how wide a
/// branch is drawn.
const REACH: f32 = 92.0;
const HUB_RADIUS: f32 = 34.0;
const BRANCH_WIDTH: f32 = 42.0;
/// A finger further than this from the center has left the hub for a branch.
const BRANCH_FROM: f32 = HUB_RADIUS + 11.0;
/// And further than this has left the cross: it still accelerates, anywhere in the bottom of the
/// right half, but it is on no branch.
const CROSS_SLACK: f32 = 22.0;

/// How long the phone vibrates as a thumb arrives on the drift branch, in milliseconds: a tick
/// that says the slide was felt, so the eyes can stay on the road.
const DRIFT_BUZZ: u32 = 14;
/// A wall hit buzzes for as long as this, in milliseconds, for every m/s it took from the car,
/// between the shortest buzz a phone makes felt and one that still reads as a knock.
const HIT_BUZZ_PER_SPEED: f32 = 4.0;
const HIT_BUZZ_SHORTEST: f32 = 12.0;
const HIT_BUZZ_LONGEST: f32 = 45.0;
/// A hit taking less than this from the car, in m/s, is a graze, and buzzes nothing.
const HIT_BUZZ_FROM: f32 = 1.5;
/// Scraping along a wall is a hit on every frame: one buzz at most this often, in seconds.
const HIT_BUZZ_EVERY: f64 = 0.3;

/// The buttons that pick which car a spectator watches: the size of the steering buttons, under
/// the same thumbs.
const WATCH_RADIUS: f32 = STEER_RADIUS;

/// Dark enough to read the road through, solid enough to stand out over a lit wall.
const PAD: Color = Color::srgba(0.02, 0.05, 0.09, 0.72);
const PAD_HELD: Color = Color::srgba(0.05, 0.6, 1.0, 0.45);
/// A branch with nothing on it yet: fainter, and it answers a thumb in grey rather than in neon.
const SLOT: Color = Color::srgba(0.02, 0.05, 0.09, 0.45);
const SLOT_HELD: Color = Color::srgba(0.45, 0.5, 0.6, 0.3);

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Touchscreen>()
            .init_resource::<TouchControls>()
            .add_observer(go_full_screen)
            .add_systems(Update, offer_full_screen.run_if(touching))
            .add_observer(watch_another_car)
            .add_systems(
                OnEnter(Screen::Lobby),
                (spawn_controls, spawn_watch_controls).run_if(touching),
            )
            .add_systems(
                Update,
                (read_touches, style_controls, style_watch_controls)
                    .chain()
                    .in_set(TouchUpdate)
                    .after(RaceUpdate)
                    .run_if(in_state(Screen::Lobby))
                    .run_if(touching),
            )
            .add_systems(
                PostUpdate,
                buzz_on_hits
                    .run_if(in_state(Screen::Lobby))
                    .run_if(touching),
            );
    }
}

/// The thumbs are read before the input is sent, so a press reaches the server on the frame it
/// was made, carrying the tick the player saw when they made it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TouchUpdate;

/// Whether the player is on a touchscreen -- a phone or a tablet in a browser -- which decides
/// how large the interface is drawn, what the menus make room for, and whether the on-screen
/// controls exist at all. Read from the browser, or asked for with `--touch`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Touchscreen(pub bool);

impl Touchscreen {
    /// `phone` on a touchscreen and `desk` everywhere else: how a screen says what it does
    /// differently, in the line that does it rather than in a branch of its own.
    pub fn pick<T>(self, desk: T, phone: T) -> T {
        if self.0 { phone } else { desk }
    }
}

fn touching(screen: Res<Touchscreen>) -> bool {
    screen.0
}

/// A button that puts the page full screen and turns it to landscape, offered on a phone's
/// browser while the page is not full screen and the browser can make it so: the browser's own
/// bars take a seventh of a phone's height in landscape. It is offered rather than done: taking
/// the screen over without being asked is not the game's to do. An iPhone's Safari cannot put a
/// page full screen at all, and never shows it; there, the game added to the home screen opens
/// full screen by itself (see `client/web/index.html`).
#[derive(Component)]
pub struct FullScreenButton;

/// Whether a full screen button belongs on this screen at all: a touchscreen in a browser.
pub fn full_screen_wanted(touchscreen: Touchscreen) -> bool {
    touchscreen.0 && cfg!(target_arch = "wasm32")
}

fn go_full_screen(
    activate: On<bevy::ui_widgets::Activate>,
    buttons: Query<(), With<FullScreenButton>>,
) {
    if !buttons.contains(activate.entity) {
        return;
    }
    // The browser allows it for a few seconds after a touch, which the frame or two between the
    // tap and this comfortably fits.
    #[cfg(target_arch = "wasm32")]
    if let Some(page) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = page.request_fullscreen();
    }
}

/// Shows the full screen buttons while they can do something, and turns the page to landscape
/// once it is full screen, which is the only time a browser lets a page lock its orientation.
fn offer_full_screen(
    mut buttons: Query<&mut Node, With<FullScreenButton>>,
    mut locked: Local<bool>,
) {
    #[cfg(target_arch = "wasm32")]
    let (offered, full) = match web_sys::window().and_then(|window| window.document()) {
        Some(document) => {
            let full = document.fullscreen_element().is_some();
            (document.fullscreen_enabled() && !full, full)
        }
        None => (false, false),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (offered, full) = (false, false);

    if full && !*locked {
        // Awaited only so that a browser refusing it is not reported as an error: the page stays
        // as the phone is held, and portrait says to turn it.
        #[cfg(target_arch = "wasm32")]
        if let Some(Ok(lock)) = web_sys::window()
            .and_then(|window| window.screen().ok())
            .map(|screen| {
                screen
                    .orientation()
                    .lock(web_sys::OrientationLockType::Landscape)
            })
        {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(lock).await;
            });
        }
    }
    *locked = full;

    let display = if offered {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut buttons {
        if node.display != display {
            node.display = display;
        }
    }
}

/// What the thumbs are asking for, worked out from where they rest on the screen.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct TouchControls {
    /// From full left at -1 to full right at 1, as a stick.
    steer: f32,
    /// Where the steering thumb rests along the buttons, from the middle of the gap between them,
    /// if one does.
    thumb: Option<f32>,
    /// A finger is on the cross, or near enough, so the car accelerates.
    throttle: bool,
    /// The branch it has slid onto, if it has left the hub.
    branch: Option<Branch>,
}

impl TouchControls {
    pub fn accelerating(self) -> bool {
        self.throttle
    }

    pub fn steer(self) -> f32 {
        self.steer
    }

    pub fn drifting(self) -> bool {
        self.branch == Some(Branch::DRIFT)
    }
}

/// How hard a thumb resting `offset` from the middle of the gap between the buttons steers, and
/// which way: nothing on the gap, three quarters on the middle of a button, full lock on its outer
/// part.
fn steering_at(offset: f32) -> f32 {
    let amount = ((offset.abs() - STEER_DEAD) / (STEER_FULL - STEER_DEAD)).clamp(0.0, 1.0);
    amount.copysign(offset)
}

/// A branch of the cross, named by the way the thumb slides to reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Branch {
    Up,
    Left,
    Right,
    Down,
}

impl Branch {
    /// Drift is the first branch, straight up: the shortest slide for a thumb holding the hub,
    /// and the only function the game has to put on the cross so far.
    const DRIFT: Self = Self::Up;

    const ALL: [Self; 4] = [Self::Up, Self::Left, Self::Right, Self::Down];

    /// What the branch does, or nothing, which is drawn as nothing.
    fn label(self) -> &'static str {
        if self == Self::DRIFT { "DRIFT" } else { "" }
    }

    /// Where the branch lies inside the square the cross is drawn in, whose side is `2 * REACH`.
    /// Every branch runs from the center outward, and the hub covers their inner ends.
    fn node(self) -> Node {
        let across = REACH - BRANCH_WIDTH / 2.0;
        let mut node = Node {
            position_type: PositionType::Absolute,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: px(1.5).all(),
            border_radius: BorderRadius::all(px(BRANCH_WIDTH / 2.0)),
            ..default()
        };
        match self {
            // The label sits at the tip, out from under the thumb.
            Self::Up => {
                (node.width, node.height) = (px(BRANCH_WIDTH), px(REACH));
                (node.left, node.top) = (px(across), px(0));
                node.padding = UiRect::bottom(px(BRANCH_WIDTH));
            }
            Self::Down => {
                (node.width, node.height) = (px(BRANCH_WIDTH), px(REACH));
                (node.left, node.bottom) = (px(across), px(0));
                node.padding = UiRect::top(px(BRANCH_WIDTH));
            }
            Self::Left => {
                (node.width, node.height) = (px(REACH), px(BRANCH_WIDTH));
                (node.left, node.top) = (px(0), px(across));
                node.padding = UiRect::right(px(BRANCH_WIDTH));
            }
            Self::Right => {
                (node.width, node.height) = (px(REACH), px(BRANCH_WIDTH));
                (node.right, node.top) = (px(0), px(across));
                node.padding = UiRect::left(px(BRANCH_WIDTH));
            }
        }
        node
    }
}

/// Everything the thumbs touch, so it all goes at once when there is nothing to drive.
#[derive(Component)]
struct Controls;

/// One thing a thumb can be on.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Pad {
    /// A steering button, the right one flagged.
    Steer(bool),
    Hub,
    Branch(Branch),
}

impl Pad {
    /// How far the pad is held, from 0 to 1: a steering button lights up as hard as the car
    /// turns its way.
    fn held(self, controls: &TouchControls) -> f32 {
        match self {
            Self::Steer(false) => (-controls.steer).max(0.0),
            Self::Steer(true) => controls.steer.max(0.0),
            Self::Hub => f32::from(u8::from(controls.throttle)),
            Self::Branch(branch) => f32::from(u8::from(controls.branch == Some(branch))),
        }
    }

    /// Whether the pad does anything at all. An empty branch still answers a thumb that finds
    /// it, so it is plain that it is there and that it is waiting for a function.
    fn works(self) -> bool {
        !matches!(self, Self::Branch(branch) if branch != Branch::DRIFT)
    }

    /// Background, border and text, as it should look now, held this far.
    fn colors(self, held: f32) -> (Color, Color, Color) {
        let (rest, pressed) = if self.works() {
            (
                (PAD, theme::NEON_DIM, theme::TEXT_DIM),
                (PAD_HELD, theme::NEON, theme::TEXT),
            )
        } else {
            (
                (SLOT, theme::PANEL_BORDER, theme::TEXT_FAINT),
                (SLOT_HELD, theme::TEXT_FAINT, theme::TEXT_DIM),
            )
        };
        (
            rest.0.mix(&pressed.0, held),
            rest.1.mix(&pressed.1, held),
            rest.2.mix(&pressed.2, held),
        )
    }
}

/// The mark that follows the steering thumb along the buttons.
#[derive(Component)]
struct SteerKnob;

fn spawn_controls(mut commands: Commands, fonts: Res<Fonts>) {
    let pad = |kind: Pad, node: Node, label: &'static str, size: f32| {
        let (background, border, text) = kind.colors(0.0);
        (
            kind,
            node,
            BackgroundColor(background),
            BorderColor::all(border),
            // The fingers are read from `Touches`; nothing here takes part in picking.
            Pickable::IGNORE,
            children![(
                Text::new(label),
                fonts.caption(size),
                theme::CAPTION_SPACING,
                TextColor(text),
            )],
        )
    };
    let round = |diameter: f32, border: f32| Node {
        position_type: PositionType::Absolute,
        width: px(diameter),
        height: px(diameter),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        border: px(border).all(),
        border_radius: BorderRadius::MAX,
        ..default()
    };

    commands
        .spawn((
            Controls,
            DespawnOnExit(Screen::Lobby),
            Pickable::IGNORE,
            // Laid out in the safe area, so a notch never covers a steering button and a thumb on
            // the cross stays above an iPhone's home bar.
            SafeInset,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|root| {
            // Bottom left, under the left thumb: left and right.
            for (index, right) in [false, true].into_iter().enumerate() {
                let mut node = round(2.0 * STEER_RADIUS, 1.5);
                node.left = px(MARGIN + index as f32 * (2.0 * STEER_RADIUS + STEER_GAP));
                node.bottom = px(MARGIN);
                let arrow = if right { "\u{2192}" } else { "\u{2190}" };
                root.spawn(pad(Pad::Steer(right), node, arrow, 30.0));
            }
            // Over them, the mark of where the thumb rests, so its place on the buttons reads as
            // how hard the car turns.
            let mut knob = round(KNOB, 2.0);
            knob.bottom = px(MARGIN + STEER_RADIUS - KNOB / 2.0);
            knob.display = Display::None;
            root.spawn((
                SteerKnob,
                knob,
                BackgroundColor(theme::TEXT.with_alpha(0.85)),
                BorderColor::all(theme::NEON),
                BoxShadow::new(theme::NEON.with_alpha(0.6), px(0), px(0), px(0), px(10)),
                Pickable::IGNORE,
            ));

            // Bottom right, under the right thumb: the cross.
            root.spawn((
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    right: px(MARGIN),
                    bottom: px(MARGIN),
                    width: px(2.0 * REACH),
                    height: px(2.0 * REACH),
                    ..default()
                },
            ))
            .with_children(|cross| {
                for branch in Branch::ALL {
                    cross.spawn(pad(
                        Pad::Branch(branch),
                        branch.node(),
                        branch.label(),
                        13.0,
                    ));
                }
                let mut hub = round(2.0 * HUB_RADIUS, 2.0);
                (hub.left, hub.top) = (px(REACH - HUB_RADIUS), px(REACH - HUB_RADIUS));
                cross.spawn(pad(Pad::Hub, hub, "GO", 16.0));
            });
        });
}

/// Where every finger is, turned into what the car is being asked to do. A finger on nothing asks
/// for nothing: during a race there is nothing else on the screen to press.
fn read_touches(
    touches: Res<Touches>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
    safe_area: Res<SafeArea>,
    view: Res<RaceView>,
    dialog: Res<LeaveDialog>,
    mut controls: ResMut<TouchControls>,
) {
    // Nothing to drive while spectating, and nothing to drive behind the leaving question.
    let driving = view.own_car.is_some() && !dialog.open;
    let next = match windows.single() {
        Ok(window) if driving => {
            // The controls are laid out in the interface's own units, inside the safe area, which
            // is where the fingers are brought to meet them.
            let scale = scale.0.max(0.1);
            let area = safe_area.rect(Vec2::new(window.width(), window.height()), scale);
            fingers(
                touches
                    .iter()
                    .map(|touch| (touch.id(), touch.position() / scale - area.min)),
                area.size(),
            )
        }
        _ => TouchControls::default(),
    };
    if next.drifting() && !controls.drifting() {
        buzz(DRIFT_BUZZ);
    }
    controls.set_if_neq(next);
}

/// What fingers resting at these places, in interface units on a screen of this size, are asking
/// the car to do. Each finger comes with its touch's number, which grows with every new touch.
fn fingers(fingers: impl Iterator<Item = (u64, Vec2)>, size: Vec2) -> TouchControls {
    let gap = Vec2::new(
        MARGIN + STEER_RADIUS + STEER_SPAN,
        size.y - MARGIN - STEER_RADIUS,
    );
    let cross = Vec2::new(size.x - MARGIN - REACH, size.y - MARGIN - REACH);
    let steering_top = size.y - MARGIN - 2.0 * STEER_RADIUS - ZONE_ABOVE;
    let throttle_top = size.y - MARGIN - 2.0 * REACH - ZONE_ABOVE;

    let mut controls = TouchControls::default();
    // Two thumbs on the steering: the one put down last steers, as the last key pressed would.
    let mut steering: Option<(u64, f32)> = None;
    for (id, at) in fingers {
        let left_half = at.x < size.x / 2.0;
        if left_half && at.y >= steering_top {
            if steering.is_none_or(|(latest, _)| id > latest) {
                steering = Some((id, at.x - gap.x));
            }
        } else if !left_half && at.y >= throttle_top {
            controls.throttle = true;
            let from_hub = at - cross;
            if from_hub.length() <= REACH + CROSS_SLACK {
                controls.branch = branch_at(from_hub).or(controls.branch);
            }
        }
    }
    if let Some((_, offset)) = steering {
        controls.steer = steering_at(offset);
        controls.thumb = Some(offset);
    }
    controls
}

/// A short tick of the phone's vibration motor, where there is one and the browser lets the page
/// use it: Android does, iOS does not, and a desktop has nothing to vibrate.
fn buzz(milliseconds: u32) {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(milliseconds);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = milliseconds;
}

/// The branch a finger `from_hub` away from the center of the cross is on: the way it leans, once
/// it is far enough out to have left the hub. Screen coordinates, so `y` grows downward.
fn branch_at(from_hub: Vec2) -> Option<Branch> {
    if from_hub.length() < BRANCH_FROM {
        return None;
    }
    Some(if from_hub.x.abs() > from_hub.y.abs() {
        if from_hub.x < 0.0 {
            Branch::Left
        } else {
            Branch::Right
        }
    } else if from_hub.y < 0.0 {
        Branch::Up
    } else {
        Branch::Down
    })
}

fn style_controls(
    controls: Res<TouchControls>,
    view: Res<RaceView>,
    dialog: Res<LeaveDialog>,
    mut root: Query<&mut Node, (With<Controls>, Without<SteerKnob>)>,
    mut knob: Query<&mut Node, (With<SteerKnob>, Without<Controls>)>,
    mut pads: Query<(&Pad, &Children, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<&mut TextColor>,
) {
    // A spectator has no car to drive, and the leaving question wants the screen to itself.
    let shown = view.own_car.is_some() && !dialog.open;
    for mut node in &mut root {
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    if !shown {
        return;
    }

    // The mark rides along the buttons, from the outer edge of one to the outer edge of the other.
    for mut node in &mut knob {
        let (display, left) = match controls.thumb {
            Some(offset) => {
                let along = offset.clamp(-STEER_SPAN - STEER_RADIUS, STEER_SPAN + STEER_RADIUS);
                let left = MARGIN + STEER_RADIUS + STEER_SPAN + along - KNOB / 2.0;
                (Display::Flex, px(left))
            }
            None => (Display::None, node.left),
        };
        if node.display != display || node.left != left {
            node.display = display;
            node.left = left;
        }
    }

    for (pad, children, mut background, mut border) in &mut pads {
        let (fill, edge, text) = pad.colors(pad.held(&controls));
        if background.0 != fill {
            background.0 = fill;
        }
        *border = BorderColor::all(edge);
        for child in children {
            if let Ok(mut color) = labels.get_mut(*child)
                && color.0 != text
            {
                color.0 = text;
            }
        }
    }
}

/// What a spectator's thumbs have instead of the controls: a button under each, the left one
/// watching the car ahead and the right one the car behind, as left and right do on a keyboard.
#[derive(Component)]
struct WatchControls;

/// A button that watches the car this many places behind the one watched now.
#[derive(Component, Clone, Copy)]
struct WatchButton(i32);

fn spawn_watch_controls(mut commands: Commands, fonts: Res<Fonts>) {
    let watch = |step: i32, arrow: &'static str, caption: &'static str| {
        let mut node = Node {
            position_type: PositionType::Absolute,
            bottom: px(MARGIN),
            width: px(2.0 * WATCH_RADIUS),
            height: px(2.0 * WATCH_RADIUS),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: px(1.5).all(),
            border_radius: BorderRadius::MAX,
            ..default()
        };
        if step < 0 {
            node.left = px(MARGIN);
        } else {
            node.right = px(MARGIN);
        }
        (
            WatchButton(step),
            Button,
            node,
            BackgroundColor(PAD),
            BorderColor::all(theme::NEON_DIM),
            children![
                (
                    Text::new(arrow),
                    fonts.caption(30.0),
                    TextColor(theme::TEXT),
                    Pickable::IGNORE,
                ),
                (
                    Text::new(caption),
                    fonts.caption(11.0),
                    theme::CAPTION_SPACING,
                    TextColor(theme::TEXT_DIM),
                    Pickable::IGNORE,
                ),
            ],
        )
    };
    commands.spawn((
        WatchControls,
        DespawnOnExit(Screen::Lobby),
        Pickable::IGNORE,
        SafeInset,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: Display::None,
            ..default()
        },
        children![
            watch(-1, "\u{2190}", "AHEAD"),
            watch(1, "\u{2192}", "BEHIND"),
        ],
    ));
}

fn watch_another_car(
    activate: On<Activate>,
    buttons: Query<&WatchButton>,
    view: Res<RaceView>,
    mut spectated: ResMut<Spectated>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        spectated.step(&view, button.0);
    }
}

/// The spectator's buttons show while there is a car to watch and nothing of the player's own to
/// drive, and light up under the thumb.
fn style_watch_controls(
    view: Res<RaceView>,
    dialog: Res<LeaveDialog>,
    mut root: Query<&mut Node, With<WatchControls>>,
    mut buttons: Query<(Has<Pressed>, &mut BackgroundColor, &mut BorderColor), With<WatchButton>>,
) {
    let shown = view.own_car.is_none() && view.cars.len() > 1 && !dialog.open;
    let display = if shown { Display::Flex } else { Display::None };
    for mut node in &mut root {
        if node.display != display {
            node.display = display;
        }
    }
    for (pressed, mut background, mut border) in &mut buttons {
        let (fill, edge) = if pressed {
            (PAD_HELD, theme::NEON)
        } else {
            (PAD, theme::NEON_DIM)
        };
        if background.0 != fill {
            background.0 = fill;
            *border = BorderColor::all(edge);
        }
    }
}

/// A wall hit knocks the phone in the player's hands, harder the more speed it took, so a hit is
/// felt as well as seen: the eyes are on the road, and the thumbs on the glass feel nothing of it
/// otherwise. Only the player's own car, and only where the browser lets a page vibrate.
fn buzz_on_hits(
    push: Res<FollowedPush>,
    view: Res<RaceView>,
    time: Res<Time<Real>>,
    mut last: Local<f64>,
) {
    let Some(push) = push.0 else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if view.own_car.is_none() || now - *last < HIT_BUZZ_EVERY {
        return;
    }
    if let Some(milliseconds) = hit_buzz(push.impact) {
        buzz(milliseconds);
        *last = now;
    }
}

/// How long a hit that took `impact` m/s from the car buzzes, if it is more than a graze.
fn hit_buzz(impact: f32) -> Option<u32> {
    (impact >= HIT_BUZZ_FROM)
        .then(|| (impact * HIT_BUZZ_PER_SPEED).clamp(HIT_BUZZ_SHORTEST, HIT_BUZZ_LONGEST) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone held in landscape, in interface units: what `scale_to_window` leaves a 844 by 390
    /// screen at the touch design height.
    const SCREEN: Vec2 = Vec2::new(844.0 * 400.0 / 390.0, 400.0);

    /// The middle of a steering button.
    fn button(right: bool) -> Vec2 {
        gap() + Vec2::X * if right { STEER_SPAN } else { -STEER_SPAN }
    }

    /// The middle of the gap between the steering buttons.
    fn gap() -> Vec2 {
        Vec2::new(
            MARGIN + STEER_RADIUS + STEER_SPAN,
            SCREEN.y - MARGIN - STEER_RADIUS,
        )
    }

    fn hub() -> Vec2 {
        Vec2::new(SCREEN.x - MARGIN - REACH, SCREEN.y - MARGIN - REACH)
    }

    /// Fingers put down in this order.
    fn read(at: impl IntoIterator<Item = Vec2>) -> TouchControls {
        fingers(
            at.into_iter().enumerate().map(|(id, at)| (id as u64, at)),
            SCREEN,
        )
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn a_thumb_anywhere_in_the_bottom_right_accelerates() {
        let hub = hub();
        assert!(read([hub]).accelerating());
        // Out along a branch, and a little past its tip.
        assert!(read([hub + Vec2::new(0.0, -REACH)]).accelerating());
        assert!(read([hub + Vec2::new(REACH + CROSS_SLACK - 1.0, 0.0)]).accelerating());
        // Slipped off the cross toward the middle of the screen, or above it: still the throttle,
        // since nothing else is there.
        assert!(read([hub + Vec2::new(-REACH - 60.0, 0.0)]).accelerating());
        assert!(read([hub + Vec2::new(0.0, -REACH - ZONE_ABOVE + 2.0)]).accelerating());
        // Up by the standings and the LEAVE button, or in the left half: no throttle.
        assert!(!read([Vec2::new(SCREEN.x - 60.0, 30.0)]).accelerating());
        assert!(!read([Vec2::new(SCREEN.x * 0.4, SCREEN.y - 40.0)]).accelerating());
        assert_eq!(
            read([SCREEN / 2.0 - Vec2::Y * 120.0]),
            TouchControls::default()
        );
    }

    /// Sliding up from the hub drifts; the other three branches are found and do nothing yet,
    /// and a thumb off the cross is on no branch at all.
    #[test]
    fn sliding_up_from_the_hub_drifts() {
        let hub = hub();
        assert!(!read([hub]).drifting());
        assert!(read([hub + Vec2::new(0.0, -REACH + 6.0)]).drifting());
        for slid in [
            Vec2::new(0.0, REACH - 6.0),
            Vec2::new(-REACH + 6.0, 0.0),
            Vec2::new(REACH - 6.0, 0.0),
            Vec2::new(-REACH - 60.0, -REACH),
        ] {
            let controls = read([hub + slid]);
            assert!(controls.accelerating() && !controls.drifting(), "{slid}");
        }
    }

    /// Where the thumb rests is how hard the car turns: nothing on the gap, three quarters on the
    /// middle of a button, which still grips at top speed, full lock on its outer part.
    #[test]
    fn the_steering_is_graded_by_where_the_thumb_rests() {
        let gap = gap();
        assert_eq!(read([gap]).steer(), 0.0);
        assert_eq!(read([gap + Vec2::X * (STEER_DEAD - 1.0)]).steer(), 0.0);
        assert!(close(read([button(false)]).steer(), -0.75));
        assert!(close(read([button(true)]).steer(), 0.75));
        let between = read([gap + Vec2::X * STEER_SPAN / 2.0]).steer();
        assert!(between > 0.2 && between < 0.5, "{between}");
        assert_eq!(read([gap - Vec2::X * STEER_FULL]).steer(), -1.0);
        assert_eq!(read([button(true) + Vec2::X * STEER_RADIUS]).steer(), 1.0);
        // Past the buttons on either side, and above them: still steering, as hard as it goes.
        assert_eq!(read([Vec2::new(4.0, SCREEN.y - 10.0)]).steer(), -1.0);
        assert_eq!(read([Vec2::new(SCREEN.x * 0.45, gap.y)]).steer(), 1.0);
        assert!(read([button(false) - Vec2::Y * (STEER_RADIUS + ZONE_ABOVE - 2.0)]).steer() < 0.0);
        // Up by the speed, nothing.
        assert_eq!(read([Vec2::new(button(false).x, 40.0)]).steer(), 0.0);
    }

    #[test]
    fn one_thumb_steers_while_the_other_drifts() {
        let controls = read([button(false), hub() + Vec2::new(0.0, -REACH + 6.0)]);
        assert!(close(controls.steer(), -0.75));
        assert!(controls.accelerating() && controls.drifting());
    }

    /// Two fingers on the steering: the one put down last steers, as the last key pressed would,
    /// rather than the two cancelling out into a car going straight into the wall.
    #[test]
    fn the_last_thumb_down_steers() {
        assert!(close(read([button(false), button(true)]).steer(), 0.75));
        assert!(close(read([button(true), button(false)]).steer(), -0.75));
    }

    /// A graze buzzes nothing; a hit buzzes longer the more speed it took, within what a phone
    /// makes felt and what still reads as a knock.
    #[test]
    fn a_hit_buzzes_as_hard_as_it_was() {
        assert_eq!(hit_buzz(0.0), None);
        assert_eq!(hit_buzz(HIT_BUZZ_FROM - 0.1), None);
        assert_eq!(hit_buzz(HIT_BUZZ_FROM), Some(HIT_BUZZ_SHORTEST as u32));
        assert_eq!(hit_buzz(5.0), Some(20));
        assert_eq!(hit_buzz(40.0), Some(HIT_BUZZ_LONGEST as u32));
    }
}
