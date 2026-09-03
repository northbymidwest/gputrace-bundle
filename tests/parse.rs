use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

// 248-byte descriptor: tag at word 0, then fields.
fn desc(tag: u64, fields: &[(usize, u64)]) -> Vec<u8> {
    let mut p = vec![0u8; 248];
    let mut set = |i: usize, v: u64| p[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
    set(0, tag);
    for &(i, v) in fields {
        set(i, v);
    }
    p
}

#[test]
fn opens_parses_resolves_alias_and_sorts() {
    let tmp = std::env::temp_dir().join(format!("bundle_parse_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("synthetic.gputrace");
    std::fs::create_dir_all(&dir).unwrap();

    // Two texture descriptors (distinct dims), one bulk payload record.
    // record 0: canonical 64x64 texture (tag 0xE1, format 80, mip 3)
    // record 1: 128x128 texture (tag 0xE1, format 80, mip 1) -- HIGHER store0 offset
    // record 2: alias -> record 0 (store0_offset 0, csize == id 2)
    // record 3: bulk (non-248) record
    let d0 = desc(0xE1, &[(2, 80), (3, 64), (4, 64), (6, 3), (7, 1), (1, 2)]);
    let d1 = desc(0xE1, &[(2, 80), (3, 128), (4, 128), (6, 1), (7, 1), (1, 2)]);
    let bulk = b"not a descriptor".repeat(3);

    let c0 = zlib(&d0);
    let c1 = zlib(&d1);
    let cb = zlib(&bulk);
    // store0 layout: d1 first (lower offset) then d0 (higher) to prove sorting.
    let mut store0 = Vec::new();
    let off_d1 = store0.len() as u64;
    store0.extend_from_slice(&c1);
    let off_d0 = store0.len() as u64;
    store0.extend_from_slice(&c0);
    let off_bulk = store0.len() as u64;
    store0.extend_from_slice(&cb);

    // index: header (buckets=4, records=4), bucket table, record array.
    let buckets = 4u32;
    let records = 4u32;
    let mut index = Vec::new();
    index.extend_from_slice(b"xdic");
    index.extend_from_slice(&0u32.to_le_bytes());
    index.extend_from_slice(&buckets.to_le_bytes());
    index.extend_from_slice(&records.to_le_bytes());
    index.extend_from_slice(&records.to_le_bytes());
    // bucket table: all-empty except one entry mapping alias id 2 -> canonical 0
    let mut btable = vec![0xFFu8; buckets as usize * 12];
    btable[0..4].copy_from_slice(&0u32.to_le_bytes()); // canonical 0
    btable[4..8].copy_from_slice(&2u32.to_le_bytes()); // this = 2 (the alias)
    btable[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    index.extend_from_slice(&btable);
    // record array (24 bytes each): usize, csize, store0_offset, flags
    let mut rec = |usz: u32, cs: u32, off: u64| {
        index.extend_from_slice(&usz.to_le_bytes());
        index.extend_from_slice(&cs.to_le_bytes());
        index.extend_from_slice(&off.to_le_bytes());
        index.extend_from_slice(&0u64.to_le_bytes());
    };
    rec(248, c0.len() as u32, off_d0); // record 0 = d0
    rec(248, c1.len() as u32, off_d1); // record 1 = d1
    rec(248, 2, 0); // record 2 = alias of 0 (store0_offset 0, csize == id 2)
    rec(bulk.len() as u32, cb.len() as u32, off_bulk); // record 3 = bulk

    std::fs::write(dir.join("index"), &index).unwrap();
    std::fs::write(dir.join("store0"), &store0).unwrap();

    let bundle = gputrace_bundle::Bundle::open(&dir).unwrap();
    let t = bundle.textures();
    // Three texture descriptors (record 0, record 1, alias->0), sorted by
    // store0_offset ascending: d1(off lower) first, then the two copies of d0.
    assert_eq!(bundle.texture_count(), 3);
    assert_eq!(t[0].width, 128); // lowest store0 offset
    assert_eq!(t[1].width, 64);
    assert_eq!(t[2].width, 64); // the alias resolved to d0's bytes
    assert!(
        t.windows(2)
            .all(|w| w[0].store0_offset <= w[1].store0_offset)
    );
    assert_eq!(bundle.name(), "synthetic");
}

/// An alias record redirecting to a canonical record whose own `usize_len` is
/// not 248 must be skipped, not read: a crafted bucket table could otherwise
/// redirect to a canonical record claiming an arbitrary `usize_len`, forcing
/// a huge transient allocation in `store::read_extent`. The bundle should
/// still open, just without that descriptor.
#[test]
fn alias_to_a_bogus_sized_canonical_is_skipped_not_read() {
    let tmp = std::env::temp_dir().join(format!("bundle_parse_alias_bogus_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("synthetic.gputrace");
    std::fs::create_dir_all(&dir).unwrap();

    let buckets = 1u32;
    let records = 2u32;
    let mut index = Vec::new();
    index.extend_from_slice(b"xdic");
    index.extend_from_slice(&0u32.to_le_bytes());
    index.extend_from_slice(&buckets.to_le_bytes());
    index.extend_from_slice(&records.to_le_bytes());
    index.extend_from_slice(&records.to_le_bytes());
    // bucket table: alias id 1 -> canonical id 0
    let mut btable = vec![0xFFu8; buckets as usize * 12];
    btable[0..4].copy_from_slice(&0u32.to_le_bytes()); // canonical
    btable[4..8].copy_from_slice(&1u32.to_le_bytes()); // this = 1 (the alias)
    btable[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    index.extend_from_slice(&btable);
    // record 0: the canonical target, claiming a huge (bogus) usize_len -
    // never a real 248-byte descriptor. Nonzero store0_offset so it doesn't
    // itself look like an alias marker.
    index.extend_from_slice(&u32::MAX.to_le_bytes()); // usize_len
    index.extend_from_slice(&1u32.to_le_bytes()); // csize
    index.extend_from_slice(&5u64.to_le_bytes()); // store0_offset
    index.extend_from_slice(&0u64.to_le_bytes()); // flags
    // record 1: the alias, a normal 248-byte record so it passes the outer
    // filter and reaches the canonical-record check.
    index.extend_from_slice(&248u32.to_le_bytes()); // usize_len
    index.extend_from_slice(&0u32.to_le_bytes()); // csize
    index.extend_from_slice(&0u64.to_le_bytes()); // store0_offset
    index.extend_from_slice(&0u64.to_le_bytes()); // flags

    std::fs::write(dir.join("index"), &index).unwrap();
    std::fs::write(dir.join("store0"), []).unwrap();

    // Opens without panicking or attempting a huge allocation; the bogus
    // descriptor is simply absent.
    let bundle = gputrace_bundle::Bundle::open(&dir).unwrap();
    assert_eq!(bundle.texture_count(), 0);
}

/// A header claiming a huge `record_count` against a short index must be
/// rejected before it drives an allocation sized by that untrusted count.
#[test]
fn oversized_record_count_against_short_index_is_rejected() {
    let tmp = std::env::temp_dir().join(format!("bundle_parse_bad_count_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("synthetic.gputrace");
    std::fs::create_dir_all(&dir).unwrap();

    // Header only (20 bytes): no bucket table, no record array, but claims a
    // huge record count.
    let mut index = Vec::new();
    index.extend_from_slice(b"xdic");
    index.extend_from_slice(&0u32.to_le_bytes());
    index.extend_from_slice(&0u32.to_le_bytes()); // bucket_count = 0
    let huge = 0xFFFF_FFF0u32;
    index.extend_from_slice(&huge.to_le_bytes()); // record_count (untrusted)
    index.extend_from_slice(&huge.to_le_bytes());

    std::fs::write(dir.join("index"), &index).unwrap();
    std::fs::write(dir.join("store0"), []).unwrap();

    let err = gputrace_bundle::Bundle::open(&dir).unwrap_err();
    assert!(matches!(err, gputrace_bundle::Error::BadIndex(_)));
}

/// Trailing bytes after the record array (index padding) must be tolerated:
/// the record array occupying LESS than the whole index is normal; only
/// claiming MORE than the index holds is the error.
#[test]
fn trailing_bytes_after_the_record_array_are_tolerated() {
    let tmp = std::env::temp_dir().join(format!("bundle_parse_trailing_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("synthetic.gputrace");
    std::fs::create_dir_all(&dir).unwrap();

    let d = desc(0xE1, &[(2, 80), (3, 64), (4, 64), (6, 1), (7, 1)]);
    let c = zlib(&d);
    let store0 = c.clone(); // one stream at offset 0

    let mut index = Vec::new();
    index.extend_from_slice(b"xdic");
    index.extend_from_slice(&0u32.to_le_bytes());
    index.extend_from_slice(&0u32.to_le_bytes()); // bucket_count = 0
    index.extend_from_slice(&1u32.to_le_bytes()); // record_count = 1
    index.extend_from_slice(&1u32.to_le_bytes());
    index.extend_from_slice(&248u32.to_le_bytes()); // record 0: usize_len
    index.extend_from_slice(&(c.len() as u32).to_le_bytes()); // csize
    index.extend_from_slice(&0u64.to_le_bytes()); // store0_offset
    index.extend_from_slice(&0u64.to_le_bytes()); // flags
    // Padding beyond the record array: record_array_len < index.len().
    index.extend_from_slice(&[0u8; 16]);

    std::fs::write(dir.join("index"), &index).unwrap();
    std::fs::write(dir.join("store0"), &store0).unwrap();

    let bundle = gputrace_bundle::Bundle::open(&dir).unwrap();
    assert_eq!(bundle.texture_count(), 1);
}

/// The parser skips a canonical record that is itself an alias marker
/// (store0_offset 0, csize == its own id). Even when that record's csize
/// happens to equal a real, decompressible stream's length at offset 0, an
/// alias marker is not a payload and must not be turned into a texture.
#[test]
fn alias_whose_canonical_is_itself_an_alias_yields_no_texture() {
    let tmp = std::env::temp_dir().join(format!("bundle_parse_alias2_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("synthetic.gputrace");
    std::fs::create_dir_all(&dir).unwrap();

    let d = desc(0xE1, &[(2, 80), (3, 64), (4, 64), (6, 1), (7, 1)]);
    let comp = zlib(&d);
    let l = comp.len(); // the canonical alias sits at record index `l`
    let records = l + 1;

    // store0: the real 248-byte stream at offset 0, `l` bytes long.
    let store0 = comp.clone();

    let mut index = Vec::new();
    index.extend_from_slice(b"xdic");
    index.extend_from_slice(&0u32.to_le_bytes());
    index.extend_from_slice(&1u32.to_le_bytes()); // bucket_count = 1
    index.extend_from_slice(&(records as u32).to_le_bytes());
    index.extend_from_slice(&(records as u32).to_le_bytes());
    // one bucket: record 1 -> canonical `l`
    index.extend_from_slice(&(l as u32).to_le_bytes()); // canonical
    index.extend_from_slice(&1u32.to_le_bytes()); // this = 1
    index.extend_from_slice(&u32::MAX.to_le_bytes()); // marker
    for i in 0..records {
        let (usz, cs, off): (u32, u32, u64) = if i == l {
            // canonical `l`: an alias marker (offset 0, csize == its id `l`),
            // and csize == comp.len() so read_extent WOULD succeed if the guard
            // let it through.
            (248, l as u32, 0)
        } else if i == 1 {
            // the aliased record: 248-sized, not itself an alias.
            (248, 5, 100)
        } else {
            (0, 0, 0) // filler: usize_len != 248, filtered out
        };
        index.extend_from_slice(&usz.to_le_bytes());
        index.extend_from_slice(&cs.to_le_bytes());
        index.extend_from_slice(&off.to_le_bytes());
        index.extend_from_slice(&0u64.to_le_bytes());
    }

    std::fs::write(dir.join("index"), &index).unwrap();
    std::fs::write(dir.join("store0"), &store0).unwrap();

    // Original: record 1's canonical (l) is an alias -> skipped; record l is
    // itself an alias -> skipped. No textures. A bypassed guard would read
    // record l's stream and produce one.
    let bundle = gputrace_bundle::Bundle::open(&dir).unwrap();
    assert_eq!(bundle.texture_count(), 0);
}
