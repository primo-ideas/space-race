//! Playing with thumbs: the controls drawn over the race, and what tells every other screen that
//! it is being touched rather than pointed at.
//!
//! A phone is held in landscape, a thumb in each bottom corner. The left one steers, on two
//! buttons rather than a stick: left and right is all the car is ever told. The right one holds
//! the cross. Touching the cross anywhere accelerates -- the game has no brake, so the throttle
//! is simply wherever the thumb lands -- and sliding from its hub onto a branch calls that
//! branch's function without ever letting go of the throttle. Drift is the first branch, straight
//! up; the other three are drawn as the empty slots they are, waiting for the turbo and whatever
//! follows it.
//!
//! The controls are hit-tested against `Touches` rather than picked as interface nodes: a thumb
//! sliding from the hub onto a branch is one gesture on one control, and no picking event ever
//! says "the finger that pressed there is now here".

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::hud::LeaveDialog;
use super::theme::{self, Fonts};
use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;

/// Between the controls and the edges of the screen.
const MARGIN: f32 = 24.0;

/// The steering buttons: two circles a thumb rocks between.
const STEER_RADIUS: f32 = 44.0;
const STEER_GAP: f32 = 14.0;
/// How far past a button's edge a finger still steers: a thumb covers more than it points at.
const STEER_SLACK: f32 = 16.0;

/// The cross: from its center to the tip of a branch, the hub in the middle, and how wide a
/// branch is drawn.
const REACH: f32 = 92.0;
const HUB_RADIUS: f32 = 34.0;
const BRANCH_WIDTH: f32 = 42.0;
/// A finger further than this from the center has left the hub for a branch.
const BRANCH_FROM: f32 = HUB_RADIUS + 11.0;
/// And further than this has left the cross altogether: the throttle closes.
const CROSS_SLACK: f32 = 22.0;

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
            .add_systems(OnEnter(Screen::Lobby), spawn_controls.run_if(touching))
            .add_systems(
                Update,
                (read_touches, style_controls)
                    .chain()
                    .in_set(TouchUpdate)
                    .after(RaceUpdate)
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

/// What the thumbs are asking for, worked out from where they rest on the screen.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TouchControls {
    left: bool,
    right: bool,
    /// A finger is on the cross, so the car accelerates.
    throttle: bool,
    /// The branch it has slid onto, if it has left the hub.
    branch: Option<Branch>,
}

impl TouchControls {
    pub fn accelerating(self) -> bool {
        self.throttle
    }

    pub fn steer(self) -> f32 {
        f32::from(self.right) - f32::from(self.left)
    }

    pub fn drifting(self) -> bool {
        self.branch == Some(Branch::DRIFT)
    }
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
    fn held(self, controls: &TouchControls) -> bool {
        match self {
            Self::Steer(false) => controls.left,
            Self::Steer(true) => controls.right,
            Self::Hub => controls.throttle,
            Self::Branch(branch) => controls.branch == Some(branch),
        }
    }

    /// Whether the pad does anything at all. An empty branch still answers a thumb that finds
    /// it, so it is plain that it is there and that it is waiting for a function.
    fn works(self) -> bool {
        !matches!(self, Self::Branch(branch) if branch != Branch::DRIFT)
    }

    /// Background, border and text, as it should look now.
    fn colors(self, held: bool) -> (Color, Color, Color) {
        match (self.works(), held) {
            (true, true) => (PAD_HELD, theme::NEON, theme::TEXT),
            (true, false) => (PAD, theme::NEON_DIM, theme::TEXT_DIM),
            (false, true) => (SLOT_HELD, theme::TEXT_FAINT, theme::TEXT_DIM),
            (false, false) => (SLOT, theme::PANEL_BORDER, theme::TEXT_FAINT),
        }
    }
}

fn spawn_controls(mut commands: Commands, fonts: Res<Fonts>) {
    let pad = |kind: Pad, node: Node, label: &'static str, size: f32| {
        let (background, border, text) = kind.colors(false);
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
    view: Res<RaceView>,
    dialog: Res<LeaveDialog>,
    mut controls: ResMut<TouchControls>,
) {
    // Nothing to drive while spectating, and nothing to drive behind the leaving question.
    let driving = view.own_car.is_some() && !dialog.open;
    let next = match windows.single() {
        Ok(window) if driving => {
            // The controls are laid out in the interface's own units, which is where the fingers
            // are brought to meet them.
            let scale = scale.0.max(0.1);
            let size = Vec2::new(window.width(), window.height()) / scale;
            fingers(touches.iter().map(|touch| touch.position() / scale), size)
        }
        _ => TouchControls::default(),
    };
    controls.set_if_neq(next);
}

/// What fingers resting at these places, in interface units on a screen of this size, are asking
/// the car to do.
fn fingers(fingers: impl Iterator<Item = Vec2>, size: Vec2) -> TouchControls {
    let left = Vec2::new(MARGIN + STEER_RADIUS, size.y - MARGIN - STEER_RADIUS);
    let right = left + Vec2::X * (2.0 * STEER_RADIUS + STEER_GAP);
    let cross = Vec2::new(size.x - MARGIN - REACH, size.y - MARGIN - REACH);

    let mut controls = TouchControls::default();
    for at in fingers {
        let steering = STEER_RADIUS + STEER_SLACK;
        controls.left |= at.distance(left) <= steering;
        controls.right |= at.distance(right) <= steering;

        let from_hub = at - cross;
        if from_hub.length() <= REACH + CROSS_SLACK {
            controls.throttle = true;
            controls.branch = branch_at(from_hub).or(controls.branch);
        }
    }
    controls
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
    mut root: Query<&mut Node, With<Controls>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone held in landscape, in interface units: what `scale_to_window` leaves a 844 by 390
    /// screen at the touch design height.
    const SCREEN: Vec2 = Vec2::new(952.0, 440.0);

    /// The middle of a control, from the corner it is anchored to.
    fn steering(right: bool) -> Vec2 {
        Vec2::new(
            MARGIN + STEER_RADIUS + f32::from(right) * (2.0 * STEER_RADIUS + STEER_GAP),
            SCREEN.y - MARGIN - STEER_RADIUS,
        )
    }

    fn hub() -> Vec2 {
        Vec2::new(SCREEN.x - MARGIN - REACH, SCREEN.y - MARGIN - REACH)
    }

    fn read(at: impl IntoIterator<Item = Vec2>) -> TouchControls {
        fingers(at.into_iter(), SCREEN)
    }

    #[test]
    fn a_thumb_anywhere_on_the_cross_accelerates() {
        let hub = hub();
        assert!(read([hub]).accelerating());
        // Out along a branch, and a little past its tip: still the throttle.
        assert!(read([hub + Vec2::new(0.0, -REACH)]).accelerating());
        assert!(read([hub + Vec2::new(REACH + CROSS_SLACK - 1.0, 0.0)]).accelerating());
        // Off the cross, and in the middle of the screen: nothing at all.
        assert!(!read([hub + Vec2::splat(REACH)]).accelerating());
        assert_eq!(read([SCREEN / 2.0]), TouchControls::default());
    }

    /// Sliding up from the hub drifts; the other three branches are found and do nothing yet.
    #[test]
    fn sliding_up_from_the_hub_drifts() {
        let hub = hub();
        assert!(!read([hub]).drifting());
        assert!(read([hub + Vec2::new(0.0, -REACH + 6.0)]).drifting());
        for slid in [
            Vec2::new(0.0, REACH - 6.0),
            Vec2::new(-REACH + 6.0, 0.0),
            Vec2::new(REACH - 6.0, 0.0),
        ] {
            let controls = read([hub + slid]);
            assert!(controls.accelerating() && !controls.drifting(), "{slid}");
        }
    }

    #[test]
    fn one_thumb_steers_while_the_other_drifts() {
        let controls = read([steering(false), hub() + Vec2::new(0.0, -REACH + 6.0)]);
        assert_eq!(controls.steer(), -1.0);
        assert!(controls.accelerating() && controls.drifting());

        assert_eq!(read([steering(true)]).steer(), 1.0);
        // Both buttons at once is straight on, as both keys are on a keyboard.
        assert_eq!(read([steering(false), steering(true)]).steer(), 0.0);
    }
}
