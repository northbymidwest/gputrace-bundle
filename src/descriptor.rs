//! The 248-byte Metal texture descriptor: 31 little-endian u64 words whose
//! word 0 is the capture's descriptor tag. Word map established by
//! `gpu-trace-parse-rs` and reproduced here (byte offset = word * 8).

use std::collections::HashMap;

pub(crate) const DESC_SIZE: usize = 248;

/// A decoded texture descriptor. Fields are raw Metal enum/bitflag values; hl
/// interprets them. `store0_offset` is the join key (bridge order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureDescriptor {
    pub store0_offset: u64,
    pub format: u32,
    pub texture_type: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u32,
    pub array_length: u32,
    pub sample_count: u32,
    pub usage: u64,
    pub texture_id: u64,
}

/// Read word `i` (little-endian u64) of a descriptor payload, 0 if out of range.
fn word(payload: &[u8], i: usize) -> u64 {
    let o = i * 8;
    payload
        .get(o..o + 8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

/// Word 0: the per-capture descriptor tag shared by every descriptor struct.
pub(crate) fn descriptor_tag(payload: &[u8]) -> u64 {
    word(payload, 0)
}

pub(crate) fn parse_descriptor(payload: &[u8], store0_offset: u64) -> TextureDescriptor {
    TextureDescriptor {
        store0_offset,
        texture_type: word(payload, 1) as u32,
        format: word(payload, 2) as u32,
        width: word(payload, 3) as u32,
        height: word(payload, 4) as u32,
        depth: word(payload, 5) as u32,
        mip_levels: word(payload, 6) as u32,
        array_length: word(payload, 7) as u32,
        sample_count: word(payload, 8) as u32,
        usage: word(payload, 11),
        texture_id: word(payload, 17),
    }
}

/// The capture's descriptor tag: the most common word-0 value among the
/// 248-byte payloads. `None` when there are none.
pub(crate) fn derive_tag(word0s: &[u64]) -> Option<u64> {
    let mut votes: HashMap<u64, usize> = HashMap::new();
    for &w in word0s {
        *votes.entry(w).or_default() += 1;
    }
    votes.into_iter().max_by_key(|&(_, n)| n).map(|(w, _)| w)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a synthetic 248-byte descriptor: 31 LE u64 words, tag at word 0.
    fn desc(tag: u64, fields: &[(usize, u64)]) -> Vec<u8> {
        let mut p = vec![0u8; DESC_SIZE];
        let mut set = |i: usize, v: u64| p[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        set(0, tag);
        for &(i, v) in fields {
            set(i, v);
        }
        p
    }

    #[test]
    fn parses_the_word_map() {
        let p = desc(
            0xE1,
            &[
                (1, 2),   // texture_type = 2D-ish
                (2, 80),  // format = BGRA8Unorm
                (3, 256), // width
                (4, 256), // height
                (5, 1),   // depth
                (6, 9),   // mip_levels
                (7, 1),   // array_length
                (8, 1),   // sample_count
                (11, 5),  // usage
                (17, 16), // texture_id
            ],
        );
        let d = parse_descriptor(&p, 12964);
        assert_eq!(d.store0_offset, 12964);
        assert_eq!(d.format, 80);
        assert_eq!(d.texture_type, 2);
        assert_eq!((d.width, d.height, d.depth), (256, 256, 1));
        assert_eq!((d.mip_levels, d.array_length, d.sample_count), (9, 1, 1));
        assert_eq!(d.usage, 5);
        assert_eq!(d.texture_id, 16);
        assert_eq!(descriptor_tag(&p), 0xE1);
    }

    #[test]
    fn derives_the_modal_tag() {
        // three descriptors tagged 0xE1, one stray 0x99 -> tag is 0xE1
        let tags = [0xE1u64, 0xE1, 0x99, 0xE1];
        assert_eq!(derive_tag(&tags), Some(0xE1));
        assert_eq!(derive_tag(&[]), None);
    }
}
