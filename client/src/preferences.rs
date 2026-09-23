//! What the player chose in the menus, kept between runs: their nickname and the settings of the
//! lobby creation dialog. They are stored beside the player's key (see
//! [`crate::net::identity`]), so `--identity` gives each player on a machine their own.
//!
//! **A generated nickname is not a choice.** It is remembered only once the player has made it
//! theirs, by editing it; a player who takes what the dice offered gets a new one next time. The
//! same goes for the lobby name, which is offered as "*nickname*'s lobby". Everything read back is
//! checked before it is used: a stored nickname that is not valid any more, or a number outside
//! what a lobby allows, falls back to the default rather than reaching the server.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use space_race_protocol::{
    LobbySettings, MAX_LAPS, MAX_LOBBY_PLAYERS, is_valid_lobby_name, is_valid_nickname,
};

use crate::net::identity::IdentityStore;

/// Where the choices are stored, next to the identity key.
const SLOT: &str = "preferences.ron";

pub const DEFAULT_LAPS: u8 = 3;
pub const DEFAULT_MIN_PLAYERS: u8 = 2;
pub const DEFAULT_MAX_PLAYERS: u8 = 8;
/// The track offered first, when the server has it.
pub const DEFAULT_TRACK: &str = "esplanade";

pub struct PreferencesPlugin {
    pub store: IdentityStore,
}

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StoredPreferences::load(self.store.clone()))
            .init_resource::<OfferedNickname>();
    }
}

/// The player's choices, as last saved.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Preferences {
    /// The nickname the player typed. None while they keep what was generated for them.
    pub nickname: Option<String>,
    /// The lobby creation dialog, as they last left it.
    pub lobby: LobbyPreferences,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LobbyPreferences {
    /// The lobby name the player typed. None while they keep the one offered.
    pub name: Option<String>,
    /// Key of the track, if the player picked one. An unknown key falls back to the default.
    pub track: Option<String>,
    pub laps: u8,
    pub min_players: u8,
    pub max_players: u8,
}

impl Default for LobbyPreferences {
    fn default() -> Self {
        Self {
            name: None,
            track: None,
            laps: DEFAULT_LAPS,
            min_players: DEFAULT_MIN_PLAYERS,
            max_players: DEFAULT_MAX_PLAYERS,
        }
    }
}

impl Preferences {
    /// The same choices, with anything a lobby would refuse replaced by the default: a file can be
    /// edited by hand, and what a past version stored may not be valid now.
    fn checked(mut self) -> Self {
        self.nickname = self.nickname.filter(|nickname| is_valid_nickname(nickname));
        self.lobby.name = self.lobby.name.filter(|name| is_valid_lobby_name(name));
        let lobby = &mut self.lobby;
        lobby.laps = lobby.laps.clamp(1, MAX_LAPS);
        lobby.max_players = lobby.max_players.clamp(1, MAX_LOBBY_PLAYERS);
        lobby.min_players = lobby.min_players.clamp(1, lobby.max_players);
        self
    }
}

impl LobbyPreferences {
    /// What the player last created a lobby with, to fill the dialog again.
    pub fn of(settings: &LobbySettings, offered_name: &str) -> Self {
        Self {
            name: (settings.name != offered_name).then(|| settings.name.clone()),
            track: Some(settings.track.clone()),
            laps: settings.laps,
            min_players: settings.min_players,
            max_players: settings.max_players,
        }
    }
}

/// The player's choices and the store they came from.
#[derive(Resource)]
pub struct StoredPreferences {
    store: IdentityStore,
    current: Preferences,
}

impl StoredPreferences {
    fn load(store: IdentityStore) -> Self {
        let current = match store.read(SLOT) {
            Some(text) => match ron::from_str::<Preferences>(&text) {
                Ok(preferences) => preferences.checked(),
                Err(error) => {
                    warn!("ignoring unreadable preferences: {error}");
                    Preferences::default()
                }
            },
            None => Preferences::default(),
        };
        Self { store, current }
    }

    pub fn get(&self) -> &Preferences {
        &self.current
    }

    /// Changes the choices and writes them out. Failing to store them is not worth interrupting a
    /// player over: they lose the memory, not the game.
    pub fn update(&mut self, change: impl FnOnce(&mut Preferences)) {
        change(&mut self.current);
        match ron::ser::to_string_pretty(&self.current, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(error) = self.store.write(SLOT, &text) {
                    warn!("cannot remember the menu choices: {error:#}");
                }
            }
            Err(error) => warn!("cannot write down the menu choices: {error}"),
        }
    }
}

/// The nickname the dice offered this run, if the player has not been given a remembered one.
/// Playing under it does not make it theirs: it is not saved.
#[derive(Resource, Default)]
pub struct OfferedNickname(pub Option<String>);

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(preferences: &Preferences) -> Preferences {
        let text = ron::ser::to_string_pretty(preferences, ron::ser::PrettyConfig::default())
            .expect("preferences can be written");
        ron::from_str::<Preferences>(&text)
            .expect("preferences can be read back")
            .checked()
    }

    #[test]
    fn choices_survive_being_written_and_read_back() {
        let preferences = Preferences {
            nickname: Some("Turbo Otter".into()),
            lobby: LobbyPreferences {
                name: Some("Friday night".into()),
                track: Some("esplanade".into()),
                laps: 5,
                min_players: 3,
                max_players: 4,
            },
        };
        assert_eq!(round_trip(&preferences), preferences);
    }

    /// A file written by hand, or by an older version, must not send a lobby the server refuses.
    #[test]
    fn stored_choices_that_are_not_valid_any_more_fall_back() {
        let broken = Preferences {
            nickname: Some(" leading space".into()),
            lobby: LobbyPreferences {
                name: Some(String::new()),
                track: Some("gone".into()),
                laps: 200,
                min_players: 12,
                max_players: 3,
            },
        }
        .checked();

        assert_eq!(broken.nickname, None);
        assert_eq!(broken.lobby.name, None);
        // An unknown track is only noticed against the server's list, in the dialog.
        assert_eq!(broken.lobby.track.as_deref(), Some("gone"));
        assert_eq!(broken.lobby.laps, MAX_LAPS);
        assert_eq!(broken.lobby.max_players, 3);
        assert_eq!(broken.lobby.min_players, 3);
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let preferences: Preferences = ron::from_str("()").expect("an empty record reads");
        assert_eq!(preferences, Preferences::default());
        assert_eq!(preferences.lobby.laps, DEFAULT_LAPS);
    }

    /// The name offered by the dialog is not a choice either.
    #[test]
    fn only_a_lobby_name_the_player_typed_is_kept() {
        let settings = |name: &str| LobbySettings {
            name: name.into(),
            track: "esplanade".into(),
            laps: 3,
            min_players: 2,
            max_players: 8,
        };
        let offered = "Turbo Otter's lobby";
        assert_eq!(LobbyPreferences::of(&settings(offered), offered).name, None);
        assert_eq!(
            LobbyPreferences::of(&settings("Duel"), offered)
                .name
                .as_deref(),
            Some("Duel")
        );
    }
}
