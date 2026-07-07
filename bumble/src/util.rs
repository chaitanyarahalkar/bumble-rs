//! Small generic helpers, ported from the portable parts of `bumble.utils` /
//! `bumble.core`.
//!
//! Note: most of `bumble.utils` is Python-asyncio event-emitter infrastructure
//! (`EventEmitter`, `AsyncRunner`, `FlowControlAsyncPipe`, …) with no place in
//! this synchronous port. The genuinely reusable pieces are here; the L2CAP
//! frame-check CRC lives in `bumble-l2cap::crc_16`, and the `OpenIntEnum` /
//! `CompatibleIntFlag` pattern is realized as newtype-with-constants throughout
//! the crates.

/// Names of the set bits in `bits`, in ascending bit order. A bit with no name
/// in `bit_flag_names` is rendered as `#<index>`. Mirrors
/// `bumble.core.bit_flags_to_strings`.
pub fn bit_flags_to_strings(mut bits: u64, bit_flag_names: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 0usize;
    while bits != 0 {
        if bits & 1 != 0 {
            names.push(
                bit_flag_names
                    .get(index)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("#{index}")),
            );
        }
        bits >>= 1;
        index += 1;
    }
    names
}

/// The name for `number` from `dictionary`, or a `[0x..]` hex fallback padded to
/// `width` hex digits. Mirrors `bumble.core.name_or_number`.
pub fn name_or_number(dictionary: &[(u64, &str)], number: u64, width: usize) -> String {
    dictionary
        .iter()
        .find(|(n, _)| *n == number)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("[0x{number:0width$X}]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_flags() {
        // bits 0 and 2 set.
        assert_eq!(
            bit_flags_to_strings(0b101, &["a", "b", "c"]),
            vec!["a".to_string(), "c".to_string()]
        );
        // unnamed bit → #index
        assert_eq!(bit_flags_to_strings(0b1000, &["a"]), vec!["#3".to_string()]);
    }

    #[test]
    fn name_or_number_lookup() {
        let dict = [(1u64, "one"), (2, "two")];
        assert_eq!(name_or_number(&dict, 1, 2), "one");
        assert_eq!(name_or_number(&dict, 9, 2), "[0x09]");
        assert_eq!(name_or_number(&dict, 0x1234, 4), "[0x1234]");
    }
}
