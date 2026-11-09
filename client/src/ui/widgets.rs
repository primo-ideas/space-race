//! The building blocks of the menus: panels, buttons, text fields, steppers, badges and key hints.
//!
//! Widgets only say what they are; systems here style them every frame from their state (hovered,
//! pressed, focused, disabled), so a screen never has to restyle anything itself.

use bevy::ecs::query::QueryFilter;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{EditableText, LetterSpacing, TextCursorStyle, TextEdit};
use bevy::ui::auto_directional_navigation::AutoDirectionalNavigation;
use bevy::ui::{InteractionDisabled, Pressed};
use bevy::ui_widgets::{Activate, Button};

use super::navigation::LastInput;
use super::theme::{self, Fonts};
use super::touch::Touchscreen;

pub struct WidgetsPlugin;

impl Plugin for WidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchTravel>()
            .add_observer(step_on_activate)
            .add_observer(focus_on_press)
            .add_observer(type_on_touch)
            .add_observer(start_touch_travel)
            .add_observer(scroll_on_drag)
            .add_systems(
                PostUpdate,
                (
                    style_buttons,
                    style_fields,
                    style_selectables,
                    show_stepper_values,
                    show_hints,
                    hide_hints_on_touch,
                    size_for_thumbs,
                ),
            );
    }
}

/// A dark panel with a faint neon border and a shadow, to put a `Node` in.
pub fn panel() -> (BackgroundColor, BorderColor, BoxShadow) {
    (
        BackgroundColor(theme::PANEL),
        BorderColor::all(theme::PANEL_BORDER),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.7),
            px(0),
            px(10),
            px(0),
            px(30),
        ),
    )
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one main action of a screen.
    Primary,
    Secondary,
    /// Minor actions, text only until hovered.
    Quiet,
    /// The way into ranked: as prominent as the main action, and not to be mistaken for it.
    Ranked,
}

/// The text of a button, styled with it.
#[derive(Component)]
pub struct ButtonLabel;

pub fn button(fonts: &Fonts, kind: ButtonKind, label: impl Into<String>) -> impl Bundle {
    let size = match kind {
        ButtonKind::Primary | ButtonKind::Ranked => 18.0,
        ButtonKind::Secondary | ButtonKind::Quiet => 15.0,
    };
    button_of_size(fonts, kind, label, size)
}

/// A button whose label is this big.
pub fn button_of_size(
    fonts: &Fonts,
    kind: ButtonKind,
    label: impl Into<String>,
    size: f32,
) -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(px(20), px(9)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: px(1.5).all(),
            border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
            ..default()
        },
        Button,
        kind,
        Hovered::default(),
        AutoDirectionalNavigation::default(),
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        BoxShadow(Vec::new()),
        children![(
            ButtonLabel,
            Text::new(label),
            fonts.text(size, 700),
            LetterSpacing::Px(1.0),
            TextColor(theme::TEXT),
        )],
    )
}

/// A glow around whatever has the focus.
fn focus_glow(color: Color) -> BoxShadow {
    BoxShadow::new(color, px(0), px(0), px(0), px(14))
}

fn style_buttons(
    focus: Res<InputFocus>,
    mut buttons: Query<(
        Entity,
        &ButtonKind,
        &Hovered,
        Has<Pressed>,
        Has<InteractionDisabled>,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut BoxShadow,
        &Children,
    )>,
    mut labels: Query<&mut TextColor, With<ButtonLabel>>,
) {
    for (
        entity,
        kind,
        hovered,
        pressed,
        disabled,
        mut background,
        mut border,
        mut shadow,
        children,
    ) in &mut buttons
    {
        let focused = focus.get() == Some(entity) && !disabled;
        let hovered = hovered.get() && !disabled;
        let (fill, edge, text) = match kind {
            ButtonKind::Primary => {
                let fill = if disabled {
                    theme::NEON.with_alpha(0.22)
                } else if pressed {
                    Color::srgb(0.03, 0.45, 0.8)
                } else if hovered || focused {
                    Color::srgb(0.35, 0.78, 1.0)
                } else {
                    theme::NEON
                };
                let edge = if focused { theme::TEXT } else { fill };
                let text = if disabled {
                    theme::TEXT_ON_NEON.with_alpha(0.5)
                } else {
                    theme::TEXT_ON_NEON
                };
                (fill, edge, text)
            }
            ButtonKind::Ranked => {
                let fill = if disabled {
                    theme::MAGENTA.with_alpha(0.22)
                } else if pressed {
                    Color::srgb(0.65, 0.12, 0.38)
                } else if hovered || focused {
                    Color::srgb(1.0, 0.45, 0.7)
                } else {
                    theme::MAGENTA
                };
                let edge = if focused { theme::TEXT } else { fill };
                let text = if disabled {
                    theme::TEXT_ON_NEON.with_alpha(0.5)
                } else {
                    theme::TEXT_ON_NEON
                };
                (fill, edge, text)
            }
            ButtonKind::Secondary => {
                let fill = if pressed {
                    theme::NEON_FAINT
                } else if hovered || focused {
                    theme::SURFACE_HOVER
                } else {
                    theme::SURFACE
                };
                let edge = if focused {
                    theme::NEON
                } else if hovered {
                    theme::NEON_DIM
                } else {
                    theme::PANEL_BORDER
                };
                let text = if disabled {
                    theme::TEXT_FAINT
                } else {
                    theme::TEXT
                };
                (fill, edge, text)
            }
            ButtonKind::Quiet => {
                let fill = if hovered || focused || pressed {
                    theme::SURFACE_HOVER
                } else {
                    Color::NONE
                };
                let edge = if focused {
                    theme::NEON_DIM
                } else {
                    Color::NONE
                };
                let text = if disabled {
                    theme::TEXT_FAINT
                } else if hovered || focused {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                };
                (fill, edge, text)
            }
        };
        background.0 = fill;
        *border = BorderColor::all(edge);
        *shadow = if focused {
            focus_glow(theme::NEON.with_alpha(0.45))
        } else {
            BoxShadow(Vec::new())
        };
        for child in children {
            if let Ok(mut color) = labels.get_mut(*child) {
                color.0 = text;
            }
        }
    }
}

/// A single-line text input, and what it holds, for the box a touchscreen types into.
#[derive(Component)]
pub struct TextField {
    name: &'static str,
}

pub fn text_field(
    fonts: &Fonts,
    name: &'static str,
    initial: &str,
    max_chars: usize,
) -> impl Bundle {
    (
        TextField { name },
        EditableText {
            max_characters: Some(max_chars),
            allow_newlines: false,
            // Without it the field measures as tall as the space it is offered, which inflates the
            // panel around it.
            visible_lines: Some(1.0),
            ..EditableText::new(initial)
        },
        TextLayout::no_wrap(),
        fonts.text(21.0, 500),
        TextColor(theme::TEXT),
        TextCursorStyle {
            color: theme::NEON,
            selection_color: theme::NEON_DIM,
            unfocused_selection_color: Color::NONE,
            selected_text_color: None,
        },
        Node {
            flex_grow: 1.0,
            padding: UiRect::axes(px(14), px(9)),
            border: px(1.5).all(),
            border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
            ..default()
        },
        BackgroundColor(theme::SURFACE),
        BorderColor::all(theme::PANEL_BORDER),
        BoxShadow(Vec::new()),
        Hovered::default(),
        AutoDirectionalNavigation::default(),
    )
}

/// Replaces the whole text of a field.
pub fn set_text(field: &mut EditableText, text: &str) {
    field.queue_edit(TextEdit::SelectAll);
    field.queue_edit(TextEdit::Insert(text.into()));
}

/// A phone shows its keyboard only for a text box of the page itself, and the game draws its own
/// fields, so a tap on one opens the browser's own box instead, filled with what the field holds:
/// the keyboard comes up, and what is typed there replaces the field's text. Cancelling leaves it
/// as it was.
fn type_on_touch(
    click: On<Pointer<Click>>,
    touchscreen: Res<Touchscreen>,
    mut fields: Query<(&TextField, &mut EditableText)>,
) {
    if !touchscreen.0 {
        return;
    }
    let Ok((field, mut text)) = fields.get_mut(click.entity) else {
        return;
    };
    let Some(typed) = ask_for_text(field.name, &text.value().to_string()) else {
        return;
    };
    let typed: String = typed
        .trim()
        .chars()
        .take(text.max_characters.unwrap_or(usize::MAX))
        .collect();
    set_text(&mut text, &typed);
}

/// The browser's own text box, which brings up a phone's keyboard. Nothing natively, where a
/// touchscreen is only ever asked for to look at its interface, and the keyboard is there.
fn ask_for_text(name: &str, current: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?
            .prompt_with_message_and_default(name, current)
            .ok()
            .flatten()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (name, current);
        None
    }
}

fn style_fields(
    focus: Res<InputFocus>,
    mut fields: Query<
        (
            Entity,
            &Hovered,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
        ),
        With<TextField>,
    >,
) {
    for (entity, hovered, mut background, mut border, mut shadow) in &mut fields {
        let focused = focus.get() == Some(entity);
        background.0 = if focused {
            theme::NEON_FAINT
        } else {
            theme::SURFACE
        };
        *border = BorderColor::all(if focused {
            theme::NEON
        } else if hovered.get() {
            theme::NEON_DIM
        } else {
            theme::PANEL_BORDER
        });
        *shadow = if focused {
            focus_glow(theme::NEON.with_alpha(0.3))
        } else {
            BoxShadow(Vec::new())
        };
    }
}

/// Something that takes the focus without being a button: a list row, a stepper. It highlights
/// when hovered or focused, and takes the focus when clicked.
#[derive(Component, Default)]
#[require(
    Hovered,
    AutoDirectionalNavigation,
    BoxShadow,
    BackgroundColor,
    BorderColor
)]
pub struct Selectable;

fn style_selectables(
    focus: Res<InputFocus>,
    mut selectables: Query<
        (
            Entity,
            &Hovered,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
        ),
        With<Selectable>,
    >,
) {
    for (entity, hovered, mut background, mut border, mut shadow) in &mut selectables {
        let focused = focus.get() == Some(entity);
        background.0 = if focused {
            theme::NEON_FAINT
        } else if hovered.get() {
            theme::SURFACE_HOVER
        } else {
            theme::SURFACE
        };
        *border = BorderColor::all(if focused { theme::NEON } else { Color::NONE });
        *shadow = if focused {
            focus_glow(theme::NEON.with_alpha(0.25))
        } else {
            BoxShadow(Vec::new())
        };
    }
}

/// Clicking a button or a selectable gives it the focus, so the keyboard and gamepad carry on from
/// there.
fn focus_on_press(
    press: On<Pointer<Press>>,
    focusable: Query<(), Or<(With<Button>, With<Selectable>)>>,
    mut focus: ResMut<InputFocus>,
) {
    if focusable.contains(press.entity) {
        focus.set(press.entity, FocusCause::Pressed);
    }
}

/// A value picked with left and right, or with the − and + buttons beside it.
#[derive(Component)]
pub struct Stepper {
    pub value: u8,
    pub min: u8,
    pub max: u8,
    /// Past the last value comes the first one again, as in a list of choices.
    pub wrap: bool,
}

impl Stepper {
    pub fn step(&mut self, delta: i32) {
        let value = i32::from(self.value) + delta;
        let (min, max) = (i32::from(self.min), i32::from(self.max));
        self.value = if self.wrap {
            (min + (value - min).rem_euclid(max - min + 1)) as u8
        } else {
            value.clamp(min, max) as u8
        };
    }
}

/// The − or + button of the stepper `0`.
#[derive(Component)]
pub struct StepButton {
    pub stepper: Entity,
    pub delta: i32,
}

/// Shows the value of the stepper `0` as a number.
#[derive(Component)]
pub struct StepperValue(pub Entity);

/// A small square button with a single glyph, for steppers. Not reachable by navigation: the
/// stepper itself is, and left and right step it.
pub fn step_button(fonts: &Fonts, stepper: Entity, delta: i32) -> impl Bundle {
    (
        Node {
            width: px(34),
            height: px(34),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: px(1.5).all(),
            border_radius: BorderRadius::all(px(theme::RADIUS_SMALL)),
            ..default()
        },
        Button,
        ButtonKind::Secondary,
        Hovered::default(),
        StepButton { stepper, delta },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        BoxShadow(Vec::new()),
        children![(
            ButtonLabel,
            Text::new(if delta < 0 { "‹" } else { "›" }),
            fonts.text(24.0, 600),
            TextColor(theme::TEXT),
        )],
    )
}

fn step_on_activate(
    activate: On<Activate>,
    buttons: Query<&StepButton>,
    mut steppers: Query<&mut Stepper>,
) {
    if let Ok(button) = buttons.get(activate.entity)
        && let Ok(mut stepper) = steppers.get_mut(button.stepper)
    {
        stepper.step(button.delta);
    }
}

fn show_stepper_values(
    steppers: Query<&Stepper, Changed<Stepper>>,
    mut values: Query<(&StepperValue, &mut Text)>,
) {
    for (value, mut text) in &mut values {
        if let Ok(stepper) = steppers.get(value.0) {
            text.0 = stepper.value.to_string();
        }
    }
}

/// A rounded label with a colored dot, for statuses.
pub fn badge(fonts: &Fonts, text: impl Into<String>, color: Color) -> impl Bundle {
    (
        Badge,
        Node {
            padding: UiRect::axes(px(10), px(3)),
            column_gap: px(7),
            align_items: AlignItems::Center,
            border: px(1).all(),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(color.with_alpha(0.12)),
        BorderColor::all(color.with_alpha(0.45)),
        children![
            (
                BadgeDot,
                Node {
                    width: px(7),
                    height: px(7),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(color),
            ),
            (
                BadgeText,
                Text::new(text),
                fonts.text(12.5, 700),
                LetterSpacing::Px(1.0),
                TextColor(color),
            ),
        ],
    )
}

#[derive(Component)]
pub struct Badge;
#[derive(Component)]
pub struct BadgeDot;
#[derive(Component)]
pub struct BadgeText;

/// Changes a badge made by [`badge`] in place. The queries may filter more, to stay disjoint from
/// the caller's other queries.
pub fn set_badge<B: QueryFilter, D: QueryFilter, T: QueryFilter>(
    badge: Entity,
    text: &str,
    color: Color,
    badges: &mut Query<(&mut BackgroundColor, &mut BorderColor, &Children), B>,
    dots: &mut Query<&mut BackgroundColor, D>,
    texts: &mut Query<(&mut Text, &mut TextColor), T>,
) {
    let Ok((mut background, mut border, children)) = badges.get_mut(badge) else {
        return;
    };
    background.0 = color.with_alpha(0.12);
    *border = BorderColor::all(color.with_alpha(0.45));
    for child in children {
        if let Ok(mut dot) = dots.get_mut(*child) {
            dot.0 = color;
        }
        if let Ok((mut label, mut label_color)) = texts.get_mut(*child) {
            if label.0 != text {
                label.0 = text.to_owned();
            }
            label_color.0 = color;
        }
    }
}

/// A reminder of an action: the key or gamepad button, then what it does. It follows whichever
/// device the player used last.
#[derive(Component)]
pub struct Hint {
    pub keyboard: &'static str,
    pub gamepad: &'static str,
}

#[derive(Component)]
struct HintKey;

pub fn hint(
    fonts: &Fonts,
    keyboard: &'static str,
    gamepad: &'static str,
    action: &'static str,
) -> impl Bundle {
    (
        Hint { keyboard, gamepad },
        Node {
            column_gap: px(7),
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                Node {
                    padding: UiRect::axes(px(7), px(1)),
                    min_width: px(22),
                    justify_content: JustifyContent::Center,
                    border: px(1).all(),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BorderColor::all(theme::TEXT_FAINT),
                BackgroundColor(theme::SURFACE),
                children![(
                    HintKey,
                    Text::new(keyboard),
                    fonts.text(12.5, 700),
                    TextColor(theme::TEXT_DIM),
                )],
            ),
            (
                Text::new(action),
                fonts.text(14.0, 500),
                TextColor(theme::TEXT_DIM),
            ),
        ],
    )
}

/// A row of hints, spaced out. There is nothing to hint at on a touchscreen, which has no keys
/// and no buttons, so the row is never shown there.
pub fn hint_row() -> impl Bundle {
    (
        HintRow,
        Node {
            column_gap: px(22),
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

#[derive(Component)]
struct HintRow;

/// What a thumb can hit without looking twice, in interface units: about 7 mm on a phone held in
/// landscape, against the 34 a mouse pointer is happy with.
const THUMB_TARGET: f32 = 46.0;

/// On a touchscreen every button and every row that can be picked is at least a thumb high, and a
/// stepper's arrows a thumb square. Done here once, as they appear, rather than in every screen
/// that draws one.
fn size_for_thumbs(
    touchscreen: Res<Touchscreen>,
    mut buttons: Query<(&mut Node, Has<StepButton>), Or<(Added<Button>, Added<Selectable>)>>,
) {
    if !touchscreen.0 {
        return;
    }
    for (mut node, stepper) in &mut buttons {
        if stepper {
            node.width = px(THUMB_TARGET);
            node.height = px(THUMB_TARGET);
        } else {
            node.min_height = px(THUMB_TARGET);
        }
    }
}

/// How far a finger may slide between touching the screen and leaving it and still have tapped,
/// in logical pixels. Past it, the finger was scrolling a list.
const TAP_SLOP: f32 = 12.0;

/// How far the last finger to touch the screen has slid since, in logical pixels: lifted after
/// sliding a list, it has scrolled it rather than tapped the row it lay on.
#[derive(Resource, Default)]
pub struct TouchTravel(f32);

impl TouchTravel {
    /// Whether the finger just lifted touched the screen and left it rather than sliding.
    pub fn tapped(&self) -> bool {
        self.0 < TAP_SLOP
    }
}

/// A new finger on the screen has not slid anywhere yet. Pointer events bubble up through every
/// ancestor of what was touched, and this sees each step: only the first counts.
fn start_touch_travel(press: On<Pointer<Press>>, mut travel: ResMut<TouchTravel>) {
    if press.pointer_id.is_touch() && press.entity == press.original_event_target() {
        travel.0 = 0.0;
    }
}

/// A finger sliding over a list that scrolls scrolls it, the way a phone's own lists do: the
/// content follows the finger, and stops at either end. The list is whichever ancestor of what
/// the finger first touched scrolls vertically, the touched node itself included.
fn scroll_on_drag(
    drag: On<Pointer<Drag>>,
    scale: Res<UiScale>,
    ancestors: Query<&ChildOf>,
    mut lists: Query<(&Node, &ComputedNode, &mut ScrollPosition)>,
    mut travel: ResMut<TouchTravel>,
) {
    if !drag.pointer_id.is_touch() || drag.entity != drag.original_event_target() {
        return;
    }
    travel.0 = travel.0.max(drag.event.distance.length());
    let mut at = Some(drag.entity);
    while let Some(entity) = at {
        if let Ok((node, computed, mut position)) = lists.get_mut(entity)
            && node.overflow.y == OverflowAxis::Scroll
        {
            position.y = scrolled(
                position.y,
                drag.event.delta.y / scale.0.max(0.1),
                (computed.content_size.y - computed.size.y) * computed.inverse_scale_factor,
            );
            return;
        }
        at = ancestors.get(entity).ok().map(ChildOf::parent);
    }
}

/// Where a list scrolled to `position` is after a finger moves `delta` down it, both in interface
/// units, when its content overflows it by `overflow`: the content follows the finger, so a
/// finger moving down shows what is above.
fn scrolled(position: f32, delta: f32, overflow: f32) -> f32 {
    (position - delta).clamp(0.0, overflow.max(0.0))
}

fn hide_hints_on_touch(touchscreen: Res<Touchscreen>, mut rows: Query<&mut Node, Added<HintRow>>) {
    if !touchscreen.0 {
        return;
    }
    for mut node in &mut rows {
        node.display = Display::None;
    }
}

fn show_hints(
    last_input: Res<LastInput>,
    hints: Query<(&Hint, &Children)>,
    keycaps: Query<&Children, Without<Hint>>,
    mut texts: Query<&mut Text, With<HintKey>>,
    added: Query<(), Added<Hint>>,
) {
    if !last_input.is_changed() && added.is_empty() {
        return;
    }
    for (hint, children) in &hints {
        let label = match *last_input {
            LastInput::Keyboard => hint.keyboard,
            LastInput::Gamepad => hint.gamepad,
        };
        for keycap in children.iter().filter_map(|child| keycaps.get(child).ok()) {
            for key in keycap {
                if let Ok(mut text) = texts.get_mut(*key) {
                    text.0 = label.to_owned();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The content follows the finger, and stops at either end of the list.
    #[test]
    fn a_list_scrolls_with_the_finger_and_stops_at_its_ends() {
        // A finger moving up the list shows what is below.
        assert_eq!(scrolled(0.0, -30.0, 100.0), 30.0);
        assert_eq!(scrolled(30.0, 10.0, 100.0), 20.0);
        assert_eq!(scrolled(90.0, -30.0, 100.0), 100.0);
        assert_eq!(scrolled(10.0, 30.0, 100.0), 0.0);
        // A list that fits does not move.
        assert_eq!(scrolled(0.0, -30.0, -12.0), 0.0);
    }

    #[test]
    fn a_finger_that_slid_has_not_tapped() {
        assert!(TouchTravel(0.0).tapped());
        assert!(TouchTravel(TAP_SLOP - 1.0).tapped());
        assert!(!TouchTravel(TAP_SLOP).tapped());
    }
}
