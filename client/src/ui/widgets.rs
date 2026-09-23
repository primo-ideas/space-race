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
        app.add_observer(step_on_activate)
            .add_observer(focus_on_press)
            .add_systems(
                PostUpdate,
                (
                    style_buttons,
                    style_fields,
                    style_selectables,
                    show_stepper_values,
                    show_hints,
                    hide_hints_on_touch,
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
}

/// The text of a button, styled with it.
#[derive(Component)]
pub struct ButtonLabel;

pub fn button(fonts: &Fonts, kind: ButtonKind, label: impl Into<String>) -> impl Bundle {
    let size = match kind {
        ButtonKind::Primary => 18.0,
        ButtonKind::Secondary | ButtonKind::Quiet => 15.0,
    };
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

/// A single-line text input.
#[derive(Component)]
pub struct TextField;

pub fn text_field(fonts: &Fonts, initial: &str, max_chars: usize) -> impl Bundle {
    (
        TextField,
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
