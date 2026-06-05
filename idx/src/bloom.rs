/// Bloom filter layer over the columnar index.
///
/// Purpose: fast existence checks and prefix queries BEFORE hitting the index.
///
///   "Does any file with extension .rs exist?" → bloom check → O(k) hash ops
///   "Does path src/main.rs exist?"            → bloom check → O(k) hash ops
///
/// False positives are fine — we fall through to the index for confirmation.
/// False negatives are impossible — if bloom says no, the file isn't there.
///
/// We maintain two bloom filters:
///   1. filename_bloom  — over bare filenames (main.rs, Cargo.toml)
///   2. ext_bloom       — over extensions (rs, toml, json)
///
/// This lets us answer "does any .proto file exist in this tree?" in microseconds
/// before doing any index scan, which is the hot path for agent tool calls like
/// "find all .proto files" on an unfamiliar repo.

use bloomfilter::Bloom;
use serde::{Deserialize, Serialize};

/// Serializable wrapper around two bloom filters.
/// bloomfilter::Bloom isn't Serialize, so we store the raw bitmap.
#[derive(Serialize, Deserialize)]
pub struct IndexBloom {
    // Filename bloom (bare names, e.g. "main.rs")
    filename_bitmap: Vec<u8>,
    filename_bits:   usize,
    filename_k:      u32,
    filename_sip_keys: [(u64, u64); 2],

    // Extension bloom (e.g. "rs", "toml")
    ext_bitmap: Vec<u8>,
    ext_bits:   usize,
    ext_k:      u32,
    ext_sip_keys: [(u64, u64); 2],

    // Path prefix bloom (first two path components, e.g. "src/lib")
    prefix_bitmap: Vec<u8>,
    prefix_bits:   usize,
    prefix_k:      u32,
    prefix_sip_keys: [(u64, u64); 2],
}

/// In-memory bloom state (rebuilt from IndexBloom on load)
pub struct BloomSet {
    pub filename: Bloom<str>,
    pub ext:      Bloom<str>,
    pub prefix:   Bloom<str>,
}

impl BloomSet {
    /// Create fresh bloom filters sized for `expected_items` files.
    /// False positive rate: 0.1% (1 in 1000)
    pub fn new(expected_items: usize) -> Self {
        let n = expected_items.max(1000);
        BloomSet {
            filename: Bloom::new_for_fp_rate(n, 0.001),
            ext:      Bloom::new_for_fp_rate(n, 0.001),
            prefix:   Bloom::new_for_fp_rate(n, 0.001),
        }
    }

    /// Insert a file path into all relevant bloom filters.
    pub fn insert(&mut self, path: &str) {
        use std::path::Path;
        let p = Path::new(path);

        // Filename
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            self.filename.set(name);
        }

        // Extension
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            self.ext.set(ext);
        }

        // Path prefix (first 1-2 components for subtree pruning)
        let components: Vec<&str> = p
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();

        // Each individual component (so a bare-directory prefix like "src"
        // matches) ...
        for component in &components {
            self.prefix.set(component);
        }
        // ... plus joined pairs for two-component subtree prefixes.
        for window in components.windows(2) {
            let prefix = format!("{}/{}", window[0], window[1]);
            self.prefix.set(prefix.as_str());
        }
    }

    /// Check if a filename MIGHT exist. Returns false → definitely not present.
    pub fn might_have_filename(&self, name: &str) -> bool {
        self.filename.check(name)
    }

    /// Check if an extension MIGHT exist in the tree.
    pub fn might_have_ext(&self, ext: &str) -> bool {
        self.ext.check(ext)
    }

    /// Check if a path prefix MIGHT contain files.
    pub fn might_have_prefix(&self, prefix: &str) -> bool {
        self.prefix.check(prefix)
    }
}

/// Extract bloom state for serialization.
/// We rebuild BloomSet from the columnar index on load, so this is
/// only needed for the daemon's checkpoint saves.
pub fn serialize_bloom(bloom: &BloomSet) -> Vec<u8> {
    // Serialize the raw bit arrays via the internal bitmap access
    // bloomfilter exposes bitmap() -> &[u8]
    let mut out = Vec::new();

    // filename
    let fb = bloom.filename.bitmap();
    out.extend_from_slice(&(fb.len() as u64).to_le_bytes());
    out.extend_from_slice(&fb);

    // ext
    let eb = bloom.ext.bitmap();
    out.extend_from_slice(&(eb.len() as u64).to_le_bytes());
    out.extend_from_slice(&eb);

    // prefix
    let pb = bloom.prefix.bitmap();
    out.extend_from_slice(&(pb.len() as u64).to_le_bytes());
    out.extend_from_slice(&pb);

    out
}

/// Rebuild BloomSet from a columnar index (faster than deserializing raw bitmaps).
/// Called on daemon startup after loading the index from disk.
pub fn rebuild_bloom(paths: &[String], expected: usize) -> BloomSet {
    let mut bloom = BloomSet::new(expected);
    for path in paths {
        bloom.insert(path);
    }
    bloom
}
