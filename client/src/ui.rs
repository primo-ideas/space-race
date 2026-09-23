//! The game's interface: the login screen, the lobby browser and its creation dialog, the race HUD,
//! and what they are built from.

mod backdrop;
mod create_lobby;
pub mod hud;
mod lobbies;
mod login;
mod navigation;
mod theme;
pub mod thumbnail;
pub mod touch;
mod widgets;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// The interface is laid out for a window this many logical pixels high, and scaled to the actual
/// height, within these bounds.
const DESIGN_HEIGHT: f32 = 720.0;
const MIN_SCALE: f32 = 0.75;
const MAX_SCALE: f32 = 2.5;
/// A phone in landscape is a short window held close, not a tall one across a desk. The same
/// screens are laid out for a window this high instead, so everything on them comes out larger
/// against the glass, and they may shrink further before they stop fitting at all.
const TOUCH_DESIGN_HEIGHT: f32 = 440.0;
const TOUCH_MIN_SCALE: f32 = 0.55;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            theme::ThemePlugin,
            widgets::WidgetsPlugin,
            navigation::NavigationPlugin,
            backdrop::BackdropPlugin,
            login::LoginPlugin,
            lobbies::LobbiesPlugin,
            create_lobby::CreateLobbyPlugin,
            hud::HudPlugin,
            touch::TouchPlugin,
        ))
        .add_systems(PreUpdate, scale_to_window);
    }
}

/// Keeps the interface the same size relative to the window, from a phone in landscape to a 4K
/// screen.
fn scale_to_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    touchscreen: Res<touch::Touchscreen>,
    mut scale: ResMut<UiScale>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let design = touchscreen.pick(DESIGN_HEIGHT, TOUCH_DESIGN_HEIGHT);
    let smallest = touchscreen.pick(MIN_SCALE, TOUCH_MIN_SCALE);
    let wanted = (window.height() / design).clamp(smallest, MAX_SCALE);
    if (scale.0 - wanted).abs() > 0.01 {
        scale.0 = wanted;
    }
}
