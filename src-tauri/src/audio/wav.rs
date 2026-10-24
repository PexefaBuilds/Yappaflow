//! Minimal PCM16 mono WAV encoder.

pub fn encode(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// Decode a WAV produced by `encode` (used by "retry last"). Returns (samples, rate).
pub fn decode(bytes: &[u8]) -> Option<(Vec<i16>, u32)> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let rate = u32::from_le_bytes(bytes[24..28].try_into().ok()?);
    // walk chunks to find "data"
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        if id == b"data" {
            let end = (i + 8 + len).min(bytes.len());
            let pcm = bytes[i + 8..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            return Some((pcm, rate));
        }
        i += 8 + len + (len & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_roundtrip() {
        let pcm: Vec<i16> = vec![0, 1, -1, 32767, -32768];
        let w = encode(&pcm, 16_000);
        assert_eq!(w.len(), 44 + 10);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 10);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
        let (back, rate) = decode(&w).unwrap();
        assert_eq!(back, pcm);
        assert_eq!(rate, 16_000);
    }
}
