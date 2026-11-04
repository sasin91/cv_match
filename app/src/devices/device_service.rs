//! Pairing a device and knowing it again: the pairing link the window shows as a QR code, the
//! token a device trades its code for, the token checked on each request, and removing a device.
//! A token is treated like an API token: only the device has it, and this computer keeps only its
//! SHA-256 hash (token_hash), so nothing kept here can be used to reach the app.
//! Errors are said to whoever asked (the window, or the device) and logged where they aren't the
//! asker's own doing.

use device_protocol::{Hello, MAX_DEVICE_NAME, PAIRING_CODE_SECONDS, PORT, PairAnswer, PairAsk, PairingLink, Token};

use super::device_model::{self, Device};
use super::same_secret;
use crate::context::Context;
use crate::service_error::{ErrorKind, ServiceError};

/// Random bytes in a pairing code (written as twice as many hex digits).
const PAIRING_CODE_BYTES: usize = 16;

/// Random bytes in a device token's secret (written as twice as many hex digits).
const TOKEN_SECRET_BYTES: usize = 32;

/// What a device is called when it gives no name.
const UNNAMED_DEVICE: &str = "A device";

/// A new pairing link for the window to show, good once for PAIRING_CODE_SECONDS from now, in
/// place of the one shown before. Err when Tailscale isn't running, so a device couldn't reach
/// the app.
pub fn new_pairing_link(context: &Context, now: i64) -> Result<PairingLink, ServiceError> {
    let Some(host) = context.devices.tailnet_name() else {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Tailscale isn't running on this computer, so a device couldn't reach JobFinder. Start Tailscale and sign in, then try again.",
        ));
    };
    let code = random_hex(PAIRING_CODE_BYTES)?;
    context
        .devices
        .new_pairing_code(code.clone(), now + PAIRING_CODE_SECONDS);
    Ok(PairingLink { host, port: PORT, code })
}

/// Pairs the device that sent ask with the pairing code shown now: keeps it and answers its
/// token. The code works once.
pub async fn pair(context: &Context, ask: &PairAsk, now: i64) -> Result<PairAnswer, ServiceError> {
    if !context.devices.take_pairing_code(ask.code.trim(), now) {
        return Err(ServiceError::new(
            ErrorKind::NotAllowed,
            "This pairing code is used or too old. Show a new one on the computer, under Settings, Devices.",
        ));
    }
    let name = device_name(&ask.name);
    let device_id = device_model::add(context.db(), &name, now)
        .await
        .map_err(|e| ServiceError::internal("devices/pair", format!("The device couldn't be kept: {e}")))?;
    let token = Token {
        device_id,
        secret: random_hex(TOKEN_SECRET_BYTES)?,
    };
    if let Err(why) = context
        .key_vault
        .save_device_token_hash(context.db(), device_id, &token_hash(&token.secret))
        .await
    {
        // A device without a kept token could never be known again: don't keep it at all.
        if let Err(e) = device_model::remove(context.db(), device_id).await {
            crate::log_line!("devices/pair: device {device_id} without a token couldn't be removed: {e}");
        }
        return Err(ServiceError::internal(
            "devices/pair",
            format!("The device's token couldn't be kept: {why}"),
        ));
    }
    crate::log_line!("devices: paired \"{name}\" (device {device_id}).");
    Ok(PairAnswer { token: token.to_text() })
}

/// The paired device whose token is in authorization (the request's Authorization header), noted
/// as seen at now. NotAllowed when there is no such device or the token isn't its own.
pub async fn device_for(context: &Context, authorization: Option<&str>, now: i64) -> Result<Device, ServiceError> {
    let not_paired = || {
        ServiceError::new(
            ErrorKind::NotAllowed,
            "This device isn't paired with JobFinder on this computer. Pair it again under Settings, Devices.",
        )
    };
    let token = authorization
        .and_then(|header| header.strip_prefix("Bearer "))
        .and_then(Token::parse)
        .ok_or_else(not_paired)?;
    let kept = context
        .key_vault
        .device_token_hash(context.db(), token.device_id)
        .await
        .map_err(|why| ServiceError::internal("devices", format!("The device's token couldn't be read: {why}")))?;
    if !kept.is_some_and(|kept| same_secret(&kept, &token_hash(&token.secret))) {
        return Err(not_paired());
    }
    let db = context.db();
    let device = device_model::find(db, token.device_id)
        .await
        .map_err(|e| ServiceError::internal("devices", format!("The device couldn't be read: {e}")))?
        .ok_or_else(not_paired)?;
    if let Err(e) = device_model::seen(db, device.id, now).await {
        // Only the Devices screen's "last seen" misses it.
        crate::log_line!("devices: device {} couldn't be noted as seen: {e}", device.id);
    }
    Ok(device)
}

/// What a paired device is told it is paired with.
pub fn hello(device: &Device) -> Hello {
    Hello {
        version: env!("CARGO_PKG_VERSION").to_string(),
        device_name: device.name.clone(),
    }
}

/// Every paired device, the one paired first first.
pub async fn devices(context: &Context) -> Result<Vec<Device>, ServiceError> {
    device_model::all(context.db())
        .await
        .map_err(|e| ServiceError::internal("devices", format!("The paired devices couldn't be read: {e}")))
}

/// Unpairs the device with this id: its token stops working and it is forgotten.
pub async fn remove(context: &Context, id: i64) -> Result<(), ServiceError> {
    // The token first: once it is gone the device can't get in, whatever happens to its row.
    context
        .key_vault
        .forget_device_token_hash(context.db(), id)
        .await
        .map_err(|why| ServiceError::internal("devices", format!("The device's token couldn't be removed: {why}")))?;
    let removed = device_model::remove(context.db(), id)
        .await
        .map_err(|e| ServiceError::internal("devices", format!("The device couldn't be removed: {e}")))?;
    if !removed {
        return Err(ServiceError::new(ErrorKind::NotFound, "That device isn't paired."));
    }
    context.devices.evaluations.forget(id);
    crate::log_line!("devices: removed device {id}.");
    Ok(())
}

/// The name a device gave itself, tidied: one line, at most MAX_DEVICE_NAME characters.
fn device_name(asked: &str) -> String {
    let name: String = asked
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_DEVICE_NAME)
        .collect();
    if name.is_empty() {
        UNNAMED_DEVICE.to_string()
    } else {
        name
    }
}

/// What this computer keeps of a token's secret: its SHA-256, as lowercase hex.
pub fn token_hash(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(secret.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// bytes random bytes from the system, as lowercase hex.
fn random_hex(bytes: usize) -> Result<String, ServiceError> {
    let mut random = vec![0u8; bytes];
    getrandom::fill(&mut random)
        .map_err(|e| ServiceError::internal("devices", format!("The system gave no random numbers for a code: {e}")))?;
    Ok(hex(&random))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_name_is_one_tidy_line() {
        assert_eq!(device_name("  Pixel\n 8  "), "Pixel 8");
        assert_eq!(device_name(" "), UNNAMED_DEVICE);
        assert_eq!(device_name(&"x".repeat(200)).chars().count(), MAX_DEVICE_NAME);
    }

    #[test]
    fn a_token_hash_is_the_secret_s_sha256() {
        assert_eq!(
            token_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn random_hex_is_hex_of_twice_the_bytes() {
        let code = random_hex(PAIRING_CODE_BYTES).unwrap();
        assert_eq!(code.len(), PAIRING_CODE_BYTES * 2);
        assert!(device_protocol::is_code(&code));
        assert_ne!(code, random_hex(PAIRING_CODE_BYTES).unwrap());
    }
}
