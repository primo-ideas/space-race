//! Player accounts: a name and a password, and the devices signed in to them.
//!
//! An account is created with a name no other account has, whatever its case, and a password,
//! kept only as an Argon2 hash. The device that signed in is remembered by its public key, the one
//! it proves it holds on every connection, so it comes back signed in without the password until
//! it signs out. A device is signed in to one account at most.
//!
//! An account's player identity, which the ladder is kept by, is a random number drawn when the
//! account is created: names never change hands, and devices come and go.
//!
//! Kept in a RON file beside the ladder, outside the data directory a deployment replaces, and
//! written on a task of its own like the ladder (see [`crate::ranking`]).

use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use serde::{Deserialize, Serialize};
use star_racer_protocol::auth::{self, PublicKey};
use star_racer_protocol::{RejectReason, is_valid_nickname, is_valid_password};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::ranking::{hex, player_id, spawn_writer};
use crate::session::PlayerId;

/// An account as the file keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Account {
    /// The player identity, in hexadecimal.
    id: String,
    name: String,
    /// The password's Argon2 hash, in the PHC string format: algorithm, parameters and salt
    /// included.
    password: String,
    /// The public keys of the devices signed in to it, in hexadecimal.
    devices: Vec<String>,
}

/// Every account, shared by every connection.
#[derive(Clone)]
pub struct Accounts(Arc<Mutex<Store>>);

struct Store {
    accounts: Vec<Account>,
    /// Hands the file's new contents to the task that writes it. `None` keeps accounts in memory.
    file: Option<mpsc::UnboundedSender<String>>,
}

/// A player signed in to their account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub id: PlayerId,
    pub name: String,
}

impl Accounts {
    /// Accounts that are never saved, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::new(Vec::new(), None)
    }

    /// Reads the accounts from `path`, and keeps them there from now on. No file is no account. A
    /// file that cannot be understood stops the server rather than being replaced: unlike the
    /// ladder, accounts cannot be started over without locking every player out.
    pub fn load(path: PathBuf) -> Result<Self> {
        let accounts = match std::fs::read_to_string(&path) {
            Ok(text) => ron::from_str::<Vec<Account>>(&text)
                .with_context(|| format!("reading the accounts in {}", path.display()))?,
            Err(problem) if problem.kind() == ErrorKind::NotFound => Vec::new(),
            Err(problem) => {
                return Err(problem).with_context(|| format!("reading {}", path.display()));
            }
        };
        let accounts: Vec<Account> = accounts
            .into_iter()
            .filter(|account| {
                let fine = player_id(&account.id).is_some();
                if !fine {
                    warn!(name = %account.name, "skipping an account with a bad identity");
                }
                fine
            })
            .collect();
        info!(file = %path.display(), accounts = accounts.len(), "accounts loaded");
        Ok(Self::new(accounts, Some(spawn_writer(path, "accounts"))))
    }

    fn new(accounts: Vec<Account>, file: Option<mpsc::UnboundedSender<String>>) -> Self {
        Self(Arc::new(Mutex::new(Store { accounts, file })))
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.0.lock().expect("accounts poisoned")
    }

    /// The account `device` is signed in to, if any.
    pub fn device(&self, device: &PublicKey) -> Option<SignedIn> {
        let device = hex(device);
        let store = self.store();
        store
            .accounts
            .iter()
            .find(|account| account.devices.contains(&device))
            .and_then(signed_in)
    }

    /// Creates an account, and signs `device` in to it.
    pub async fn create(
        &self,
        name: &str,
        password: &str,
        device: &PublicKey,
    ) -> Result<SignedIn, RejectReason> {
        if !is_valid_nickname(name) {
            return Err(RejectReason::InvalidNickname);
        }
        if !is_valid_password(password) {
            return Err(RejectReason::InvalidPassword);
        }
        if self.store().find(name).is_some() {
            return Err(RejectReason::NameTaken);
        }
        // Hashing takes tens of milliseconds on purpose: away from the async threads.
        let password = password.to_owned();
        let hash = tokio::task::spawn_blocking(move || {
            Argon2::default()
                .hash_password(password.as_bytes())
                .map(|hash| hash.to_string())
        })
        .await
        .map_err(|problem| {
            error!(%problem, "hashing a password failed");
            RejectReason::AuthenticationFailed
        })?
        .map_err(|problem| {
            error!(%problem, "hashing a password failed");
            RejectReason::AuthenticationFailed
        })?;

        let mut store = self.store();
        // Someone else may have taken the name while the password was hashed.
        if store.find(name).is_some() {
            return Err(RejectReason::NameTaken);
        }
        let account = Account {
            id: hex(&auth::new_nonce()),
            name: name.to_owned(),
            password: hash,
            devices: Vec::new(),
        };
        store.accounts.push(account);
        let index = store.accounts.len() - 1;
        store.link(index, device);
        info!(name, "account created");
        Ok(signed_in(&store.accounts[index]).expect("a fresh identity is well formed"))
    }

    /// Signs `device` in to the account called `name`, if `password` is its password.
    pub async fn sign_in(
        &self,
        name: &str,
        password: &str,
        device: &PublicKey,
    ) -> Result<SignedIn, RejectReason> {
        let found = {
            let store = self.store();
            store.find(name).map(|index| {
                let account = &store.accounts[index];
                (account.id.clone(), account.password.clone())
            })
        };
        let Some((id, hash)) = found else {
            return Err(RejectReason::WrongPassword);
        };
        // Verifying takes as long as hashing: away from the async threads, and without the lock.
        let password = password.to_owned();
        let matches = tokio::task::spawn_blocking(move || {
            PasswordHash::new(&hash).is_ok_and(|hash| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &hash)
                    .is_ok()
            })
        })
        .await
        .unwrap_or(false);
        if !matches {
            return Err(RejectReason::WrongPassword);
        }

        let mut store = self.store();
        // Found again by its identity: accounts are never removed, but one may have been added.
        let index = store
            .accounts
            .iter()
            .position(|account| account.id == id)
            .ok_or(RejectReason::WrongPassword)?;
        store.link(index, device);
        Ok(signed_in(&store.accounts[index]).expect("loaded identities are well formed"))
    }

    /// Forgets `device`: it is signed in to no account any more.
    pub fn sign_out(&self, device: &PublicKey) {
        let device = hex(device);
        let mut store = self.store();
        let mut changed = false;
        for account in &mut store.accounts {
            let before = account.devices.len();
            account.devices.retain(|known| *known != device);
            changed |= account.devices.len() != before;
        }
        if changed {
            store.save();
        }
    }
}

impl Store {
    /// The account called `name`, whatever its case.
    fn find(&self, name: &str) -> Option<usize> {
        let name = name.to_lowercase();
        self.accounts
            .iter()
            .position(|account| account.name.to_lowercase() == name)
    }

    /// Signs `device` in to the account at `index`, and out of any other.
    fn link(&mut self, index: usize, device: &PublicKey) {
        let device = hex(device);
        for account in &mut self.accounts {
            account.devices.retain(|known| *known != device);
        }
        self.accounts[index].devices.push(device);
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let pretty = ron::ser::PrettyConfig::default();
        match ron::ser::to_string_pretty(&self.accounts, pretty) {
            // Fails only if the writer is gone, which is the server stopping.
            Ok(text) => {
                let _ = file.send(text);
            }
            Err(problem) => error!(%problem, "the accounts could not be encoded"),
        }
    }
}

fn signed_in(account: &Account) -> Option<SignedIn> {
    Some(SignedIn {
        id: player_id(&account.id)?,
        name: account.name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(number: u8) -> PublicKey {
        [number; 32]
    }

    #[tokio::test]
    async fn an_account_signs_in_with_its_password_and_keeps_its_device() {
        let accounts = Accounts::in_memory();
        let created = accounts
            .create("Ada", "analytical engine", &device(1))
            .await
            .unwrap();
        assert_eq!(created.name, "Ada");
        assert_eq!(accounts.device(&device(1)), Some(created.clone()));

        assert_eq!(
            accounts.sign_in("ada", "wrong password", &device(2)).await,
            Err(RejectReason::WrongPassword)
        );
        assert_eq!(accounts.device(&device(2)), None);
        let again = accounts
            .sign_in("ADA", "analytical engine", &device(2))
            .await
            .unwrap();
        assert_eq!(again, created, "the same account, under the name it was created with");
        assert_eq!(accounts.device(&device(2)), Some(created));
    }

    #[tokio::test]
    async fn a_name_is_taken_whatever_its_case() {
        let accounts = Accounts::in_memory();
        accounts.create("Ada", "analytical engine", &device(1)).await.unwrap();
        assert_eq!(
            accounts.create("ADA", "another password", &device(2)).await,
            Err(RejectReason::NameTaken)
        );
    }

    #[tokio::test]
    async fn bad_names_and_passwords_are_refused() {
        let accounts = Accounts::in_memory();
        assert_eq!(
            accounts.create(" Ada", "analytical engine", &device(1)).await,
            Err(RejectReason::InvalidNickname)
        );
        assert_eq!(
            accounts.create("Ada", "short", &device(1)).await,
            Err(RejectReason::InvalidPassword)
        );
        assert_eq!(
            accounts.sign_in("Nobody", "analytical engine", &device(1)).await,
            Err(RejectReason::WrongPassword)
        );
    }

    #[tokio::test]
    async fn a_device_is_signed_in_to_one_account_and_can_sign_out() {
        let accounts = Accounts::in_memory();
        let ada = accounts.create("Ada", "analytical engine", &device(1)).await.unwrap();
        let grace = accounts.create("Grace", "compiler first", &device(1)).await.unwrap();
        assert_ne!(ada.id, grace.id);
        assert_eq!(accounts.device(&device(1)), Some(grace));

        accounts.sign_out(&device(1));
        assert_eq!(accounts.device(&device(1)), None);
    }

    #[test]
    fn the_file_keeps_hashes_not_passwords() {
        let hash = Argon2::default()
            .hash_password(b"analytical engine")
            .unwrap()
            .to_string();
        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(!hash.contains("analytical"));
    }
}
