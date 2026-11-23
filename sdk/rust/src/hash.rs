//! The engine's 32-bit name hash: Jenkins one-at-a-time over the ASCII-lower-cased bytes.

use std::ffi::{c_char, CStr};

/// `joaat("adder")`
pub const fn joaat(s: &str) -> u32 {
    joaat_bytes(s.as_bytes())
}

pub const fn joaat_bytes(b: &[u8]) -> u32 {
    let mut h: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i].to_ascii_lowercase();
        h = h.wrapping_add(c as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
        i += 1;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h.wrapping_add(h << 15)
}

/// C form (used when a host offers no hash function).
pub(crate) unsafe extern "C" fn joaat_c(t: *const c_char) -> u32 {
    if t.is_null() {
        return 0;
    }
    joaat_bytes(CStr::from_ptr(t).to_bytes())
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_values() {
        // "a" is the public one-at-a-time test vector; the engine form lower-cases first
        assert_eq!(super::joaat(""), 0);
        assert_eq!(super::joaat("a"), 0xCA2E_9442);
        assert_eq!(super::joaat("A"), 0xCA2E_9442);
        assert_eq!(super::joaat("The quick brown fox jumps over the lazy dog"), 0x2D16_43CE);
        assert_eq!(super::joaat("adder"), 0xB779_A091);
    }
}
