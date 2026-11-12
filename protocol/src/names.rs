//! Generated names: what a guest plays under. A guest cannot pick a name, only roll another one, so
//! the server checks that a guest's name is one of these.
//!
//! An adjective and a noun from the game's world: speed, light and geometry. Every combination is a
//! valid nickname.

use crate::is_valid_nickname;

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

/// A random name, different from `previous` so asking for another one always changes it.
pub fn generate(previous: Option<&str>) -> String {
    loop {
        let name = format!("{} {}", pick(ADJECTIVES), pick(NOUNS));
        if Some(name.as_str()) != previous {
            debug_assert!(is_valid_nickname(&name));
            return name;
        }
    }
}

/// Whether `name` is one [`generate`] can give.
pub fn is_generated(name: &str) -> bool {
    name.split_once(' ').is_some_and(|(adjective, noun)| {
        ADJECTIVES.contains(&adjective) && NOUNS.contains(&noun)
    })
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
    fn every_combination_is_a_valid_nickname_and_is_recognized() {
        for adjective in ADJECTIVES {
            for noun in NOUNS {
                let name = format!("{adjective} {noun}");
                assert!(is_valid_nickname(&name), "{name}");
                assert!(is_generated(&name), "{name}");
            }
        }
    }

    #[test]
    fn a_chosen_name_is_not_a_generated_one() {
        for name in ["Primo", "Neon", "Neon Falcon ", "neon falcon", "Neon  Falcon", "Falcon Neon"] {
            assert!(!is_generated(name), "{name}");
        }
    }

    #[test]
    fn another_name_is_always_different() {
        let first = generate(None);
        for _ in 0..50 {
            assert_ne!(generate(Some(&first)), first);
        }
    }
}
