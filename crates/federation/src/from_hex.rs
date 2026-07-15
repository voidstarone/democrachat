//! Hex decoding (no external dependency).

use crate::fed_error::FedError;

pub(crate) fn from_hex<const N: usize>(s: &str, what: &'static str) -> Result<[u8; N], FedError> {
    let bytes = s.as_bytes();
    if bytes.len() != N * 2 {
        return Err(FedError::BadHex(what));
    }
    let mut out = [0u8; N];
    for (i, chunk) in bytes.chunks_exact(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or(FedError::BadHex(what))?;
        let lo = (chunk[1] as char).to_digit(16).ok_or(FedError::BadHex(what))?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}
