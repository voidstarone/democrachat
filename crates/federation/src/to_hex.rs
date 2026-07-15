//! Hex encoding (no external dependency).

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_hex::from_hex;

    #[test]
    fn hex_round_trips() {
        let bytes = [0u8, 1, 15, 16, 255, 128, 7];
        let hex = to_hex(&bytes);
        assert_eq!(hex, "00010f10ff8007");
        assert_eq!(from_hex::<7>(&hex, "t").unwrap(), bytes);
        assert!(from_hex::<7>("xyz", "t").is_err());
        assert!(from_hex::<8>(&hex, "t").is_err()); // wrong length
    }
}
