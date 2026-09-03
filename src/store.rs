//! Random access to the zlib streams concatenated in `store0`. Each record's
//! extent holds exactly one stream; this refuses to read into the next one.

use flate2::read::ZlibDecoder;
use std::io::Read;

/// Inflate the single zlib stream at `store0[offset .. offset + csize]`, which
/// must decompress to exactly `usize_len` bytes. Returns `None` on any
/// bounds/inflate/size failure (the caller skips such a record).
pub(crate) fn read_extent(
    store0: &[u8],
    offset: u64,
    csize: u32,
    usize_len: u32,
) -> Option<Vec<u8>> {
    let start = usize::try_from(offset).ok()?;
    let end = start.checked_add(csize as usize)?;
    let extent = store0.get(start..end)?;
    let mut dec = ZlibDecoder::new(extent);
    let mut out = vec![0u8; usize_len as usize];
    dec.read_exact(&mut out).ok()?;
    // The stream must end exactly here: a further byte would mean it inflated
    // past the declared size (or ran into a following stream).
    let mut extra = [0u8; 1];
    match dec.read(&mut extra) {
        Ok(0) => Some(out),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::read_extent;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn reads_one_stream_at_an_offset() {
        let payload = b"the descriptor bytes".repeat(4);
        let comp = zlib(&payload);
        let mut store0 = vec![0u8; 7]; // a leading gap
        let off = store0.len() as u64;
        store0.extend_from_slice(&comp);
        let got = read_extent(&store0, off, comp.len() as u32, payload.len() as u32);
        assert_eq!(got.as_deref(), Some(&payload[..]));
    }

    #[test]
    fn rejects_out_of_range_extent() {
        assert_eq!(read_extent(&[0u8; 4], 2, 100, 8), None);
    }

    #[test]
    fn rejects_a_size_mismatch() {
        let comp = zlib(&[1u8, 2, 3]);
        // declare 99 bytes uncompressed when only 3 inflate
        assert_eq!(read_extent(&comp, 0, comp.len() as u32, 99), None);
    }
}
