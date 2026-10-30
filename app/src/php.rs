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

/// PHP's `round($value, $places)`: round() on the value scaled to that many decimals.
pub fn round_to(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    round(value * scale) / scale
}

/// The significant decimal digits PHP's round() trusts in a double.
const SIGNIFICANT_DIGITS: i32 = 15;

/// What PCRE's \s matches without Unicode properties, as PHP's patterns use it, /u or not:
/// space, tab, newline, vertical tab, form feed and carriage return.
pub fn is_pcre_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// PHP's `preg_replace('/\s+/u', ' ', $text)`.
pub fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_pcre_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// PHP's `trim($text)`: only space, tab, newline, carriage return, NUL and vertical tab.
pub fn trim(text: &str) -> &str {
    trim_chars(text, " \t\n\r\0\x0B")
}

/// PHP's `trim($text, $characters)` with a plain list of characters (no ranges).
pub fn trim_chars<'a>(text: &'a str, characters: &str) -> &'a str {
    text.trim_matches(|c| characters.contains(c))
}

/// PHP's `rtrim($text, $characters)` with a plain list of characters (no ranges).
pub fn trim_end_chars<'a>(text: &'a str, characters: &str) -> &'a str {
    text.trim_end_matches(|c| characters.contains(c))
}

/// Whether PHP counts the string as true, as array_filter() without a callback keeps it: not ""
/// and not "0".
pub fn filled(text: &str) -> bool {
    !text.is_empty() && text != "0"
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
