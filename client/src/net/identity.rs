//! Where the client keeps what belongs to the player between runs: their Ed25519 private key,
//! created on first launch, and named slots of text for everything else, such as their menu
//! choices (see [`crate::preferences`]). Losing the key means losing the player identity.
//!
//! Native clients keep them in files of a directory, web clients in the browser's local storage.
//! `--identity` (`?identity=` on the web) picks another store, so several players can share a
//! machine.

use anyhow::{Context, Result};
use space_race_protocol::auth::{self, SigningKey};

pub use platform::IdentityStore;

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use std::fs;
    use std::path::PathBuf;

    use directories::ProjectDirs;

    use super::*;

    const KEY_FILE: &str = "identity.key";

    /// A directory holding the key file and one file per slot.
    #[derive(Clone)]
    pub struct IdentityStore(PathBuf);

    impl IdentityStore {
        pub fn in_dir(dir: PathBuf) -> Self {
            Self(dir)
        }

        /// The user data directory (`%APPDATA%\Space Race\data` on Windows).
        pub fn default_dir() -> Option<Self> {
            ProjectDirs::from("", "", "Space Race").map(|dirs| Self(dirs.data_dir().to_path_buf()))
        }

        pub fn load_or_create(&self) -> Result<SigningKey> {
            let path = self.0.join(KEY_FILE);
            if !path.exists() {
                fs::create_dir_all(&self.0)?;
                fs::write(&path, auth::new_signing_key().to_bytes())?;
            }

            let bytes = fs::read(&path)?;
            let bytes = bytes
                .try_into()
                .ok()
                .with_context(|| format!("corrupted identity file: {}", path.display()))?;
            Ok(SigningKey::from_bytes(&bytes))
        }

        /// What was written in `slot`, if anything ever was.
        pub fn read(&self, slot: &str) -> Option<String> {
            fs::read_to_string(self.0.join(slot)).ok()
        }

        pub fn write(&self, slot: &str, value: &str) -> Result<()> {
            fs::create_dir_all(&self.0)?;
            fs::write(self.0.join(slot), value)?;
            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use anyhow::anyhow;

    use super::*;

    const KEY_PREFIX: &str = "space-race/identity";

    /// Keys of the browser's local storage: the key itself, in hexadecimal, and one per slot.
    #[derive(Clone)]
    pub struct IdentityStore(String);

    impl IdentityStore {
        /// `name` picks a separate identity, to play several players from one browser.
        pub fn named(name: Option<&str>) -> Self {
            Self(match name {
                Some(name) => format!("{KEY_PREFIX}/{name}"),
                None => KEY_PREFIX.to_owned(),
            })
        }

        pub fn load_or_create(&self) -> Result<SigningKey> {
            let storage = storage()?;
            let stored = storage
                .get_item(&self.0)
                .map_err(|error| anyhow!("cannot read local storage: {error:?}"))?;

            if let Some(hex) = stored {
                let bytes = decode_hex(&hex)
                    .with_context(|| format!("corrupted identity in local storage: {}", self.0))?;
                return Ok(SigningKey::from_bytes(&bytes));
            }

            let key = auth::new_signing_key();
            storage
                .set_item(&self.0, &encode_hex(&key.to_bytes()))
                .map_err(|error| anyhow!("cannot write local storage: {error:?}"))?;
            Ok(key)
        }

        /// What was written in `slot`, if anything ever was.
        pub fn read(&self, slot: &str) -> Option<String> {
            storage().ok()?.get_item(&self.slot_key(slot)).ok()?
        }

        pub fn write(&self, slot: &str, value: &str) -> Result<()> {
            storage()?
                .set_item(&self.slot_key(slot), value)
                .map_err(|error| anyhow!("cannot write local storage: {error:?}"))
        }

        fn slot_key(&self, slot: &str) -> String {
            format!("{}/{slot}", self.0)
        }
    }

    fn storage() -> Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .context("the browser gives no access to local storage")
    }

    fn encode_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn decode_hex(hex: &str) -> Option<[u8; 32]> {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
            .collect::<Option<_>>()?;
        bytes.try_into().ok()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The store keeps the same key across runs, and a slot holds what was last written in it.
    #[test]
    fn a_store_remembers_its_key_and_its_slots() {
        let dir = std::env::temp_dir().join(format!("space-race-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = IdentityStore::in_dir(dir.clone());

        let key = store.load_or_create().expect("a key is created");
        assert_eq!(
            store
                .load_or_create()
                .expect("the key is read back")
                .to_bytes(),
            key.to_bytes()
        );

        assert_eq!(store.read("choices"), None);
        store
            .write("choices", "(laps: 5)")
            .expect("the slot is written");
        assert_eq!(store.read("choices").as_deref(), Some("(laps: 5)"));
        store
            .write("choices", "(laps: 7)")
            .expect("the slot is rewritten");
        assert_eq!(store.read("choices").as_deref(), Some("(laps: 7)"));

        std::fs::remove_dir_all(&dir).expect("the test directory goes away");
    }
}
