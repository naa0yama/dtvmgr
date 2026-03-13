//! Shared formatting utilities for TUI modules.

/// Formats an integer with 3-digit comma separators (e.g. 12500 → "12,500").
#[allow(clippy::arithmetic_side_effects)]
pub fn with_commas(n: u64) -> String {
    let s = n.to_string();
    let len = s.len();
    if len <= 3 {
        return s;
    }
    let mut result = String::with_capacity(len + len / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn small_numbers() {
        assert_eq!(with_commas(0), "0");
        assert_eq!(with_commas(999), "999");
    }

    #[test]
    fn thousands() {
        assert_eq!(with_commas(1000), "1,000");
        assert_eq!(with_commas(12500), "12,500");
        assert_eq!(with_commas(1_000_000), "1,000,000");
    }
}
