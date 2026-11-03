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
pub const PAIR_PATH: &str = "/v1/device/pair";

/// Where a paired device asks what it is paired with (Hello).
pub const HELLO_PATH: &str = "/v1/device/hello";

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

/// Where a paired device reads a list of saved matches, best first (MatchesAsk, Matches).
pub const MATCHES_PATH: &str = "/v1/device/matches";

/// Where a paired device reads one saved match (MatchAsk, MatchDetails).
pub const MATCH_PATH: &str = "/v1/device/match";

/// Where a paired device sends a job post, a link or its text, to be matched (EvaluateAsk,
/// Evaluation).
pub const EVALUATE_PATH: &str = "/v1/device/evaluate";

/// Where a paired device reads what became of the job posts it sent (Evaluations).
pub const EVALUATIONS_PATH: &str = "/v1/device/evaluations";

/// Most characters of a job post a device may send to EVALUATE_PATH: a long post is a few
/// thousand; this leaves room for one pasted with its page around it.
pub const MAX_EVALUATE_CHARS: usize = 100_000;

/// The list a device shows first.
pub const SHORTLIST: &str = "shortlist";

/// Which saved matches a device asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchesAsk {
    /// The list's name (one of Matches::lists); the shortlist when None or unknown.
    #[serde(default)]
    pub list: Option<String>,
    /// Matches::next_after of the batch before; empty for the first batch.
    #[serde(default)]
    pub after: String,
}

/// A list a device can ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListName {
    /// What MatchesAsk::list names it by: "shortlist".
    pub name: String,
    /// How it is shown: "Shortlist".
    pub label: String,
    /// What to show when it has no matches.
    pub empty: String,
}

/// A batch of saved matches, best first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matches {
    /// Every list there is, in the order the computer shows them.
    pub lists: Vec<ListName>,
    /// The list these matches are from.
    pub list: String,
    pub matches: Vec<MatchRow>,
    /// What to send as MatchesAsk::after for the next batch, when there is one.
    pub next_after: Option<String>,
}

/// A saved match in a list, as the computer's list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRow {
    pub id: i64,
    /// "top", "good", "fair" or "low".
    pub tag: String,
    pub title: String,
    /// "81% good · likely disqualifier: Located close to Copenhagen".
    pub about: String,
    pub saved_at: String,
    pub saved_on: String,
    /// "application · applied · on Jobnet", or "".
    pub done: String,
    /// "Deadline 15 Nov (in 3 days)" or "Closed 2 Nov: …", or "".
    pub opening: String,
    /// The job no longer takes applications.
    pub closed: bool,
}

/// Which saved match a device asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchAsk {
    pub id: i64,
}

/// A saved match as the computer's match page shows it, without its application panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchDetails {
    pub id: i64,
    pub percent: i64,
    pub tag: String,
    /// "Good match".
    pub tag_name: String,
    /// The job title and company.
    pub heading: String,
    /// How the score is made up, in words.
    pub parts: String,
    pub saved_at: String,
    /// Where the post was read; empty for a pasted post.
    pub job_url: String,
    /// Whether it still takes applications, when that is known.
    pub opening: Option<OpeningText>,
    /// Requirements missed that likely rule the job out, with why.
    pub disqualifiers: Vec<Requirement>,
    /// The requirements by group, missing first in each.
    pub sections: Vec<RequirementGroup>,
}

/// "Deadline 15 Nov (in 3 days)", or "Closed 2 Nov: the post was taken down".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningText {
    pub text: String,
    pub closed: bool,
}

/// One group of a match's requirements: "Requirements", "Nice to have", ….
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementGroup {
    pub heading: String,
    /// "not weighed · a miss caps the score", or "".
    pub note: String,
    pub rows: Vec<Requirement>,
}

/// A requirement and its verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    /// "missing", "partial" or "met".
    pub verdict: String,
    pub text: String,
    pub reason: String,
    /// What the CV says that meets it, or "".
    pub evidence: String,
    /// "model", "rule", "taxonomy, exact", or "".
    pub decided_by: String,
    /// For a requirement met by any of several: each of them.
    pub alternatives: Vec<Requirement>,
}

/// A job post a device sends to be matched with the user's profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluateAsk {
    /// A link to the post (as shared from a browser), or its text.
    pub text: String,
}

/// Evaluation::state while the post is being read and matched.
pub const EVALUATION_WORKING: &str = "working";
/// Evaluation::state once it is matched.
pub const EVALUATION_DONE: &str = "done";
/// Evaluation::state when it couldn't be matched; Evaluation::said says why.
pub const EVALUATION_FAILED: &str = "failed";

/// A job post a device sent, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    /// Its number on the computer, for as long as the app runs there.
    pub id: u64,
    /// The start of what was sent: the link, or the post's first line.
    pub sent: String,
    /// When it was sent, in unix seconds.
    pub sent_at: i64,
    /// EVALUATION_WORKING, EVALUATION_DONE or EVALUATION_FAILED.
    pub state: String,
    /// The step it is at ("Reading the job post…"), the error when it failed, or what to know
    /// of its result ("Matched before: this is the saved match.").
    pub said: String,
    /// The saved match it became, to open at MATCH_PATH; None until done, and for a match that
    /// wasn't saved.
    pub match_id: Option<i64>,
    /// The match's score and job, once done.
    pub percent: Option<i64>,
    pub tag: String,
    pub tag_name: String,
    pub heading: String,
}

/// The job posts this device sent while the app has run on the computer, the latest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluations {
    pub evaluations: Vec<Evaluation>,
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

    #[test]
    fn an_empty_matches_ask_is_the_shortlists_first_batch() {
        assert_eq!(serde_json::from_str::<MatchesAsk>("{}").unwrap(), MatchesAsk::default());
    }
}
