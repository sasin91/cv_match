//! The devices paired with the app: a phone or a tablet, which shows the shortlist and sends job
//! posts to evaluate. A device reaches the app through Tailscale: `tailscale serve` passes
//! https://<computer>.<tailnet>.ts.net:8766 on to device_server, which listens on 127.0.0.1:8766
//! only. A device pairs once with a code the window shows as a QR code, and sends the token it
//! got for it with every request after that (device_service). What both sides send is in the
//! device-protocol crate.
//!
//! Devices holds what pairing needs while the app runs: the one pairing code shown now, and the
//! computer's tailnet name, which is the only Host device_server answers to; and the job posts
//! devices sent to be matched (evaluations).

pub mod device_matches;
pub mod device_model;
pub mod device_server;
pub mod device_service;
pub mod evaluations;
pub mod tailscale;

use std::sync::Mutex;

/// How long the tailnet name read last is used before a request with another Host reads it
/// again, in seconds: Tailscale may have started, or been signed in to another tailnet.
const TAILNET_NAME_SECONDS: i64 = 60;

#[derive(Default)]
pub struct Devices {
    /// The pairing code shown now, until it is used or too old.
    pairing: Mutex<Option<PairingCode>>,
    /// The computer's tailnet name, as read last (None: not read, or Tailscale isn't running).
    tailnet: Mutex<TailnetName>,
    /// Why device_server isn't listening, when it couldn't (its port taken, say).
    pub not_listening: Mutex<Option<String>>,
    /// The job posts devices sent to be matched, and what became of them.
    pub evaluations: evaluations::Evaluations,
}

struct PairingCode {
    code: String,
    expires_at: i64,
}

#[derive(Default)]
struct TailnetName {
    name: Option<String>,
    read_at: Option<i64>,
}

impl Devices {
    /// Makes a new pairing code, good once until expires_at, in place of one shown before.
    pub fn new_pairing_code(&self, code: String, expires_at: i64) {
        *lock(&self.pairing) = Some(PairingCode { code, expires_at });
    }

    /// Whether code is the pairing code shown now and still good at now. A good code is used up.
    pub fn take_pairing_code(&self, code: &str, now: i64) -> bool {
        let mut pairing = lock(&self.pairing);
        let good = pairing
            .as_ref()
            .is_some_and(|shown| now < shown.expires_at && same_secret(&shown.code, code));
        if good {
            *pairing = None;
        }
        good
    }

    /// The tailnet name read last.
    pub fn tailnet_name(&self) -> Option<String> {
        lock(&self.tailnet).name.clone()
    }

    /// Keeps name as the computer's tailnet name, read at now.
    pub fn set_tailnet_name(&self, name: Option<String>, now: i64) {
        *lock(&self.tailnet) = TailnetName {
            name,
            read_at: Some(now),
        };
    }

    /// Whether the tailnet name should be read again at now: never read, or read longer than
    /// TAILNET_NAME_SECONDS ago.
    pub fn tailnet_name_is_old(&self, now: i64) -> bool {
        lock(&self.tailnet)
            .read_at
            .is_none_or(|read_at| now - read_at >= TAILNET_NAME_SECONDS)
    }
}

/// Whether two secrets are the same, compared in a time that doesn't say how much of them is.
pub fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |differs, (x, y)| differs | (x ^ y)) == 0
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pairing_code_works_once_and_only_while_it_is_good() {
        let devices = Devices::default();
        devices.new_pairing_code("ab12".to_string(), 100);
        assert!(!devices.take_pairing_code("ab13", 50));
        assert!(!devices.take_pairing_code("ab12", 100));
        assert!(devices.take_pairing_code("ab12", 99));
        assert!(!devices.take_pairing_code("ab12", 99));
    }

    #[test]
    fn a_new_pairing_code_replaces_the_one_before() {
        let devices = Devices::default();
        devices.new_pairing_code("aaaa".to_string(), 100);
        devices.new_pairing_code("bbbb".to_string(), 100);
        assert!(!devices.take_pairing_code("aaaa", 1));
        assert!(devices.take_pairing_code("bbbb", 1));
    }

    #[test]
    fn the_tailnet_name_is_read_again_once_it_is_old() {
        let devices = Devices::default();
        assert!(devices.tailnet_name_is_old(0));
        devices.set_tailnet_name(Some("pc.tail1.ts.net".to_string()), 1000);
        assert!(!devices.tailnet_name_is_old(1000 + TAILNET_NAME_SECONDS - 1));
        assert!(devices.tailnet_name_is_old(1000 + TAILNET_NAME_SECONDS));
        assert_eq!(devices.tailnet_name().as_deref(), Some("pc.tail1.ts.net"));
    }

    #[test]
    fn secrets_are_the_same_only_when_every_byte_is() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
    }
}
