//! The `xdic` index: a 20-byte header, a bucket table, then a record array.

use crate::Error;

const XDIC_MAGIC: &[u8; 4] = b"xdic";
const HEADER_LEN: usize = 20;
const BUCKET_SIZE: usize = 12;
pub(crate) const RECORD_SIZE: usize = 24;
const EMPTY: u32 = u32::MAX;

pub(crate) struct Header {
    pub bucket_count: u32,
    pub record_count: u32,
}

impl Header {
    pub fn parse(index: &[u8]) -> Result<Header, Error> {
        let head = index
            .get(..HEADER_LEN)
            .ok_or(Error::BadIndex("index shorter than header"))?;
        if &head[..4] != XDIC_MAGIC {
            return Err(Error::BadIndex("bad xdic magic"));
        }
        let word = |i: usize| u32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap());
        Ok(Header {
            bucket_count: word(2),
            record_count: word(3),
        })
    }

    /// The record array begins after the header and the bucket table.
    pub fn record_array_offset(&self) -> Result<usize, Error> {
        (self.bucket_count as usize)
            .checked_mul(BUCKET_SIZE)
            .and_then(|b| b.checked_add(HEADER_LEN))
            .ok_or(Error::BadIndex("record array offset overflow"))
    }
}

pub(crate) struct Record {
    pub usize_len: u32,
    pub csize: u32,
    pub store0_offset: u64,
}

impl Record {
    pub fn parse_at(index: &[u8], off: usize) -> Option<Record> {
        let b = index.get(off..off.checked_add(RECORD_SIZE)?)?;
        Some(Record {
            usize_len: u32::from_le_bytes(b[0..4].try_into().unwrap()),
            csize: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            store0_offset: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        })
    }

    /// An alias carries a marker where an extent would be: `store0_offset == 0`
    /// and `csize == this record's own index`. Its bytes live in a canonical
    /// record reached via the bucket table.
    pub fn is_alias(&self, id: usize) -> bool {
        self.store0_offset == 0 && self.csize as usize == id
    }
}

/// Open-addressed hash table mapping each record to the record that actually
/// holds its bytes. Absent (`None`) when the table fails validation, in which
/// case every non-alias record is still usable.
pub(crate) struct Buckets {
    canonical: Vec<u32>,
}

impl Buckets {
    pub fn build(index: &[u8], header: &Header) -> Option<Buckets> {
        let n = header.record_count as usize;
        let mut canonical: Vec<u32> = (0..header.record_count).collect();
        let mut seen = vec![false; n];
        for k in 0..header.bucket_count as usize {
            let at = HEADER_LEN + k * BUCKET_SIZE;
            let b = index.get(at..at + BUCKET_SIZE)?;
            let canon = u32::from_le_bytes(b[0..4].try_into().unwrap());
            let this = u32::from_le_bytes(b[4..8].try_into().unwrap());
            let third = u32::from_le_bytes(b[8..12].try_into().unwrap());
            if canon == EMPTY && this == EMPTY && third == EMPTY {
                continue;
            }
            if third != EMPTY {
                return None; // occupied buckets always carry the 0xFFFFFFFF marker
            }
            let (ti, ci) = (this as usize, canon as usize);
            if ti >= n || ci >= n || seen[ti] {
                return None; // out of range, or two buckets claim one record
            }
            seen[ti] = true;
            canonical[ti] = canon;
        }
        Some(Buckets { canonical })
    }

    /// The record holding `id`'s bytes (itself when not aliased).
    pub fn canonical(&self, id: usize) -> usize {
        self.canonical.get(id).map(|&c| c as usize).unwrap_or(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Header: xdic magic, then u32 words. word2 = bucket count, word3 = record
    // count, word4 mirrors word3.
    fn header_bytes(buckets: u32, records: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"xdic");
        b.extend_from_slice(&0u32.to_le_bytes()); // word1
        b.extend_from_slice(&buckets.to_le_bytes()); // word2
        b.extend_from_slice(&records.to_le_bytes()); // word3
        b.extend_from_slice(&records.to_le_bytes()); // word4 (mirror)
        b
    }

    #[test]
    fn parses_header_and_record_offset() {
        let h = Header::parse(&header_bytes(8, 35)).unwrap();
        assert_eq!(h.bucket_count, 8);
        assert_eq!(h.record_count, 35);
        assert_eq!(h.record_array_offset().unwrap(), 20 + 8 * 12);
    }

    #[test]
    fn rejects_foreign_magic() {
        let mut b = header_bytes(1, 1);
        b[0] = b'X';
        assert!(Header::parse(&b).is_err());
    }

    #[test]
    fn parses_a_record_and_detects_alias() {
        // usize=248, csize=55, offset=12964, flags=1
        let mut r = Vec::new();
        r.extend_from_slice(&248u32.to_le_bytes());
        r.extend_from_slice(&55u32.to_le_bytes());
        r.extend_from_slice(&12964u64.to_le_bytes());
        r.extend_from_slice(&1u64.to_le_bytes());
        let rec = Record::parse_at(&r, 0).unwrap();
        assert_eq!(
            (rec.usize_len, rec.csize, rec.store0_offset),
            (248, 55, 12964)
        );
        assert!(!rec.is_alias(0));

        // alias marker: store0_offset == 0 && csize == own id
        let mut a = Vec::new();
        a.extend_from_slice(&248u32.to_le_bytes());
        a.extend_from_slice(&6u32.to_le_bytes()); // csize == id 6
        a.extend_from_slice(&0u64.to_le_bytes()); // offset 0
        a.extend_from_slice(&0u64.to_le_bytes());
        assert!(Record::parse_at(&a, 0).unwrap().is_alias(6));
    }

    #[test]
    fn bucket_table_maps_alias_to_canonical() {
        // 8 buckets, one occupied: canonical=2, this=6.
        let header = Header::parse(&header_bytes(8, 10)).unwrap();
        let mut index = header_bytes(8, 10);
        index.resize(20 + 8 * 12, 0xFF); // all-FF empty buckets
        let at = 20 + 3 * 12; // put the entry in bucket slot 3
        index[at..at + 4].copy_from_slice(&2u32.to_le_bytes()); // canonical
        index[at + 4..at + 8].copy_from_slice(&6u32.to_le_bytes()); // this
        index[at + 8..at + 12].copy_from_slice(&u32::MAX.to_le_bytes()); // marker
        let buckets = Buckets::build(&index, &header).unwrap();
        assert_eq!(buckets.canonical(6), 2);
        assert_eq!(buckets.canonical(0), 0); // untouched records map to themselves
    }

    #[test]
    fn invalid_bucket_table_is_absent() {
        let header = Header::parse(&header_bytes(8, 10)).unwrap();
        let mut index = header_bytes(8, 10);
        index.resize(20 + 8 * 12, 0xFF);
        let at = 20;
        index[at..at + 4].copy_from_slice(&99u32.to_le_bytes()); // canonical out of range
        index[at + 4..at + 8].copy_from_slice(&1u32.to_le_bytes());
        index[at + 8..at + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Buckets::build(&index, &header).is_none());
    }

    #[test]
    fn a_partially_empty_bucket_is_rejected_not_skipped() {
        // A bucket that is neither fully empty (all three words 0xFFFFFFFF) nor
        // a valid occupied entry - `canonical` empty but `this` set - must
        // invalidate the table (None), not be mistaken for an empty slot and
        // silently skipped.
        let header = Header::parse(&header_bytes(1, 3)).unwrap();
        let mut index = header_bytes(1, 3);
        index.extend_from_slice(&u32::MAX.to_le_bytes()); // canonical = EMPTY
        index.extend_from_slice(&1u32.to_le_bytes()); // this = 1 (set)
        index.extend_from_slice(&u32::MAX.to_le_bytes()); // third = EMPTY
        assert!(Buckets::build(&index, &header).is_none());
    }
}
