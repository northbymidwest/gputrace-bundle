//! Lightweight corpus fuzz for the parser.
//!
//! Feeds `Bundle::open` a large batch of adversarial inputs (random mutations
//! of structurally-valid bundles, plus pure-random bytes) and asserts it is a
//! total function: it returns `Ok` or `Err`, and never panics or drives an
//! unbounded allocation. The property, not a value oracle - malformed input is
//! expected to be rejected, just never fatally.
//!
//! Deterministic from `FUZZ_SEED` so any failure reproduces exactly:
//!
//!   FUZZ_SEED=<n> cargo test --test fuzz -- --nocapture
//!
//! `FUZZ_ITERS` overrides the iteration count (default 2000).

use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

/// xorshift64* - a tiny deterministic PRNG, so the fuzz needs no `rand` dep.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// A value in `0..n`. `n` must be non-zero.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    fn byte(&mut self) -> u8 {
        self.next_u64() as u8
    }
    fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A structurally-valid `(index, store0)` pair with an rng-chosen shape: the
/// deep-coverage seed the mutator then corrupts.
fn valid_bundle(rng: &mut Rng) -> (Vec<u8>, Vec<u8>) {
    let records = 1 + rng.below(6);
    let bucket_count = rng.below(3) as u32;

    // store0: a zlib stream per record; remember each offset and compressed len.
    let mut store0 = Vec::new();
    let mut placed: Vec<(u64, u32)> = Vec::new();
    for _ in 0..records {
        // A 248-byte descriptor with plausible fields (tag at word 0).
        let mut d = vec![0u8; 248];
        let mut set = |i: usize, v: u64| d[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        set(0, 0xE1);
        set(2, 70 + rng.below(20) as u64); // format
        set(3, 1 + rng.below(512) as u64); // width
        set(4, 1 + rng.below(512) as u64); // height
        set(6, 1 + rng.below(12) as u64); // mip levels
        set(7, 1);
        let c = zlib(&d);
        let off = store0.len() as u64;
        let comp_len = c.len() as u32;
        store0.extend_from_slice(&c);
        placed.push((off, comp_len));
    }

    let mut index = Vec::new();
    index.extend_from_slice(b"xdic");
    index.extend_from_slice(&0u32.to_le_bytes());
    index.extend_from_slice(&bucket_count.to_le_bytes());
    index.extend_from_slice(&(records as u32).to_le_bytes());
    index.extend_from_slice(&(records as u32).to_le_bytes());
    // Bucket table: 0xFF padding (no alias mappings, every id self-canonical).
    index.extend(std::iter::repeat_n(0xFFu8, bucket_count as usize * 12));
    // Record array: usize_len, csize, store0_offset, flags (24 bytes each).
    for &(off, comp_len) in &placed {
        index.extend_from_slice(&248u32.to_le_bytes());
        index.extend_from_slice(&comp_len.to_le_bytes());
        index.extend_from_slice(&off.to_le_bytes());
        index.extend_from_slice(&0u64.to_le_bytes());
    }
    (index, store0)
}

/// Corrupt a buffer in place with a handful of random edits.
fn mutate(rng: &mut Rng, buf: &mut Vec<u8>) {
    let ops = 1 + rng.below(8);
    for _ in 0..ops {
        if buf.is_empty() {
            return;
        }
        match rng.below(4) {
            0 => {
                let i = rng.below(buf.len());
                buf[i] = rng.byte();
            }
            1 => {
                let i = rng.below(buf.len());
                buf.truncate(i);
            }
            2 => {
                let i = rng.below(buf.len());
                buf.insert(i, rng.byte());
            }
            _ => {
                let i = rng.below(buf.len());
                buf[i] = buf[i].wrapping_add(1);
            }
        }
    }
}

#[test]
fn open_is_total_over_adversarial_input() {
    let seed = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(1)
        });
    let iters = std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2000);
    // Always printed, so a CI failure reports the seed to reproduce with.
    eprintln!("FUZZ_SEED={seed} FUZZ_ITERS={iters}");

    let mut rng = Rng(seed | 1);
    let tmp = std::env::temp_dir().join(format!("gputrace_bundle_fuzz_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("fuzz.gputrace");
    std::fs::create_dir_all(&dir).unwrap();
    let index_p = dir.join("index");
    let store_p = dir.join("store0");

    for _ in 0..iters {
        let (mut index, mut store0) = if rng.coin() {
            valid_bundle(&mut rng)
        } else {
            let il = rng.below(256);
            let sl = rng.below(256);
            (
                (0..il).map(|_| rng.byte()).collect(),
                (0..sl).map(|_| rng.byte()).collect(),
            )
        };
        if rng.coin() {
            mutate(&mut rng, &mut index);
        }
        if rng.coin() {
            mutate(&mut rng, &mut store0);
        }
        std::fs::write(&index_p, &index).unwrap();
        std::fs::write(&store_p, &store0).unwrap();

        // The whole point: this must return, not panic.
        let _ = gputrace_bundle::Bundle::open(&dir);
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
