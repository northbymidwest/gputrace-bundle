//! Session-free reader for a `.gputrace` bundle's static texture manifest.
//!
//! Parses the `xdic` index and inflates 248-byte texture descriptors from the
//! `store0` zlib streams. No replayer, no `unsafe`, no Metal linkage. Learned
//! from `gpu-trace-parse-rs`'s format reverse engineering; implemented fresh.

mod descriptor;
mod index;
mod store;

pub use descriptor::TextureDescriptor;

use std::path::{Path, PathBuf};

use descriptor::{DESC_SIZE, derive_tag, descriptor_tag, parse_descriptor};
use index::{Buckets, Header, RECORD_SIZE, Record};

/// Errors from opening/parsing a bundle manifest.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The path is not a `.gputrace` directory.
    #[error("not a .gputrace bundle directory: {0}")]
    NotABundle(PathBuf),
    /// A required member (`index`/`store0`) is missing.
    #[error("bundle missing required member `{member}`")]
    MissingMember { member: &'static str },
    /// I/O error reading a member.
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The `index` file is malformed.
    #[error("malformed index: {0}")]
    BadIndex(&'static str),
}

/// An opened `.gputrace` texture manifest.
#[derive(Debug)]
pub struct Bundle {
    name: String,
    textures: Vec<TextureDescriptor>,
}

impl Bundle {
    pub fn open(path: &Path) -> Result<Bundle, Error> {
        if !path.is_dir() {
            return Err(Error::NotABundle(path.to_path_buf()));
        }
        let index = read_member(path, "index")?;
        let store0 = read_member(path, "store0")?;

        let header = Header::parse(&index)?;
        let rec_off = header.record_array_offset()?;
        // `record_count` is untrusted (a u32 straight from the header); bound
        // it by the actual index size before trusting it to size an
        // allocation - a huge claimed count with a short index is rejected
        // here rather than driving a huge `Vec::with_capacity`.
        let record_array_len = (header.record_count as usize)
            .checked_mul(RECORD_SIZE)
            .and_then(|len| rec_off.checked_add(len))
            .ok_or(Error::BadIndex("record array exceeds index size"))?;
        if record_array_len > index.len() {
            return Err(Error::BadIndex("record array exceeds index size"));
        }
        let mut records = Vec::with_capacity(header.record_count as usize);
        for i in 0..header.record_count as usize {
            let r = Record::parse_at(&index, rec_off + i * RECORD_SIZE)
                .ok_or(Error::BadIndex("record array truncated"))?;
            records.push(r);
        }
        let buckets = Buckets::build(&index, &header);
        let canonical = |id: usize| buckets.as_ref().map(|b| b.canonical(id)).unwrap_or(id);

        // Collect every 248-byte record's payload (alias-resolved), with the
        // canonical record's store0 offset.
        let mut payloads: Vec<(u64, Vec<u8>)> = Vec::new();
        for (id, r) in records.iter().enumerate() {
            if r.usize_len as usize != DESC_SIZE {
                continue;
            }
            let cid = canonical(id);
            let cr = match records.get(cid) {
                // canonical must hold real bytes, and be a 248-byte descriptor
                // itself - otherwise an alias could redirect to a record with
                // an arbitrary `usize_len`, forcing a huge transient
                // allocation in `store::read_extent`.
                Some(cr) if !cr.is_alias(cid) && cr.usize_len as usize == DESC_SIZE => cr,
                _ => continue,
            };
            if let Some(p) = store::read_extent(&store0, cr.store0_offset, cr.csize, cr.usize_len)
                && p.len() == DESC_SIZE
            {
                payloads.push((cr.store0_offset, p));
            }
        }

        let tag = derive_tag(
            &payloads
                .iter()
                .map(|(_, p)| descriptor_tag(p))
                .collect::<Vec<_>>(),
        );
        let mut textures: Vec<TextureDescriptor> = payloads
            .iter()
            .filter(|(_, p)| Some(descriptor_tag(p)) == tag)
            .map(|(off, p)| parse_descriptor(p, *off))
            .collect();
        textures.sort_by_key(|d| d.store0_offset);

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("trace")
            .to_string();
        Ok(Bundle { name, textures })
    }

    /// Texture descriptors, sorted by `store0_offset` ascending (the bridge
    /// order that corresponds rank-for-rank with ascending fetch streamRefs).
    pub fn textures(&self) -> &[TextureDescriptor] {
        &self.textures
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

fn read_member(dir: &Path, member: &'static str) -> Result<Vec<u8>, Error> {
    let p = dir.join(member);
    if !p.exists() {
        return Err(Error::MissingMember { member });
    }
    std::fs::read(&p).map_err(|source| Error::Io { path: p, source })
}
