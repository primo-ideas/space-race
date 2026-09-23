//! Generated nicknames, offered on the login screen so nobody has to type one, least of all with a
//! gamepad.
//!
//! An adjective and a noun from the game's world: speed, light and geometry. Every combination is a
//! valid nickname.

use space_race_protocol::is_valid_nickname;

const ADJECTIVES: &[&str] = &[
    "Neon", "Turbo", "Swift", "Electric", "Silent", "Cosmic", "Rapid", "Lunar", "Solar", "Crimson",
    "Golden", "Frosty", "Wild", "Lucky", "Clever", "Brave", "Mighty", "Sly", "Hyper", "Nitro",
    "Velvet", "Chrome", "Pixel", "Retro", "Quantum", "Stellar", "Atomic", "Sonic", "Blazing",
    "Drifting", "Flying", "Glowing", "Laser", "Midnight", "Polar", "Radiant", "Shadow", "Thunder",
    "Vivid", "Zesty",
];

const NOUNS: &[&str] = &[
    "Falcon", "Comet", "Otter", "Hexagon", "Panda", "Rocket", "Fox", "Tiger", "Cube", "Prism",
    "Viper", "Meteor", "Lynx", "Pulsar", "Orbit", "Vector", "Wolf", "Hawk", "Spark", "Nova",
    "Raven", "Cobra", "Gecko", "Pyramid", "Sphere", "Photon", "Quasar", "Jaguar", "Mantis",
    "Nebula", "Octagon", "Piston", "Racer", "Rhombus", "Shark", "Squirrel", "Tangent", "Vortex",
    "Wombat", "Zebra",
];

/// A random nickname, different from `previous` so asking for another one always changes it.
pub fn generate(previous: Option<&str>) -> String {
    loop {
        let nickname = format!("{} {}", pick(ADJECTIVES), pick(NOUNS));
        if Some(nickname.as_str()) != previous {
            debug_assert!(is_valid_nickname(&nickname));
            return nickname;
        }
    }
}

fn pick(words: &[&'static str]) -> &'static str {
    let mut bytes = [0; 4];
    getrandom::fill(&mut bytes).expect("OS random generator unavailable");
    words[u32::from_le_bytes(bytes) as usize % words.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_is_a_valid_nickname() {
        for adjective in ADJECTIVES {
            for noun in NOUNS {
                let nickname = format!("{adjective} {noun}");
                assert!(is_valid_nickname(&nickname), "{nickname}");
            }
        }
    }

    #[test]
    fn another_nickname_is_always_different() {
        let first = generate(None);
        for _ in 0..50 {
            assert_ne!(generate(Some(&first)), first);
        }
    }
}
