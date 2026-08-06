//! Minimal GGUF metadata reader — just the header fields the router's VRAM-fit
//! calc and model labels need (n_layers, n_kv_heads, head_dim, context length,
//! quant). Reads only the metadata KV table at the start of the file, seeking
//! past large arrays (tokenizer vocab/merges) rather than loading them, so it's
//! cheap even on multi-GB files.
//!
//! GGUF layout: magic "GGUF", u32 version, u64 tensor_count, u64 kv_count, then
//! kv_count entries of {key: gguf-string, type: u32, value: typed}.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};

#[derive(Debug, Clone)]
pub struct GgufMeta {
    pub architecture: String,
    /// The chat template declares tool support (contains a tools branch).
    pub template_tools: bool,
    /// The chat template hard-rejects non-alternating conversations -
    /// agent scaffolding (system inserts, tool turns) makes it throw, so
    /// such a model cannot drive agent sessions regardless of its family.
    pub template_strict_alternation: bool,
    /// `general.size_label` — the converter's param-size tag (e.g. "3B",
    /// "4.6B", "mini"); "" if absent. Used to fill an otherwise-Unknown label.
    pub size_label: String,
    pub n_layers: u64,
    pub n_heads: u64,
    pub n_kv_heads: u64,
    pub embedding_length: u64,
    pub context_length: u64,
    /// Explicit per-head dim (`attention.key_length`); 0 if the model doesn't
    /// set it (then derive from embedding_length / n_heads).
    pub key_length: u64,
    /// `general.file_type` enum → mapped to effective bits-per-weight below.
    pub file_type: u32,
}

impl GgufMeta {
    /// Can this file's template host an agent conversation (tools present,
    /// no strict-alternation guard)?
    pub fn agent_template_ok(&self) -> bool {
        self.template_tools && !self.template_strict_alternation
    }

    /// Per-head dimension — explicit `key_length` if set (e.g. Gemma), else
    /// embedding_length / n_heads.
    pub fn head_dim(&self) -> u64 {
        if self.key_length > 0 {
            self.key_length
        } else if self.n_heads == 0 {
            0
        } else {
            self.embedding_length / self.n_heads
        }
    }

    /// A human param-count label derived from the header, e.g. "8B", "270M".
    /// (Filename parsing often yields "Unknown"; this is exact-ish from tensors —
    /// but we don't read tensor sizes here, so approximate from the file when
    /// the caller has size_bytes; left to the caller. Returns "" here.)
    pub fn quant_label(&self) -> &'static str {
        file_type_name(self.file_type)
    }

    /// Effective bits-per-weight for the quant, for the weights-size estimate.
    pub fn effective_bpw(&self) -> f64 {
        file_type_bpw(self.file_type)
    }

    /// Estimate the VRAM (MiB) to fully load + run this model at `ctx` tokens:
    /// quantized weights (≈ the GGUF file size) + the KV cache + a fixed
    /// compute/context overhead. Grounded in the header (n_layers, n_kv_heads,
    /// head_dim) rather than file size alone — the shape oobabooga's empirical
    /// GGUF-VRAM fit uses. It's the TEXT footprint; the per-image vision buffer is
    /// left to the runtime fallback (it's small and only some loads pay it).
    pub fn estimate_vram_mib(&self, file_size_bytes: u64, ctx: u64) -> u64 {
        const MIB: u64 = 1024 * 1024;
        let weights = file_size_bytes / MIB;
        // KV cache, f16 (llama.cpp default): 2 (K+V) × layers × kv_heads × head_dim
        // × ctx × 2 bytes. This is what makes long contexts eat VRAM.
        let kv = (2 * self.n_layers * self.n_kv_heads * self.head_dim() * ctx * 2) / MIB;
        // Compute buffers + graphics-context + empirical safety margin (~0.6 GiB).
        const OVERHEAD_MIB: u64 = 640;
        weights + kv + OVERHEAD_MIB
    }

    /// True for encoder/embedding models (BERT family — e.g. bge, nomic-bert,
    /// jina-bert). These can't do causal generation, so the chat router must
    /// never pick them (loading one into the chat server 500s with "context
    /// does not [support] logits computation").
    pub fn is_embedding(&self) -> bool {
        let a = self.architecture.to_lowercase();
        a.contains("bert") || a.contains("jina") || a == "gte"
    }
}

// ── readers ───────────────────────────────────────────────────────────────

fn read_u32<R: Read>(r: &mut R) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64<R: Read>(r: &mut R) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

/// A GGUF string: u64 length + that many UTF-8 bytes.
fn read_gstr<R: Read>(r: &mut R) -> std::io::Result<String> {
    let len = read_u64(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Byte width of a fixed-size GGUF scalar type (None for string/array).
fn scalar_width(t: u32) -> Option<u64> {
    match t {
        0 | 1 | 7 => Some(1),       // u8, i8, bool
        2 | 3 => Some(2),           // u16, i16
        4 | 5 | 6 => Some(4),       // u32, i32, f32
        10 | 11 | 12 => Some(8),    // u64, i64, f64
        _ => None,
    }
}

/// Read an integer-valued scalar as u64 (for the count keys we care about).
fn read_int_value<R: Read>(r: &mut R, t: u32) -> std::io::Result<Option<u64>> {
    Ok(match t {
        0 => Some(read_n::<R, 1>(r)? as u64),
        2 => Some(read_n::<R, 2>(r)? as u64),
        4 => Some(read_n::<R, 4>(r)? as u64),
        10 => Some(read_n::<R, 8>(r)?),
        5 => Some(read_n::<R, 4>(r)? as u64), // i32 → treat as u64
        11 => Some(read_n::<R, 8>(r)?),       // i64
        _ => None,
    })
}

fn read_n<R: Read, const N: usize>(r: &mut R) -> std::io::Result<u64> {
    let mut b = [0u8; N];
    r.read_exact(&mut b)?;
    let mut v = 0u64;
    for (i, byte) in b.iter().enumerate() {
        v |= (*byte as u64) << (8 * i);
    }
    Ok(v)
}

/// Advance the cursor past a value of `t` without keeping it.
fn skip_value<R: Read + Seek>(r: &mut R, t: u32) -> std::io::Result<()> {
    match t {
        8 => {
            let len = read_u64(r)? as i64;
            r.seek(SeekFrom::Current(len))?;
        }
        9 => {
            // array: elem_type (u32), count (u64), then count elements
            let elem_t = read_u32(r)?;
            let count = read_u64(r)?;
            if elem_t == 8 {
                for _ in 0..count {
                    let len = read_u64(r)? as i64;
                    r.seek(SeekFrom::Current(len))?;
                }
            } else if let Some(w) = scalar_width(elem_t) {
                r.seek(SeekFrom::Current((w * count) as i64))?;
            } else {
                // nested array or unknown — give up gracefully
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unsupported nested array in gguf metadata",
                ));
            }
        }
        _ => {
            if let Some(w) = scalar_width(t) {
                r.seek(SeekFrom::Current(w as i64))?;
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unknown gguf value type",
                ));
            }
        }
    }
    Ok(())
}

/// Read the GGUF metadata of `path`. Errors on a non-GGUF / malformed file.
pub fn read_meta(path: &std::path::Path) -> Result<GgufMeta, String> {
    // Model files are immutable once downloaded, but their metadata blocks
    // (which include multi-MB tokenizer arrays) were re-parsed on every
    // call - and fit assessment reads every model several times per
    // routing decision, which added up to ~25s per assess on a modest
    // machine. Cache by (path, mtime, size); a re-download invalidates
    // naturally.
    use std::collections::HashMap;
    use std::sync::Mutex;
    static META_CACHE: Mutex<Option<HashMap<(std::path::PathBuf, u64, u64), GgufMeta>>> =
        Mutex::new(None);
    let stat = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?;
    let mtime = stat
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = (path.to_path_buf(), mtime, stat.len());
    if let Some(hit) = META_CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .get(&key)
    {
        return Ok(hit.clone());
    }
    let parsed = read_meta_uncached(path)?;
    META_CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, parsed.clone());
    Ok(parsed)
}

fn read_meta_uncached(path: &std::path::Path) -> Result<GgufMeta, String> {
    let f = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut r = BufReader::new(f);

    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| format!("read magic: {e}"))?;
    if &magic != b"GGUF" {
        return Err("not a GGUF file".to_string());
    }
    let _version = read_u32(&mut r).map_err(|e| e.to_string())?;
    let _tensor_count = read_u64(&mut r).map_err(|e| e.to_string())?;
    let kv_count = read_u64(&mut r).map_err(|e| e.to_string())?;

    let mut m = GgufMeta {
        template_tools: false,
        template_strict_alternation: false,
        architecture: String::new(),
        size_label: String::new(),
        n_layers: 0,
        n_heads: 0,
        n_kv_heads: 0,
        embedding_length: 0,
        context_length: 0,
        key_length: 0,
        file_type: 0,
    };

    for _ in 0..kv_count {
        let key = read_gstr(&mut r).map_err(|e| format!("read key: {e}"))?;
        let vtype = read_u32(&mut r).map_err(|e| e.to_string())?;

        // Match the keys we need by SUFFIX (the arch prefix varies: llama.*,
        // gemma3.*, qwen3.* …). Check head_count_kv before head_count.
        let want_int = key.ends_with(".block_count")
            || key.ends_with(".attention.head_count_kv")
            || key.ends_with(".attention.head_count")
            || key.ends_with(".attention.key_length")
            || key.ends_with(".embedding_length")
            || key.ends_with(".context_length")
            || key == "general.file_type";

        if key == "tokenizer.chat_template" && vtype == 8 {
            let tpl = read_gstr(&mut r).map_err(|e| e.to_string())?;
            m.template_tools = tpl.contains("tools") || tpl.contains("tool_call");
            m.template_strict_alternation =
                tpl.contains("raise_exception") && tpl.contains("alternate");
        } else if key == "general.architecture" && vtype == 8 {
            m.architecture = read_gstr(&mut r).map_err(|e| e.to_string())?;
        } else if key == "general.size_label" && vtype == 8 {
            m.size_label = read_gstr(&mut r).map_err(|e| e.to_string())?;
        } else if want_int {
            let v = read_int_value(&mut r, vtype)
                .map_err(|e| e.to_string())?
                .unwrap_or(0);
            if key.ends_with(".block_count") {
                m.n_layers = v;
            } else if key.ends_with(".attention.head_count_kv") {
                m.n_kv_heads = v;
            } else if key.ends_with(".attention.key_length") {
                m.key_length = v;
            } else if key.ends_with(".attention.head_count") {
                m.n_heads = v;
            } else if key.ends_with(".embedding_length") {
                m.embedding_length = v;
            } else if key.ends_with(".context_length") {
                m.context_length = v;
            } else if key == "general.file_type" {
                m.file_type = v as u32;
            }
        } else {
            skip_value(&mut r, vtype).map_err(|e| e.to_string())?;
        }
    }

    // Some models omit head_count_kv (no GQA) → it equals head_count.
    if m.n_kv_heads == 0 {
        m.n_kv_heads = m.n_heads;
    }
    Ok(m)
}

/// `general.file_type` enum → quant label (the common llama.cpp values).
fn file_type_name(ft: u32) -> &'static str {
    match ft {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        _ => "Unknown",
    }
}

/// Effective bits-per-weight per quant (used for the weights-size estimate).
fn file_type_bpw(ft: u32) -> f64 {
    match ft {
        0 => 32.0,
        1 => 16.0,
        2 | 3 => 4.5,   // Q4_0/Q4_1
        7 => 8.5,       // Q8_0
        8 | 9 => 5.5,   // Q5_0/Q5_1
        10 => 2.6,      // Q2_K
        11 => 3.4,      // Q3_K_S
        12 => 3.9,      // Q3_K_M
        13 => 4.3,      // Q3_K_L
        14 => 4.6,      // Q4_K_S
        15 => 4.83,     // Q4_K_M
        16 => 5.5,      // Q5_K_S
        17 => 5.67,     // Q5_K_M
        18 => 6.6,      // Q6_K
        _ => 5.0,       // unknown → middle-ish
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a real on-disk model if present (skips otherwise) and sanity-checks
    /// the parsed metadata.
    #[test]
    fn parses_a_real_model_if_available() {
        let dir = std::path::Path::new(
            "/home/solar/.local/share/com.solar.yourowai/models",
        );
        let candidates = [
            "gemma-4-E2B-it-Q4_K_M.gguf",
            "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf",
            "Phi-4-mini-instruct-Q4_K_M.gguf",
        ];
        for name in candidates {
            let p = dir.join(name);
            if !p.exists() {
                continue;
            }
            let m = read_meta(&p).expect("should parse gguf");
            eprintln!("{name}: {m:?} head_dim={} bpw={}", m.head_dim(), m.effective_bpw());
            assert!(m.n_layers > 0, "{name}: n_layers should be > 0");
            assert!(m.n_heads > 0, "{name}: n_heads should be > 0");
            assert!(m.embedding_length > 0, "{name}: embedding_length > 0");
            assert!(m.context_length > 0, "{name}: context_length > 0");
            assert!(m.n_kv_heads > 0 && m.n_kv_heads <= m.n_heads);
        }
    }

    /// Prints the grounded VRAM estimate + GPU/CPU verdict for every installed
    /// model (vs a 4279 MiB free-VRAM card). Diagnostic — run with --nocapture.
    #[test]
    fn vram_estimate_for_installed_models() {
        let dir = std::path::Path::new("/home/solar/.local/share/com.solar.yourowai/models");
        const FREE_VRAM_MIB: u64 = 4279; // the GTX 1050 Ti's free VRAM
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("gguf") { continue; }
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.contains("mmproj") { continue; }
            let Ok(m) = read_meta(&p) else { continue };
            if m.is_embedding() { continue; }
            let bytes = std::fs::metadata(&p).map(|x| x.len()).unwrap_or(0);
            let est = m.estimate_vram_mib(bytes, 8192);
            eprintln!(
                "{name}: ~{est} MiB (file {} MiB, layers {}, kv_heads {}, head_dim {}) -> {}",
                bytes / 1024 / 1024, m.n_layers, m.n_kv_heads, m.head_dim(),
                if est <= FREE_VRAM_MIB { "GPU" } else { "CPU" }
            );
        }
    }

    /// The embedding model must be flagged (it 500s the chat server), the chat
    /// models must not be — this is the offline-router crash fix.
    #[test]
    fn embedding_model_is_excluded() {
        let dir = std::path::Path::new("/home/solar/.local/share/com.solar.yourowai/models");
        let bge = dir.join("bge-small-en-v1.5-f16.gguf");
        if bge.exists() {
            let m = read_meta(&bge).unwrap();
            assert_eq!(m.architecture, "bert");
            assert!(m.is_embedding(), "bge must be flagged as embedding");
        }
        for chat in ["gemma-4-E2B-it-Q4_K_M.gguf", "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf"] {
            let p = dir.join(chat);
            if p.exists() {
                assert!(!read_meta(&p).unwrap().is_embedding(), "{chat} is a chat model");
            }
        }
    }
}
