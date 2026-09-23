//! Account-less authentication: a player is identified by their Ed25519 public key.
//!
//! The client generates its key on first launch and keeps it. On every connection, the server
//! sends a random nonce and the client signs it, proving it owns the private key.

pub use ed25519_dalek::SigningKey;
use ed25519_dalek::{Signature, Signer, VerifyingKey};

pub type PublicKey = [u8; 32];
pub type Nonce = [u8; 32];
pub type SignatureBytes = [u8; 64];

/// Signed along with the nonce, so a signature made for this game is worthless anywhere else.
const CONTEXT: &[u8] = b"space-race/auth/v1";

pub fn new_signing_key() -> SigningKey {
    SigningKey::from_bytes(&random_bytes())
}

pub fn new_nonce() -> Nonce {
    random_bytes()
}

pub fn sign_challenge(key: &SigningKey, nonce: &Nonce) -> SignatureBytes {
    key.sign(&signed_message(nonce)).to_bytes()
}

pub fn verify_challenge(public_key: &PublicKey, nonce: &Nonce, signature: &SignatureBytes) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    key.verify_strict(&signed_message(nonce), &Signature::from_bytes(signature))
        .is_ok()
}

fn signed_message(nonce: &Nonce) -> Vec<u8> {
    [CONTEXT, nonce].concat()
}

fn random_bytes() -> [u8; 32] {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).expect("OS random generator unavailable");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_signature_is_accepted() {
        let key = new_signing_key();
        let nonce = new_nonce();
        let signature = sign_challenge(&key, &nonce);

        assert!(verify_challenge(
            &key.verifying_key().to_bytes(),
            &nonce,
            &signature
        ));
    }

    #[test]
    fn signature_for_another_nonce_is_refused() {
        let key = new_signing_key();
        let signature = sign_challenge(&key, &new_nonce());

        assert!(!verify_challenge(
            &key.verifying_key().to_bytes(),
            &new_nonce(),
            &signature
        ));
    }

    #[test]
    fn signature_from_another_key_is_refused() {
        let nonce = new_nonce();
        let signature = sign_challenge(&new_signing_key(), &nonce);

        assert!(!verify_challenge(
            &new_signing_key().verifying_key().to_bytes(),
            &nonce,
            &signature
        ));
    }
}
