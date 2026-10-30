//! The few PHP built-ins whose exact behaviour the ported code depends on, so the Rust answers
//! match the PHP app's to the last digit and character.

/// PHP's `round($value)` (PHP 8.3): half away from zero, after pre-rounding to the 15
/// significant digits a double reliably holds, so 0.33 * 150 (49.4999…) rounds to 50 as in PHP.
pub fn round(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    // 14 - floor(log10(|value|)): the decimal places that keep 15 significant digits.
    let precision_places = SIGNIFICANT_DIGITS - 1 - value.abs().log10().floor() as i32;
    if precision_places > 0 && precision_places < SIGNIFICANT_DIGITS {
        let scale = 10f64.powi(precision_places);
        return ((value * scale).round() / scale).round();
    }
    value.round()
}

/// The significant decimal digits PHP's round() trusts in a double.
const SIGNIFICANT_DIGITS: i32 = 15;

/// PHP's `trim($text)`: only space, tab, newline, carriage return, NUL and vertical tab.
pub fn trim(text: &str) -> &str {
    trim_chars(text, " \t\n\r\0\x0B")
}

/// PHP's `trim($text, $characters)` with a plain list of characters (no ranges).
pub fn trim_chars<'a>(text: &'a str, characters: &str) -> &'a str {
    text.trim_matches(|c| characters.contains(c))
}

/// PHP's `mb_substr($text, 0, $length)`: the first `length` characters.
pub fn first_chars(text: &str, length: usize) -> String {
    text.chars().take(length).collect()
}

/// PHP's `ctype_digit($text)`: non-empty and only ASCII digits.
pub fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_follows_php_on_halves_float_error_cannot_reach() {
        assert_eq!(round(0.33 * 150.0), 50.0);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(round(0.49), 0.0);
        assert_eq!(round(80.49999), 80.0);
    }

    #[test]
    fn trim_leaves_other_whitespace_alone() {
        assert_eq!(trim("\u{a0} x \n"), "\u{a0} x");
        assert_eq!(trim_chars("Du er:.", " \t:.!-"), "Du er");
    }
}
