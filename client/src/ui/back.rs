//! The browser's back button, which on a phone is a gesture: a swipe in from the edge of the
//! screen on an iPhone and on most Android phones, where a thumb steering near the left edge
//! makes it by accident. To the browser the game is one page, so back left the game altogether,
//! race and all, for whatever page came before it.
//!
//! In a lobby the game adds a step of its own to the page's history. Going back takes that step
//! instead of leaving, and the game answers it as it answers LEAVE: it asks whether to leave the
//! lobby. Staying puts the step back, so the next back asks again; leaving takes the player to
//! the lobby list, where back leaves the page as it always did. A player who leaves through the
//! button has the step taken out of the history, so it is never one more press of back for
//! nothing.
//!
//! Web only: a native window has no back button.

use bevy::prelude::*;

pub struct BackPlugin;

impl Plugin for BackPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::screen::Screen;
            app.init_resource::<web::Step>()
                .add_systems(OnEnter(Screen::Lobby), web::add_step)
                .add_systems(OnExit(Screen::Lobby), web::take_step_out)
                .add_systems(
                    Update,
                    web::answer_back
                        .after(crate::race::RaceUpdate)
                        .run_if(in_state(Screen::Lobby)),
                );
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = app;
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use bevy::prelude::*;
    use web_sys::wasm_bindgen::JsValue;

    use crate::ui::hud::LeaveDialog;

    /// What the game's step in the history holds, so it can tell it from any other.
    const STEP: &str = "star-racer:lobby";

    /// Whether the game's step is in the history, as far as the game knows.
    #[derive(Resource, Default)]
    pub struct Step(bool);

    fn history() -> Option<web_sys::History> {
        web_sys::window()?.history().ok()
    }

    /// Whether the page is on the game's step: going back from it has not happened yet.
    fn on_step(history: &web_sys::History) -> bool {
        history
            .state()
            .ok()
            .and_then(|state| state.as_string())
            .is_some_and(|state| state == STEP)
    }

    fn push(step: &mut Step) {
        if let Some(history) = history() {
            // The page may be on the step already, a reload keeping its place in the history.
            // Otherwise it is added at the same address, so nothing reloads.
            step.0 = on_step(&history) || history.push_state(&JsValue::from_str(STEP), "").is_ok();
        }
    }

    pub fn add_step(mut step: ResMut<Step>) {
        push(&mut step);
    }

    /// Back has taken the game's step: ask whether to leave, as LEAVE does. Once the question is
    /// answered by staying, the step goes back in, so the next back asks again.
    pub fn answer_back(mut step: ResMut<Step>, mut dialog: ResMut<LeaveDialog>) {
        let Some(history) = history() else {
            return;
        };
        if step.0 && !on_step(&history) {
            step.0 = false;
            dialog.open = true;
        } else if !step.0 && !dialog.open {
            push(&mut step);
        }
    }

    /// Leaving the lobby by any other way than back: the step comes out of the history, which
    /// takes the page back to where it was before it, the lobby list.
    pub fn take_step_out(mut step: ResMut<Step>) {
        if let Some(history) = history()
            && on_step(&history)
        {
            let _ = history.back();
        }
        step.0 = false;
    }
}
