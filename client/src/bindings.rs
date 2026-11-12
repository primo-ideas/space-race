//! Which key and which gamepad button does what in a race, as the player set them in the settings
//! screen (`ui/settings.rs`), kept with their other choices (see [`crate::preferences`]).
//!
//! Every action has two keys, so the letters and the arrows both drive, and one gamepad button.
//! The left stick always steers; the D-pad buttons bound to steering are there for whoever prefers
//! it. Keys are named by their position on the keyboard, never by the letter a layout prints on
//! them, which is also how they are read.
//!
//! The defaults: W or the up arrow accelerates, A and D or the left and right arrows steer, Shift
//! or Space drifts, E or the down arrow fires the turbo; on a gamepad the right bumper
//! accelerates, the right trigger drifts and Y, the top face button, fires the turbo (the user's
//! choice, 2026-11-11). Control is never a default: with W held it closes a browser tab, which is
//! also why a page does not offer it at all.
//!
//! What is stored is the name of each key and button, and a name this version does not know is
//! read back as nothing bound rather than as a file that cannot be read: the player keeps their
//! nickname and the rest.

use bevy::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a key or a button can be bound to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Accelerate,
    SteerLeft,
    SteerRight,
    Drift,
    Turbo,
}

impl Action {
    pub const ALL: [Self; 5] = [
        Self::Accelerate,
        Self::SteerLeft,
        Self::SteerRight,
        Self::Drift,
        Self::Turbo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Accelerate => "Accelerate",
            Self::SteerLeft => "Steer left",
            Self::SteerRight => "Steer right",
            Self::Drift => "Drift",
            Self::Turbo => "Turbo",
        }
    }
}

/// One of the three things an action answers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Key,
    Alternate,
    Gamepad,
}

impl Input {
    pub const ALL: [Self; 3] = [Self::Key, Self::Alternate, Self::Gamepad];
}

/// What one action answers to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(default)]
pub struct Binding {
    pub key: Slot<KeyCode>,
    pub alternate: Slot<KeyCode>,
    pub gamepad: Slot<GamepadButton>,
}

impl Binding {
    fn new(key: KeyCode, alternate: KeyCode, gamepad: GamepadButton) -> Self {
        Self {
            key: Slot(Some(key)),
            alternate: Slot(Some(alternate)),
            gamepad: Slot(Some(gamepad)),
        }
    }

    /// The keys bound, to hand to `ButtonInput::any_pressed`.
    pub fn keys(&self) -> impl Iterator<Item = KeyCode> {
        [self.key.0, self.alternate.0].into_iter().flatten()
    }

    /// Whether this gamepad holds the button bound. A trigger counts from a third of its travel,
    /// well before Bevy calls it pressed: a drift is tapped into, and nobody taps a trigger to
    /// its stop.
    pub fn held_on(&self, gamepad: &Gamepad) -> bool {
        self.gamepad.0.is_some_and(|button| {
            gamepad.pressed(button) || gamepad.get(button).is_some_and(|value| value >= TRIGGER)
        })
    }

    /// What the settings screen and the hints call the thing bound to `input`.
    pub fn label(&self, input: Input) -> &'static str {
        match input {
            Input::Key => self.key.label(),
            Input::Alternate => self.alternate.label(),
            Input::Gamepad => self.gamepad.label(),
        }
    }
}

/// How far a trigger is pulled, from 0 to 1, when it counts as held.
const TRIGGER: f32 = 0.33;

/// Every action's binding.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Bindings {
    pub accelerate: Binding,
    pub steer_left: Binding,
    pub steer_right: Binding,
    pub drift: Binding,
    pub turbo: Binding,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            // The right bumper, above the trigger: Bevy calls the bumpers triggers and the
            // triggers "2".
            accelerate: Binding::new(
                KeyCode::KeyW,
                KeyCode::ArrowUp,
                GamepadButton::RightTrigger,
            ),
            steer_left: Binding::new(KeyCode::KeyA, KeyCode::ArrowLeft, GamepadButton::DPadLeft),
            steer_right: Binding::new(
                KeyCode::KeyD,
                KeyCode::ArrowRight,
                GamepadButton::DPadRight,
            ),
            drift: Binding::new(
                KeyCode::ShiftLeft,
                KeyCode::Space,
                GamepadButton::RightTrigger2,
            ),
            turbo: Binding::new(KeyCode::KeyE, KeyCode::ArrowDown, GamepadButton::North),
        }
    }
}

impl Bindings {
    pub fn of(&self, action: Action) -> &Binding {
        match action {
            Action::Accelerate => &self.accelerate,
            Action::SteerLeft => &self.steer_left,
            Action::SteerRight => &self.steer_right,
            Action::Drift => &self.drift,
            Action::Turbo => &self.turbo,
        }
    }

    fn of_mut(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::Accelerate => &mut self.accelerate,
            Action::SteerLeft => &mut self.steer_left,
            Action::SteerRight => &mut self.steer_right,
            Action::Drift => &mut self.drift,
            Action::Turbo => &mut self.turbo,
        }
    }

    /// Binds `key` to `action`, in its first or its second place. A key does one thing: whatever
    /// else it was bound to lets go of it.
    pub fn bind_key(&mut self, action: Action, alternate: bool, key: KeyCode) {
        for other in Action::ALL {
            let binding = self.of_mut(other);
            for slot in [&mut binding.key, &mut binding.alternate] {
                if slot.0 == Some(key) {
                    slot.0 = None;
                }
            }
        }
        let binding = self.of_mut(action);
        if alternate {
            binding.alternate.0 = Some(key);
        } else {
            binding.key.0 = Some(key);
        }
    }

    /// Binds a gamepad button to `action`, taking it from whatever else it was bound to.
    pub fn bind_button(&mut self, action: Action, button: GamepadButton) {
        for other in Action::ALL {
            let slot = &mut self.of_mut(other).gamepad;
            if slot.0 == Some(button) {
                slot.0 = None;
            }
        }
        self.of_mut(action).gamepad.0 = Some(button);
    }

    /// Leaves `action` with nothing bound to `input`.
    pub fn clear(&mut self, action: Action, input: Input) {
        let binding = self.of_mut(action);
        match input {
            Input::Key => binding.key.0 = None,
            Input::Alternate => binding.alternate.0 = None,
            Input::Gamepad => binding.gamepad.0 = None,
        }
    }

    /// The first key bound to `action`, as the hints over the race name it.
    pub fn key_label(&self, action: Action) -> &'static str {
        let binding = self.of(action);
        if binding.key.0.is_some() {
            binding.key.label()
        } else {
            binding.alternate.label()
        }
    }
}

/// A key or a button that can be bound, and what it is called: once in the file that remembers
/// it, once on the screen.
pub trait Bindable: Copy + PartialEq + 'static {
    /// Every one of them, with its stored name and its label.
    const TABLE: &'static [(Self, &'static str, &'static str)];

    fn entry(self) -> Option<&'static (Self, &'static str, &'static str)> {
        Self::TABLE.iter().find(|(candidate, _, _)| *candidate == self)
    }

    /// Whether the settings screen takes it. What the menus and the race keep for themselves is
    /// simply not in the table.
    fn bindable(self) -> bool {
        self.entry().is_some()
    }
}

/// A place for a key or a button, or nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot<T>(pub Option<T>);

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(None)
    }
}

/// What an empty place is called on the screen.
pub const UNBOUND: &str = "\u{2014}";

impl<T: Bindable> Slot<T> {
    pub fn label(&self) -> &'static str {
        self.0
            .and_then(Bindable::entry)
            .map_or(UNBOUND, |(_, _, label)| label)
    }
}

impl<T: Bindable> Serialize for Slot<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0
            .and_then(Bindable::entry)
            .map(|(_, name, _)| *name)
            .serialize(serializer)
    }
}

impl<'de, T: Bindable> Deserialize<'de> for Slot<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = Option::<String>::deserialize(deserializer)?;
        Ok(Self(name.and_then(|name| {
            T::TABLE
                .iter()
                .find(|(_, candidate, _)| *candidate == name)
                .map(|(value, _, _)| *value)
        })))
    }
}

impl Bindable for KeyCode {
    // Not here, because they are taken: Escape leaves, Enter and Tab work the menus, Backspace
    // and Delete unbind, and the function keys are the game's own shortcuts.
    const TABLE: &'static [(Self, &'static str, &'static str)] = &[
        (KeyCode::KeyA, "KeyA", "A"),
        (KeyCode::KeyB, "KeyB", "B"),
        (KeyCode::KeyC, "KeyC", "C"),
        (KeyCode::KeyD, "KeyD", "D"),
        (KeyCode::KeyE, "KeyE", "E"),
        (KeyCode::KeyF, "KeyF", "F"),
        (KeyCode::KeyG, "KeyG", "G"),
        (KeyCode::KeyH, "KeyH", "H"),
        (KeyCode::KeyI, "KeyI", "I"),
        (KeyCode::KeyJ, "KeyJ", "J"),
        (KeyCode::KeyK, "KeyK", "K"),
        (KeyCode::KeyL, "KeyL", "L"),
        (KeyCode::KeyM, "KeyM", "M"),
        (KeyCode::KeyN, "KeyN", "N"),
        (KeyCode::KeyO, "KeyO", "O"),
        (KeyCode::KeyP, "KeyP", "P"),
        (KeyCode::KeyQ, "KeyQ", "Q"),
        (KeyCode::KeyR, "KeyR", "R"),
        (KeyCode::KeyS, "KeyS", "S"),
        (KeyCode::KeyT, "KeyT", "T"),
        (KeyCode::KeyU, "KeyU", "U"),
        (KeyCode::KeyV, "KeyV", "V"),
        (KeyCode::KeyW, "KeyW", "W"),
        (KeyCode::KeyX, "KeyX", "X"),
        (KeyCode::KeyY, "KeyY", "Y"),
        (KeyCode::KeyZ, "KeyZ", "Z"),
        (KeyCode::Digit0, "Digit0", "0"),
        (KeyCode::Digit1, "Digit1", "1"),
        (KeyCode::Digit2, "Digit2", "2"),
        (KeyCode::Digit3, "Digit3", "3"),
        (KeyCode::Digit4, "Digit4", "4"),
        (KeyCode::Digit5, "Digit5", "5"),
        (KeyCode::Digit6, "Digit6", "6"),
        (KeyCode::Digit7, "Digit7", "7"),
        (KeyCode::Digit8, "Digit8", "8"),
        (KeyCode::Digit9, "Digit9", "9"),
        (KeyCode::ArrowUp, "ArrowUp", "\u{2191}"),
        (KeyCode::ArrowDown, "ArrowDown", "\u{2193}"),
        (KeyCode::ArrowLeft, "ArrowLeft", "\u{2190}"),
        (KeyCode::ArrowRight, "ArrowRight", "\u{2192}"),
        (KeyCode::Space, "Space", "Space"),
        (KeyCode::ShiftLeft, "ShiftLeft", "Shift"),
        (KeyCode::ShiftRight, "ShiftRight", "R-Shift"),
        (KeyCode::AltLeft, "AltLeft", "Alt"),
        (KeyCode::AltRight, "AltRight", "R-Alt"),
        // Control closes a browser tab with W held, so a page does not offer it.
        #[cfg(not(target_arch = "wasm32"))]
        (KeyCode::ControlLeft, "ControlLeft", "Ctrl"),
        #[cfg(not(target_arch = "wasm32"))]
        (KeyCode::ControlRight, "ControlRight", "R-Ctrl"),
        (KeyCode::CapsLock, "CapsLock", "Caps"),
        (KeyCode::Backquote, "Backquote", "`"),
        (KeyCode::Minus, "Minus", "-"),
        (KeyCode::Equal, "Equal", "="),
        (KeyCode::BracketLeft, "BracketLeft", "["),
        (KeyCode::BracketRight, "BracketRight", "]"),
        (KeyCode::Backslash, "Backslash", "\\"),
        (KeyCode::Semicolon, "Semicolon", ";"),
        (KeyCode::Quote, "Quote", "'"),
        (KeyCode::Comma, "Comma", ","),
        (KeyCode::Period, "Period", "."),
        (KeyCode::Slash, "Slash", "/"),
        (KeyCode::Numpad0, "Numpad0", "Num 0"),
        (KeyCode::Numpad1, "Numpad1", "Num 1"),
        (KeyCode::Numpad2, "Numpad2", "Num 2"),
        (KeyCode::Numpad3, "Numpad3", "Num 3"),
        (KeyCode::Numpad4, "Numpad4", "Num 4"),
        (KeyCode::Numpad5, "Numpad5", "Num 5"),
        (KeyCode::Numpad6, "Numpad6", "Num 6"),
        (KeyCode::Numpad7, "Numpad7", "Num 7"),
        (KeyCode::Numpad8, "Numpad8", "Num 8"),
        (KeyCode::Numpad9, "Numpad9", "Num 9"),
    ];
}

impl Bindable for GamepadButton {
    // Start is not here: it leaves the lobby.
    const TABLE: &'static [(Self, &'static str, &'static str)] = &[
        (GamepadButton::South, "South", "A"),
        (GamepadButton::East, "East", "B"),
        (GamepadButton::West, "West", "X"),
        (GamepadButton::North, "North", "Y"),
        (GamepadButton::LeftTrigger, "LeftBumper", "L"),
        (GamepadButton::RightTrigger, "RightBumper", "R"),
        (GamepadButton::LeftTrigger2, "LeftTrigger", "LT"),
        (GamepadButton::RightTrigger2, "RightTrigger", "RT"),
        (GamepadButton::LeftThumb, "LeftStick", "L3"),
        (GamepadButton::RightThumb, "RightStick", "R3"),
        (GamepadButton::DPadUp, "DPadUp", "D-pad \u{2191}"),
        (GamepadButton::DPadDown, "DPadDown", "D-pad \u{2193}"),
        (GamepadButton::DPadLeft, "DPadLeft", "D-pad \u{2190}"),
        (GamepadButton::DPadRight, "DPadRight", "D-pad \u{2192}"),
        (GamepadButton::Select, "Select", "Select"),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(bindings: &Bindings) -> Bindings {
        let text = ron::to_string(bindings).expect("bindings can be written");
        ron::from_str(&text).expect("bindings can be read back")
    }

    /// The user's choice of 2026-11-11: drift on the right trigger, the turbo on Y.
    #[test]
    fn by_default_the_right_trigger_drifts_and_the_top_face_button_fires_the_turbo() {
        let bindings = Bindings::default();
        assert_eq!(bindings.drift.gamepad.0, Some(GamepadButton::RightTrigger2));
        assert_eq!(bindings.turbo.gamepad.0, Some(GamepadButton::North));
        assert_eq!(
            bindings.accelerate.gamepad.0,
            Some(GamepadButton::RightTrigger)
        );
        assert_eq!(bindings.drift.label(Input::Gamepad), "RT");
        assert_eq!(bindings.turbo.label(Input::Gamepad), "Y");
    }

    #[test]
    fn control_is_never_a_default() {
        let bindings = Bindings::default();
        for action in Action::ALL {
            for key in bindings.of(action).keys() {
                assert!(
                    !matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight),
                    "{action:?}"
                );
                assert!(key.bindable(), "{action:?}");
            }
        }
    }

    #[test]
    fn bindings_survive_being_written_and_read_back() {
        let mut bindings = Bindings::default();
        bindings.bind_key(Action::Turbo, false, KeyCode::KeyQ);
        bindings.bind_button(Action::Drift, GamepadButton::West);
        bindings.clear(Action::Accelerate, Input::Alternate);
        assert_eq!(round_trip(&bindings), bindings);
    }

    /// A key does one thing: bound to another action, it leaves the first.
    #[test]
    fn a_key_bound_again_leaves_what_it_did_before() {
        let mut bindings = Bindings::default();
        bindings.bind_key(Action::Turbo, false, KeyCode::Space);
        assert_eq!(bindings.turbo.key.0, Some(KeyCode::Space));
        assert_eq!(bindings.drift.alternate.0, None);
        assert_eq!(bindings.drift.key.0, Some(KeyCode::ShiftLeft));

        bindings.bind_button(Action::Turbo, GamepadButton::RightTrigger2);
        assert_eq!(bindings.drift.gamepad.0, None);
        assert_eq!(bindings.drift.label(Input::Gamepad), UNBOUND);
    }

    /// A name from another version is nothing bound, not a file that cannot be read.
    #[test]
    fn a_name_nobody_knows_is_read_as_nothing_bound() {
        let bindings: Bindings = ron::from_str(
            r#"(drift: (key: Some("Hyperspace"), gamepad: Some("Paddle4")), turbo: (key: None))"#,
        )
        .expect("an unknown name still reads");
        assert_eq!(bindings.drift.key.0, None);
        assert_eq!(bindings.drift.gamepad.0, None);
        assert_eq!(bindings.turbo.key.0, None);
        // An action the file does not mention keeps its default.
        assert_eq!(bindings.accelerate, Bindings::default().accelerate);
    }

    #[test]
    fn what_the_game_keeps_for_itself_cannot_be_bound() {
        for key in [
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Backspace,
            KeyCode::F3,
        ] {
            assert!(!key.bindable(), "{key:?}");
        }
        assert!(!GamepadButton::Start.bindable());
    }

    #[test]
    fn every_name_is_there_once() {
        fn check<T: Bindable + std::fmt::Debug>() {
            for (index, (value, name, _)) in T::TABLE.iter().enumerate() {
                for (other, other_name, _) in &T::TABLE[index + 1..] {
                    assert!(value != other, "{value:?} twice");
                    assert_ne!(name, other_name);
                }
            }
        }
        check::<KeyCode>();
        check::<GamepadButton>();
    }
}
