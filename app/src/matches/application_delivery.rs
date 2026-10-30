//! How the application is delivered, ported from modules/cv_match/Application_delivery.php. So far
//! only the mailto link the result and the document boxes print.

/// "mailto:job@firma.dk?subject=Backend%202026": the address with its subject line, as PHP's
/// rawurlencode writes them (keeping the @).
pub fn mailto(to: &str, subject: &str) -> String {
    let link = format!("mailto:{}", rawurlencode(to).replace("%40", "@"));
    if subject.is_empty() {
        link
    } else {
        format!("{link}?subject={}", rawurlencode(subject))
    }
}

/// PHP's rawurlencode: every byte but A-Z a-z 0-9 - _ . ~ as %XX.
pub fn rawurlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mailto_link_keeps_the_at_and_encodes_the_subject() {
        assert_eq!(mailto("job@firma.dk", ""), "mailto:job@firma.dk");
        assert_eq!(
            mailto("job@firma.dk", "Backend 2026 – æ"),
            "mailto:job@firma.dk?subject=Backend%202026%20%E2%80%93%20%C3%A6"
        );
    }
}
