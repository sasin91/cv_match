//! What JobFinder on the computer and its paired devices (a phone, a tablet) send each other.
//! The computer's app answers on 127.0.0.1:PORT, and `tailscale serve` passes on what a device
//! sends to https://<computer>.<tailnet>.ts.net:PORT, so a device reaches it wherever it is, with
//! a certificate its own HTTP stack trusts.
//!
//! A device pairs once: the computer shows a pairing link as a QR code (PairingLink), the device
//! scans it and sends its code to PAIR_PATH (PairAsk), and gets a token (PairAnswer) that it
//! sends with every request after that, as `Authorization: Bearer <token>`.
//!
//! Both sides use this crate, so neither can drift from the other.

use serde::{Deserialize, Serialize};

/// The port the computer's app answers devices on, on 127.0.0.1 and through `tailscale serve`.
pub const PORT: u16 = 8766;

/// Where a device trades its pairing code for a token (PairAsk, PairAnswer).
pub const PAIR_PATH: &str = "/device/v1/pair";

/// Where a paired device asks what it is paired with (Hello).
pub const HELLO_PATH: &str = "/device/v1/hello";

/// How long a pairing code works after the computer shows it, in seconds.
pub const PAIRING_CODE_SECONDS: i64 = 5 * 60;

/// The scheme of a pairing link: jobfinder-pair://<host>:<port>/<code>.
pub const PAIRING_SCHEME: &str = "jobfinder-pair";

/// Most characters of the name a device gives itself.
pub const MAX_DEVICE_NAME: usize = 80;

/// What the QR code on the computer holds: where to reach it, and the code that pairs a device
/// once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLink {
    /// The computer's name on the tailnet, as `tailscale status` gives it: pc.tailnet.ts.net.
    pub host: String,
    pub port: u16,
    pub code: String,
}

impl PairingLink {
    /// The link the QR code shows: jobfinder-pair://pc.tailnet.ts.net:8766/<code>.
    pub fn to_link(&self) -> String {
        format!("{PAIRING_SCHEME}://{}:{}/{}", self.host, self.port, self.code)
    }

    /// The pairing link in link, or why it isn't one.
    pub fn parse(link: &str) -> Result<Self, String> {
        let not_one = || format!("This isn't a JobFinder pairing code: {link}");
        let rest = link
            .trim()
            .strip_prefix(PAIRING_SCHEME)
            .and_then(|rest| rest.strip_prefix("://"))
            .ok_or_else(not_one)?;
        let (address, code) = rest.split_once('/').ok_or_else(not_one)?;
        let (host, port) = address.rsplit_once(':').ok_or_else(not_one)?;
        let port = port.parse().map_err(|_| not_one())?;
        if !is_host_name(host) || !is_code(code) {
            return Err(not_one());
        }
        Ok(PairingLink {
            host: host.to_string(),
            port,
            code: code.to_string(),
        })
    }

    /// Where the device sends its requests: https://pc.tailnet.ts.net:8766.
    pub fn address(&self) -> String {
        format!("https://{}:{}", self.host, self.port)
    }
}

/// Whether host is a DNS name: letters, digits, '-' and '.', not empty.
fn is_host_name(host: &str) -> bool {
    !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// Whether code looks like a pairing code or a token's secret: lowercase hex, not empty.
pub fn is_code(code: &str) -> bool {
    !code.is_empty() && code.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// A device's pairing request, sent to PAIR_PATH.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairAsk {
    /// The code from the pairing link.
    pub code: String,
    /// What the device calls itself ("Pixel 8"), as the computer lists it.
    pub name: String,
}

/// The computer's answer to PairAsk: the device's token, to keep and send from now on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairAnswer {
    pub token: String,
}

/// A paired device's token: which device it is, and its secret. Written <device id>.<secret>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub device_id: i64,
    pub secret: String,
}

impl Token {
    pub fn to_text(&self) -> String {
        format!("{}.{}", self.device_id, self.secret)
    }

    /// The token in text, or None when it isn't shaped as one.
    pub fn parse(text: &str) -> Option<Self> {
        let (id, secret) = text.trim().split_once('.')?;
        let device_id = id.parse().ok().filter(|id: &i64| *id > 0)?;
        is_code(secret).then(|| Token {
            device_id,
            secret: secret.to_string(),
        })
    }
}

/// What HELLO_PATH answers: what the device is paired with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// JobFinder's version on the computer.
    pub version: String,
    /// The name the computer gave the device when it paired.
    pub device_name: String,
}

/// What the computer answers when it refuses or fails: why, in words to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pairing_link_reads_back_as_written() {
        let link = PairingLink {
            host: "pc.tail1234.ts.net".to_string(),
            port: PORT,
            code: "0a1b2c".to_string(),
        };
        assert_eq!(link.to_link(), "jobfinder-pair://pc.tail1234.ts.net:8766/0a1b2c");
        assert_eq!(PairingLink::parse(&link.to_link()), Ok(link.clone()));
        assert_eq!(link.address(), "https://pc.tail1234.ts.net:8766");
    }

    #[test]
    fn anything_else_is_not_a_pairing_link() {
        for link in [
            "https://pc.tail1234.ts.net:8766/0a1b2c",
            "jobfinder-pair://pc.tail1234.ts.net/0a1b2c",
            "jobfinder-pair://pc.tail1234.ts.net:port/0a1b2c",
            "jobfinder-pair://pc.tail1234.ts.net:8766/",
            "jobfinder-pair://pc.tail1234.ts.net:8766/NOT-HEX",
            "jobfinder-pair://evil.example/x:8766/0a1b2c",
        ] {
            assert!(PairingLink::parse(link).is_err(), "{link}");
        }
    }

    #[test]
    fn a_token_reads_back_as_written_and_nothing_else_is_one() {
        let token = Token {
            device_id: 3,
            secret: "beef".to_string(),
        };
        assert_eq!(Token::parse(&token.to_text()), Some(token));
        for text in ["", "3", "3.", "x.beef", "0.beef", "-1.beef", "3.BEEF", "3.be ef"] {
            assert_eq!(Token::parse(text), None, "{text}");
        }
    }

    #[test]
    fn the_messages_are_the_json_both_sides_expect() {
        let ask = PairAsk {
            code: "0a".to_string(),
            name: "Pixel".to_string(),
        };
        let json = serde_json::to_string(&ask).unwrap();
        assert_eq!(json, r#"{"code":"0a","name":"Pixel"}"#);
        assert_eq!(serde_json::from_str::<PairAsk>(&json).unwrap(), ask);
    }
}
