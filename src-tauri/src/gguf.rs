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

#[derive(Debug, Clone, Default)]
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
    /// `<arch>.expert_count` - number of experts per MoE layer (0 = dense).
    pub n_experts: u64,
    /// `<arch>.expert_used_count` - experts active per token (0 = dense).
    pub n_experts_used: u64,
    /// `<arch>.expert_feed_forward_length` - width of one routed expert
    /// (0 when absent).
    pub ff_expert_len: u64,
    /// `<arch>.expert_shared_feed_forward_length` - width of the always-on
    /// shared expert (0 when absent). Together with the two counts this
    /// yields the ACTIVE FRACTION - how much of the feed-forward runs on
    /// every token - which decides whether a GPU + RAM split is fast
    /// (cold experts) or a crawl (post-hoc "surgery" MoEs that activate
    /// most of their width).
    pub ff_shared_expert_len: u64,
    /// Layers that actually carry an attention KV cache, counted from the
    /// tensor table (`blk.N.attn_k*` / `attn_qkv`). Hybrid models (Qwen 3.5
    /// family, Nemotron 3.5, Granite 4, LFM2.5) keep attention on a
    /// fraction of their layers - the rest are recurrent and hold no KV -
    /// so charging every layer over-counts their context cost several
    /// times over. 0 = unknown (table unreadable): callers use n_layers.
    pub n_attn_layers: u64,
    /// Sum of `attention.head_count_kv` over all layers when the header
    /// stores it per layer (hybrids keep 0 on recurrent layers); 0 = the
    /// header had one scalar - use n_kv_heads x attention layers.
    pub kv_heads_sum: u64,
    /// `attention.value_length` when present; 0 = same as the key length.
    pub value_length: u64,
    /// Sliding-attention window in tokens; 0 = none declared.
    pub sliding_window: u64,
    /// Layers using the sliding window, from `sliding_window_pattern`.
    pub swa_layers: u64,
    /// Whether the pattern array was present (without it, a declared
    /// window is assumed to cover every attention layer).
    pub swa_pattern_read: bool,
    pub key_length_swa: u64,
    pub value_length_swa: u64,
    /// The tool-call channel opener this model's own chat template uses
    /// (None = the template declares no such channel). Read from the
    /// template, never from the model's name.
    pub tool_call_marker: Option<&'static str>,
    /// Bytes of the token-embedding table - the engine keeps it in system
    /// memory even under full offload, so it never costs the card.
    pub embd_bytes: u64,
    /// Bytes of expert tensors per layer (index = block), from the tensor
    /// table (sizes = offset deltas, no type table needed). Empty for dense
    /// models, and for files whose tensor table could not be read - the
    /// caller then falls back to "all experts on the CPU".
    pub expert_bytes_per_layer: Vec<u64>,
    /// Bytes of everything that is not an expert tensor (attention, norms,
    /// embeddings, shared experts): what stays on the GPU under offload.
    pub non_expert_bytes: u64,
    /// Sharded GGUF (`split.count` > 1): this file's index and the shard
    /// count. `(0, 1)` for an ordinary single-file model. Shard 0 carries
    /// the metadata and IS the model (it is what `--model` takes); the
    /// others carry tensors and only the split keys - never list them.
    pub split_no: u32,
    pub split_count: u32,
    /// Total bytes of every shard (== file size for a single file).
    pub total_bytes: u64,
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

    /// A shard of a sharded model that is not the first - a part, not a
    /// model: never listed, graded, loaded or offered.
    pub fn is_secondary_shard(&self) -> bool {
        self.split_count > 1 && self.split_no > 0
    }

    /// Mixture-of-experts model: most of its weights are expert tensors that
    /// only a few of fire per token - the part llama.cpp can pin to the CPU
    /// (`--cpu-moe`) when the whole file does not fit the graphics card.
    pub fn is_moe(&self) -> bool {
        self.n_experts > 1
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

/// Largest metadata string we will ever materialize (keys, architecture,
/// chat template). Real templates run to a few hundred KB; anything past
/// this is garbage read from a damaged file, not a model.
const MAX_STRING_BYTES: u64 = 16 * 1024 * 1024;
/// Sanity ceilings for the header counts (the largest real models sit at a
/// few thousand tensors and a few hundred KV pairs).
const MAX_KV_COUNT: u64 = 1 << 16;
const MAX_TENSOR_COUNT: u64 = 1 << 20;

/// Why a file's metadata could not be read.
#[derive(Debug, Clone)]
pub enum MetaError {
    /// The bytes do not read as a GGUF file: bad magic, a length or count
    /// that points past the end of the file, truncated mid-field. A
    /// half-copied download or a corrupted file - delete and re-download.
    Damaged(String),
    /// A well-formed file using a construct this reader does not handle.
    Unsupported(String),
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetaError::Damaged(m) => write!(f, "damaged: {m}"),
            MetaError::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

fn damaged(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into())
}

/// A bounded cursor over the file: every length and count read from the
/// header is checked against the bytes that actually remain, so a damaged
/// file can never make us allocate or loop on a garbage number.
struct Bounded<R> {
    r: R,
    pos: u64,
    len: u64,
}

impl<R: Read + Seek> Bounded<R> {
    fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        if (buf.len() as u64) > self.remaining() {
            return Err(damaged("field runs past the end of the file"));
        }
        self.r.read_exact(buf)?;
        self.pos += buf.len() as u64;
        Ok(())
    }

    fn skip(&mut self, n: u64) -> std::io::Result<()> {
        if n > self.remaining() {
            return Err(damaged("value runs past the end of the file"));
        }
        self.r.seek(SeekFrom::Current(n as i64))?;
        self.pos += n;
        Ok(())
    }

    fn read_u32(&mut self) -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn read_u64(&mut self) -> std::io::Result<u64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// A GGUF string: u64 length + that many UTF-8 bytes. The length is
    /// checked against the file before anything is allocated.
    fn read_gstr(&mut self) -> std::io::Result<String> {
        let len = self.read_u64()?;
        if len > MAX_STRING_BYTES || len > self.remaining() {
            return Err(damaged(format!("string length {len} is not plausible")));
        }
        let mut buf = vec![0u8; len as usize];
        self.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Skip a GGUF string without reading it.
    fn skip_gstr(&mut self) -> std::io::Result<()> {
        let len = self.read_u64()?;
        self.skip(len)
    }

    /// An array of integer scalars, averaged (rounded up). Some models store
    /// per-layer values where others store one number - gemma4 keeps
    /// `attention.head_count_kv` per layer ([2,8,...]) - and the fit math
    /// wants one representative figure. Non-integer or oversized arrays
    /// are skipped and read as None.
    fn read_int_array_mean(&mut self) -> std::io::Result<Option<u64>> {
        let elem_t = self.read_u32()?;
        let count = self.read_u64()?;
        let Some(w) = scalar_width(elem_t) else {
            // string / nested array under an integer key: not ours
            return Err(damaged("unexpected array element type under an integer key"));
        };
        if count == 0 || count > 4096 {
            self.skip(w.checked_mul(count).ok_or_else(|| damaged("array size overflows"))?)?;
            return Ok(None);
        }
        let mut sum = 0u64;
        let mut n = 0u64;
        for _ in 0..count {
            match self.read_int_value(elem_t)? {
                Some(v) => {
                    sum += v;
                    n += 1;
                }
                None => {
                    // f32/f64/bool elements: consume and ignore
                    self.skip(w)?;
                }
            }
        }
        Ok(if n > 0 { Some((sum + n - 1) / n) } else { None })
    }

    /// Like `read_int_array_mean`, also returning the SUM - the honest
    /// figure for per-layer head counts where recurrent layers store 0.
    fn read_int_array_stats(&mut self) -> std::io::Result<Option<(u64, u64)>> {
        let elem_t = self.read_u32()?;
        let count = self.read_u64()?;
        let Some(w) = scalar_width(elem_t) else {
            return Err(damaged("unexpected array element type under an integer key"));
        };
        if count == 0 || count > 4096 {
            self.skip(w.checked_mul(count).ok_or_else(|| damaged("array size overflows"))?)?;
            return Ok(None);
        }
        let mut sum = 0u64;
        let mut n = 0u64;
        for _ in 0..count {
            match self.read_int_value(elem_t)? {
                Some(v) => {
                    sum += v;
                    n += 1;
                }
                None => {
                    self.skip(w)?;
                }
            }
        }
        Ok(if n > 0 { Some(((sum + n - 1) / n, sum)) } else { None })
    }

    /// Bool array -> (count of true, total). Non-bool arrays are skipped.
    fn read_bool_array_counts(&mut self) -> std::io::Result<Option<(u64, u64)>> {
        let elem_t = self.read_u32()?;
        let count = self.read_u64()?;
        let Some(w) = scalar_width(elem_t) else {
            return Err(damaged("unexpected array element type under a pattern key"));
        };
        if elem_t != 7 || count == 0 || count > 4096 {
            self.skip(w.checked_mul(count).ok_or_else(|| damaged("array size overflows"))?)?;
            return Ok(None);
        }
        let mut on = 0u64;
        for _ in 0..count {
            let mut b = [0u8; 1];
            self.read_exact(&mut b)?;
            if b[0] != 0 {
                on += 1;
            }
        }
        Ok(Some((on, count)))
    }

    /// Read an integer-valued scalar as u64 (for the count keys we care about).
    fn read_int_value(&mut self, t: u32) -> std::io::Result<Option<u64>> {
        Ok(match t {
            0 | 1 => Some(self.read_n::<1>()?),
            2 | 3 => Some(self.read_n::<2>()?),
            4 | 5 => Some(self.read_n::<4>()?),
            10 | 11 => Some(self.read_n::<8>()?),
            _ => None,
        })
    }

    fn read_n<const N: usize>(&mut self) -> std::io::Result<u64> {
        let mut b = [0u8; N];
        self.read_exact(&mut b)?;
        let mut v = 0u64;
        for (i, byte) in b.iter().enumerate() {
            v |= (*byte as u64) << (8 * i);
        }
        Ok(v)
    }

    /// Advance the cursor past a value of type `t` without keeping it.
    fn skip_value(&mut self, t: u32) -> std::io::Result<()> {
        match t {
            8 => self.skip_gstr(),
            9 => {
                // array: elem_type (u32), count (u64), then count elements
                let elem_t = self.read_u32()?;
                let count = self.read_u64()?;
                if elem_t == 8 {
                    // Every string costs at least its 8-byte length prefix.
                    if count > self.remaining() / 8 {
                        return Err(damaged(format!("array count {count} is not plausible")));
                    }
                    for _ in 0..count {
                        self.skip_gstr()?;
                    }
                    Ok(())
                } else if let Some(w) = scalar_width(elem_t) {
                    let bytes = w
                        .checked_mul(count)
                        .ok_or_else(|| damaged("array size overflows"))?;
                    self.skip(bytes)
                } else {
                    // nested array or unknown element type
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "unsupported nested array in gguf metadata",
                    ))
                }
            }
            _ => {
                if let Some(w) = scalar_width(t) {
                    self.skip(w)
                } else {
                    Err(damaged(format!("unknown gguf value type {t}")))
                }
            }
        }
    }
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

/// Read the GGUF metadata of `path`. Errors on a non-GGUF / malformed file.
pub fn read_meta(path: &std::path::Path) -> Result<GgufMeta, String> {
    read_meta_classified(path).map_err(|e| e.to_string())
}

/// Why this file cannot be used as a model, if it cannot: `Some(reason)`
/// for a damaged file (truncated download, corrupted copy, not a GGUF at
/// all). Files this reader merely does not understand are NOT damaged.
pub fn damage(path: &std::path::Path) -> Option<String> {
    match read_meta_classified(path) {
        Err(MetaError::Damaged(reason)) => Some(reason),
        _ => None,
    }
}

/// Read the GGUF metadata with the failure classified (damaged vs
/// unsupported). Successes are cached by (path, mtime, size).
pub fn read_meta_classified(path: &std::path::Path) -> Result<GgufMeta, MetaError> {
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
    let stat = std::fs::metadata(path).map_err(|e| MetaError::Damaged(format!("stat: {e}")))?;
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
    // The parse is bounds-checked end to end; catch_unwind is the belt
    // under those braces so that no arithmetic slip on garbage bytes can
    // ever take the whole app down during a models-directory scan.
    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        read_meta_uncached(path, stat.len())
    }))
    .unwrap_or_else(|_| Err(MetaError::Damaged("header parse panicked".into())))?;
    // A sharded model's answer depends on its sibling shards too (present,
    // sizes, tensor tables), which this key cannot see - and it is cheap to
    // re-read (a few small headers), so it is not cached.
    if parsed.split_count > 1 {
        return Ok(parsed);
    }
    META_CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, parsed.clone());
    Ok(parsed)
}

fn read_meta_uncached(path: &std::path::Path, file_len: u64) -> Result<GgufMeta, MetaError> {
    let f = File::open(path).map_err(|e| MetaError::Damaged(format!("open: {e}")))?;
    let mut r = Bounded { r: BufReader::new(f), pos: 0, len: file_len };
    // Map reader errors: an `Unsupported` kind is a construct we do not
    // parse; everything else (bad lengths, truncation, io) is damage.
    let io = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::Unsupported {
            MetaError::Unsupported(e.to_string())
        } else {
            MetaError::Damaged(e.to_string())
        }
    };

    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)
        .map_err(|e| MetaError::Damaged(format!("read magic: {e}")))?;
    if &magic != b"GGUF" {
        return Err(MetaError::Damaged("not a GGUF file".to_string()));
    }
    let version = r.read_u32().map_err(io)?;
    if !(1..=3).contains(&version) {
        return Err(MetaError::Damaged(format!("gguf version {version} is not plausible")));
    }
    let tensor_count = r.read_u64().map_err(io)?;
    let kv_count = r.read_u64().map_err(io)?;
    if tensor_count > MAX_TENSOR_COUNT || kv_count > MAX_KV_COUNT {
        return Err(MetaError::Damaged(format!(
            "header counts are not plausible (tensors {tensor_count}, kv {kv_count})"
        )));
    }

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
        n_experts: 0,
        n_experts_used: 0,
        ff_expert_len: 0,
        ff_shared_expert_len: 0,
        n_attn_layers: 0,
        kv_heads_sum: 0,
        value_length: 0,
        sliding_window: 0,
        swa_layers: 0,
        swa_pattern_read: false,
        key_length_swa: 0,
        value_length_swa: 0,
        tool_call_marker: None,
        embd_bytes: 0,
        expert_bytes_per_layer: Vec::new(),
        non_expert_bytes: 0,
        split_no: 0,
        split_count: 1,
        total_bytes: file_len,
    };
    let mut alignment: u64 = 32;

    for _ in 0..kv_count {
        let key = r
            .read_gstr()
            .map_err(|e| MetaError::Damaged(format!("read key: {e}")))?;
        let vtype = r.read_u32().map_err(io)?;

        // Match the keys we need by SUFFIX (the arch prefix varies: llama.*,
        // gemma3.*, qwen3.* …). Check head_count_kv before head_count.
        let want_int = key.ends_with(".block_count")
            || key.ends_with(".attention.head_count_kv")
            || key.ends_with(".attention.head_count")
            || key.ends_with(".attention.key_length")
            || key.ends_with(".attention.value_length")
            || key.ends_with(".attention.key_length_swa")
            || key.ends_with(".attention.value_length_swa")
            || key.ends_with(".attention.sliding_window")
            || key.ends_with(".embedding_length")
            || key.ends_with(".context_length")
            || key.ends_with(".expert_count")
            || key.ends_with(".expert_used_count")
            || key.ends_with(".expert_feed_forward_length")
            || key.ends_with(".expert_shared_feed_forward_length")
            || key == "general.file_type"
            || key == "general.alignment"
            || key == "split.no"
            || key == "split.count";

        if key == "tokenizer.chat_template" && vtype == 8 {
            let tpl = r.read_gstr().map_err(io)?;
            m.template_tools = tpl.contains("tools") || tpl.contains("tool_call");
            m.template_strict_alternation =
                tpl.contains("raise_exception") && tpl.contains("alternate");
            // Which channel opener this format emits for tool calls - the
            // known openers, matched against the template itself. A plain
            // chat (no tools) treats that channel as noise, per format,
            // never per model name.
            const TOOL_CALL_OPENERS: [&str; 4] =
                ["<|tool_call_start|>", "<|tool▁calls▁begin|>", "<|tool_call|>", "<tool_call>"];
            m.tool_call_marker = TOOL_CALL_OPENERS.iter().copied().find(|o| tpl.contains(o));
        } else if key == "general.architecture" && vtype == 8 {
            m.architecture = r.read_gstr().map_err(io)?;
        } else if key == "general.size_label" && vtype == 8 {
            m.size_label = r.read_gstr().map_err(io)?;
        } else if want_int && vtype == 9 {
            // Per-layer arrays under a count key (gemma4's head_count_kv):
            // one representative figure for the fit math.
            if let Some((mean, sum)) = r.read_int_array_stats().map_err(io)? {
                if key.ends_with(".attention.head_count_kv") {
                    // Hybrid models store 0 for their non-attention layers:
                    // the SUM is the honest KV figure, the mean stays for
                    // display compatibility.
                    m.n_kv_heads = mean;
                    m.kv_heads_sum = sum;
                } else if key.ends_with(".attention.head_count") {
                    m.n_heads = mean;
                }
            }
        } else if want_int && scalar_width(vtype).is_some() {
            let v = match r.read_int_value(vtype).map_err(io)? {
                Some(v) => v,
                None => {
                    // f32/f64/bool under an integer key: not ours, skip it.
                    r.skip_value(vtype).map_err(io)?;
                    0
                }
            };
            if key.ends_with(".block_count") {
                m.n_layers = v;
            } else if key.ends_with(".attention.head_count_kv") {
                m.n_kv_heads = v;
            } else if key.ends_with(".attention.key_length_swa") {
                m.key_length_swa = v;
            } else if key.ends_with(".attention.value_length_swa") {
                m.value_length_swa = v;
            } else if key.ends_with(".attention.key_length") {
                m.key_length = v;
            } else if key.ends_with(".attention.value_length") {
                m.value_length = v;
            } else if key.ends_with(".attention.sliding_window") {
                m.sliding_window = v;
            } else if key.ends_with(".attention.head_count") {
                m.n_heads = v;
            } else if key.ends_with(".embedding_length") {
                m.embedding_length = v;
            } else if key.ends_with(".context_length") {
                m.context_length = v;
            } else if key.ends_with(".expert_used_count") {
                m.n_experts_used = v;
            } else if key.ends_with(".expert_shared_feed_forward_length") {
                m.ff_shared_expert_len = v;
            } else if key.ends_with(".expert_feed_forward_length") {
                m.ff_expert_len = v;
            } else if key.ends_with(".expert_count") {
                m.n_experts = v;
            } else if key == "general.file_type" {
                m.file_type = v as u32;
            } else if key == "general.alignment" {
                alignment = v.max(1);
            } else if key == "split.no" {
                m.split_no = v as u32;
            } else if key == "split.count" {
                m.split_count = (v as u32).max(1);
            }
        } else if key.ends_with(".attention.sliding_window_pattern") && vtype == 9 {
            // Which layers use the sliding window (true) vs full attention.
            if let Some((swa, total)) = r.read_bool_array_counts().map_err(io)? {
                let _ = total;
                m.swa_layers = swa;
                m.swa_pattern_read = true;
            }
        } else {
            r.skip_value(vtype).map_err(io)?;
        }
    }

    // Some models omit head_count_kv (no GQA) → it equals head_count.
    if m.n_kv_heads == 0 {
        m.n_kv_heads = m.n_heads;
    }

    // Tensor table: name, n_dims, dims, type, offset per tensor. Sizes come
    // from the offset deltas (tensors are laid out back to back in the data
    // section), so no ggml type table is needed; the last tensor's size is
    // the remainder of the file. Read for EVERY file: an offset past the
    // end of the file is a truncated download (a file cut in the tensor
    // data region has a perfectly good header), reported as damage. For a
    // MoE model the expert split is kept; a table that merely does not
    // read as expected leaves it empty - the loader then pins ALL experts
    // rather than none, never the other way round.
    let table = read_tensor_table(&mut r, tensor_count, alignment);
    match table {
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            return Err(MetaError::Damaged(e.to_string()));
        }
        Err(e) => {
            log::warn!("[gguf] tensor table unreadable for {}: {e}", path.display());
        }
        Ok(t) => {
            if m.is_moe() {
                let n_layers = m.n_layers as usize;
                apply_expert_split(&mut m, &t, n_layers);
            }
            m.n_attn_layers = count_attention_layers(&t);
            m.embd_bytes = t.iter().filter(|e| e.is_embd).map(|e| e.bytes).sum();
        }
    }

    // A sharded model: shard 0 holds the metadata but no tensors; the
    // expert split and the total size come from its sibling shards, which
    // sit beside it under the canonical `-0000i-of-0000N` names.
    if m.split_count > 1 && m.split_no == 0 {
        let mut total = file_len;
        let mut per_layer = vec![0u64; m.n_layers as usize];
        let mut non_expert = 0u64;
        let mut complete = true;
        let mut attn_blocks: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for sib in shard_paths(path, m.split_count).into_iter().skip(1) {
            match read_shard_table(&sib) {
                Ok((len, t)) => {
                    total += len;
                    accumulate_expert_split(&t, &mut per_layer, &mut non_expert);
                    attn_blocks.extend(t.iter().filter_map(|e| match e.attn_block {
                        Some((b, BlockMark::K)) => Some(b),
                        Some((b, BlockMark::Qkv)) => Some(b),
                        _ => None,
                    }));
                    // Recurrent blocks never count, even with a fused QKV.
                    for e in &t {
                        if let Some((b, BlockMark::Recurrent)) = e.attn_block {
                            attn_blocks.remove(&b);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[gguf] shard {} of {}: {e}", sib.display(), path.display());
                    complete = false;
                }
            }
        }
        m.total_bytes = total;
        if complete && !attn_blocks.is_empty() {
            m.n_attn_layers = attn_blocks.len() as u64;
        }
        if m.is_moe() {
            if complete && per_layer.iter().any(|&b| b > 0) {
                m.expert_bytes_per_layer = per_layer;
                m.non_expert_bytes = non_expert;
            } else {
                m.expert_bytes_per_layer.clear();
                m.non_expert_bytes = 0;
            }
        }
    }
    Ok(m)
}

/// The canonical shard filenames of a sharded model, given any one shard:
/// `<prefix>-0000i-of-0000N.gguf` for i in 1..=N (llama.cpp's
/// `llama_split_path` format). None if `path` is not named as a shard.
pub fn shard_paths(path: &std::path::Path, count: u32) -> Vec<std::path::PathBuf> {
    let Some((prefix, _, n)) = shard_name_parts(path) else {
        return vec![path.to_path_buf()];
    };
    let n = if n == 0 { count } else { n };
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    (1..=n)
        .map(|i| dir.join(format!("{prefix}-{i:05}-of-{n:05}.gguf")))
        .collect()
}

/// Parse `<prefix>-0000i-of-0000N.gguf` -> (prefix, i, N).
pub fn shard_name_parts(path: &std::path::Path) -> Option<(String, u32, u32)> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".gguf")?;
    // ...-00001-of-00003
    let (head, tail) = stem.rsplit_once("-of-")?;
    let (prefix, idx) = head.rsplit_once('-')?;
    if idx.len() != 5 || tail.len() != 5 {
        return None;
    }
    let i: u32 = idx.parse().ok()?;
    let n: u32 = tail.parse().ok()?;
    (i >= 1 && n >= 1 && i <= n).then(|| (prefix.to_string(), i, n))
}

/// One tensor-table entry we keep: byte size and the expert block it
/// belongs to (None = not an expert tensor).
struct TensorBytes {
    bytes: u64,
    expert_block: Option<usize>,
    /// The block this tensor belongs to when it is an attention K weight
    /// (the layer carries a KV cache); None otherwise.
    attn_block: Option<(usize, BlockMark)>,
    is_embd: bool,
}

/// Attention layers are the ones with a K projection (`blk.N.attn_k.*`,
/// `attn_k_norm` only appears beside one) or a fused `attn_qkv`. Recurrent
/// layers of hybrid models carry `ssm_*`/`shortconv` tensors - and some
/// (Qwen 3.5/3.8) put a fused `attn_qkv` on their LINEAR blocks too, so a
/// fused projection only counts when the block has no recurrent tensors.
fn attn_block_of(name: &str) -> Option<(usize, BlockMark)> {
    let rest = name.strip_prefix("blk.")?;
    let dot = rest.find('.')?;
    let block: usize = rest[..dot].parse().ok()?;
    let tail = &rest[dot..];
    let mark = if tail.starts_with(".attn_k.") || tail.starts_with(".attn_k_") {
        BlockMark::K
    } else if tail.starts_with(".attn_qkv") {
        BlockMark::Qkv
    } else if tail.starts_with(".ssm_") || tail.starts_with(".shortconv") {
        BlockMark::Recurrent
    } else {
        return None;
    };
    Some((block, mark))
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BlockMark {
    K,
    Qkv,
    Recurrent,
}

/// Distinct blocks that truly carry a KV cache: a K projection, or a fused
/// QKV in a block with no recurrent tensors.
fn count_attention_layers(t: &[TensorBytes]) -> u64 {
    use std::collections::HashSet;
    let mut k: HashSet<usize> = HashSet::new();
    let mut qkv: HashSet<usize> = HashSet::new();
    let mut rec: HashSet<usize> = HashSet::new();
    for e in t {
        match e.attn_block {
            Some((b, BlockMark::K)) => { k.insert(b); }
            Some((b, BlockMark::Qkv)) => { qkv.insert(b); }
            Some((b, BlockMark::Recurrent)) => { rec.insert(b); }
            None => {}
        }
    }
    let mut blocks = k;
    for b in qkv {
        if !rec.contains(&b) {
            blocks.insert(b);
        }
    }
    blocks.len() as u64
}

/// Read a shard's tensor table (bounded) and return its file length + table.
fn read_shard_table(path: &std::path::Path) -> std::io::Result<(u64, Vec<TensorBytes>)> {
    let len = std::fs::metadata(path)?.len();
    let f = File::open(path)?;
    let mut r = Bounded { r: BufReader::new(f), pos: 0, len };
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(damaged("shard is not a GGUF file"));
    }
    let _version = r.read_u32()?;
    let tensor_count = r.read_u64()?;
    let kv_count = r.read_u64()?;
    if tensor_count > MAX_TENSOR_COUNT || kv_count > MAX_KV_COUNT {
        return Err(damaged("shard header counts are not plausible"));
    }
    let mut alignment: u64 = 32;
    for _ in 0..kv_count {
        let key = r.read_gstr()?;
        let vtype = r.read_u32()?;
        if key == "general.alignment" && scalar_width(vtype).is_some() {
            if let Some(v) = r.read_int_value(vtype)? {
                alignment = v.max(1);
            }
        } else {
            r.skip_value(vtype)?;
        }
    }
    let t = read_tensor_table(&mut r, tensor_count, alignment)?;
    Ok((len, t))
}

fn accumulate_expert_split(t: &[TensorBytes], per_layer: &mut [u64], non_expert: &mut u64) {
    for e in t {
        match e.expert_block {
            Some(b) if b < per_layer.len() => per_layer[b] += e.bytes,
            _ => *non_expert += e.bytes,
        }
    }
}

fn apply_expert_split(m: &mut GgufMeta, t: &[TensorBytes], n_layers: usize) {
    let mut per_layer = vec![0u64; n_layers];
    let mut non_expert = 0u64;
    accumulate_expert_split(t, &mut per_layer, &mut non_expert);
    if per_layer.iter().any(|&b| b > 0) {
        m.expert_bytes_per_layer = per_layer;
        m.non_expert_bytes = non_expert;
    }
}

/// Expert tensors: the per-block feed-forward expert weights llama.cpp's
/// `--cpu-moe` / `--n-cpu-moe` pin to the CPU (upstream pattern
/// `blk.N.ffn_(up|down|gate|gate_up)_(ch|)exps`). Returns the block index.
fn expert_block_of(name: &str) -> Option<usize> {
    let rest = name.strip_prefix("blk.")?;
    let dot = rest.find('.')?;
    let block: usize = rest[..dot].parse().ok()?;
    let tail = &rest[dot..];
    let is_exps = [".ffn_up_", ".ffn_down_", ".ffn_gate_", ".ffn_gate_up_"]
        .iter()
        .any(|p| {
            tail.strip_prefix(p)
                .map(|t| t.starts_with("exps") || t.starts_with("chexps"))
                .unwrap_or(false)
        });
    is_exps.then_some(block)
}

/// Read the tensor table at the cursor: (name, n_dims, dims, type, offset)
/// per tensor. Sizes are offset deltas; the last tensor takes the rest of
/// the file. An offset past the end of the file is `InvalidData` (a
/// truncated download); anything else that does not read is `Other`.
fn read_tensor_table<R: Read + Seek>(
    r: &mut Bounded<R>,
    tensor_count: u64,
    alignment: u64,
) -> std::io::Result<Vec<TensorBytes>> {
    // (offset, expert block or None, attention block or None)
    let mut tensors: Vec<(u64, Option<usize>, Option<(usize, BlockMark)>, bool)> =
        Vec::with_capacity(tensor_count as usize);
    for _ in 0..tensor_count {
        let name = r.read_gstr()?;
        let n_dims = r.read_u32()?;
        if n_dims > 8 {
            return Err(damaged(format!("tensor with {n_dims} dims is not plausible")));
        }
        for _ in 0..n_dims {
            r.read_u64()?;
        }
        let _ty = r.read_u32()?;
        let offset = r.read_u64()?;
        tensors.push((offset, expert_block_of(&name), attn_block_of(&name), name == "token_embd.weight"));
    }
    // Data section starts at the header end rounded up to the alignment.
    let data_start = r.pos.div_ceil(alignment) * alignment;
    let data_len = r.len.saturating_sub(data_start);
    tensors.sort_by_key(|t| t.0);
    let mut out = Vec::with_capacity(tensors.len());
    for (i, (offset, block, attn, embd)) in tensors.iter().enumerate() {
        let end = tensors.get(i + 1).map(|t| t.0).unwrap_or(data_len);
        if end < *offset || *offset > data_len {
            return Err(damaged("tensor data runs past the end of the file (incomplete download)"));
        }
        out.push(TensorBytes { bytes: end - offset, expert_block: *block, attn_block: *attn, is_embd: *embd });
    }
    Ok(out)
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

    fn tmp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("yoai-gguf-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn gguf_header(tensors: u64, kvs: u64) -> Vec<u8> {
        let mut v = b"GGUF".to_vec();
        v.extend_from_slice(&3u32.to_le_bytes());
        v.extend_from_slice(&tensors.to_le_bytes());
        v.extend_from_slice(&kvs.to_le_bytes());
        v
    }

    fn gstr(s: &str) -> Vec<u8> {
        let mut v = (s.len() as u64).to_le_bytes().to_vec();
        v.extend_from_slice(s.as_bytes());
        v
    }

    /// The field case: a truncated/garbage file must come back as Damaged -
    /// never abort on a giant allocation, never spin on a giant count. Every
    /// variant here finishes instantly.
    #[test]
    fn damaged_files_are_reported_not_fatal() {
        // 1. Not a GGUF at all.
        let p = tmp_file("garbage.gguf", b"this is not a model file at all, just bytes");
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));
        assert!(damage(&p).is_some());

        // 2. Valid header, then a key whose length claims multi-GB (the
        //    0.5.1 crash: read_gstr allocating a garbage length).
        let mut b = gguf_header(100, 5);
        b.extend_from_slice(&(5u64 << 30).to_le_bytes()); // 5 GiB "string"
        b.extend_from_slice(b"junkjunkjunk");
        let p = tmp_file("giant-string.gguf", &b);
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));

        // 3. A string array with an absurd count (would loop forever unbounded).
        let mut b = gguf_header(100, 1);
        b.extend_from_slice(&gstr("tokenizer.ggml.tokens"));
        b.extend_from_slice(&9u32.to_le_bytes()); // array
        b.extend_from_slice(&8u32.to_le_bytes()); // of strings
        b.extend_from_slice(&u64::MAX.to_le_bytes()); // count
        let p = tmp_file("giant-array.gguf", &b);
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));

        // 4. A scalar array whose byte size overflows / runs past EOF.
        let mut b = gguf_header(100, 1);
        b.extend_from_slice(&gstr("tokenizer.ggml.scores"));
        b.extend_from_slice(&9u32.to_le_bytes());
        b.extend_from_slice(&6u32.to_le_bytes()); // f32
        b.extend_from_slice(&(u64::MAX / 2).to_le_bytes());
        let p = tmp_file("overflow-array.gguf", &b);
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));

        // 5. Truncated mid-field: header promises 3 KV pairs, file ends after one.
        let mut b = gguf_header(10, 3);
        b.extend_from_slice(&gstr("general.architecture"));
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&gstr("llama"));
        b.extend_from_slice(&gstr("llama.block_cou")); // cut off
        let p = tmp_file("truncated.gguf", &b);
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));

        // 6. Implausible header counts.
        let p = tmp_file("counts.gguf", &gguf_header(u64::MAX, u64::MAX));
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));

        // 7. Empty file.
        let p = tmp_file("empty.gguf", b"");
        assert!(matches!(read_meta_classified(&p), Err(MetaError::Damaged(_))));
    }

    /// A well-formed synthetic header parses, including the MoE keys, and is
    /// NOT flagged as damaged.
    #[test]
    fn synthetic_moe_header_parses() {
        // Header-only synthetic file: declares no tensors (a declared tensor
        // with no table is a truncated file, and reads as damaged - by design).
        let mut b = gguf_header(0, 6);
        let kv_u32 = |b: &mut Vec<u8>, k: &str, v: u32| {
            b.extend_from_slice(&gstr(k));
            b.extend_from_slice(&4u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        };
        b.extend_from_slice(&gstr("general.architecture"));
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&gstr("qwen3moe"));
        kv_u32(&mut b, "qwen3moe.block_count", 48);
        kv_u32(&mut b, "qwen3moe.attention.head_count", 32);
        kv_u32(&mut b, "qwen3moe.attention.head_count_kv", 4);
        kv_u32(&mut b, "qwen3moe.expert_count", 128);
        kv_u32(&mut b, "qwen3moe.expert_used_count", 8);
        let p = tmp_file("moe.gguf", &b);
        let m = read_meta_classified(&p).expect("parses");
        assert_eq!(m.architecture, "qwen3moe");
        assert_eq!(m.n_layers, 48);
        assert_eq!(m.n_kv_heads, 4);
        assert_eq!(m.n_experts, 128);
        assert_eq!(m.n_experts_used, 8);
        assert!(m.is_moe());
        assert!(damage(&p).is_none());
        // No tensor table -> the expert split is unknown, not fatal.
        assert!(m.expert_bytes_per_layer.is_empty());
    }

    /// A MoE file WITH a tensor table: per-layer expert bytes come from the
    /// offset deltas, everything else is non-expert, the last tensor is the
    /// file remainder. Also: an array-valued head_count_kv (gemma4's shape)
    /// reads as its mean instead of being skipped.
    #[test]
    fn tensor_table_gives_expert_bytes_per_layer() {
        let mut b = gguf_header(5, 5);
        let kv_u32 = |b: &mut Vec<u8>, k: &str, v: u32| {
            b.extend_from_slice(&gstr(k));
            b.extend_from_slice(&4u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        };
        b.extend_from_slice(&gstr("general.architecture"));
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&gstr("gemma4"));
        kv_u32(&mut b, "gemma4.block_count", 2);
        kv_u32(&mut b, "gemma4.attention.head_count", 16);
        // head_count_kv as a per-layer array [2, 8] -> mean 5
        b.extend_from_slice(&gstr("gemma4.attention.head_count_kv"));
        b.extend_from_slice(&9u32.to_le_bytes()); // array
        b.extend_from_slice(&4u32.to_le_bytes()); // of u32
        b.extend_from_slice(&2u64.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&8u32.to_le_bytes());
        kv_u32(&mut b, "gemma4.expert_count", 128);
        // tensor table: name, n_dims, dims, type, offset
        let tensor = |b: &mut Vec<u8>, name: &str, off: u64| {
            b.extend_from_slice(&gstr(name));
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&16u64.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&off.to_le_bytes());
        };
        tensor(&mut b, "token_embd.weight", 0);            // 100 bytes
        tensor(&mut b, "blk.0.ffn_up_exps.weight", 100);   // 300
        tensor(&mut b, "blk.0.attn_q.weight", 400);        // 50
        tensor(&mut b, "blk.1.ffn_gate_exps.weight", 450); // 200
        tensor(&mut b, "blk.1.ffn_down_chexps.weight", 650); // remainder = 150
        // pad header to alignment 32, then 800 bytes of "data"
        while b.len() % 32 != 0 {
            b.push(0);
        }
        b.extend(std::iter::repeat(7u8).take(800));
        let p = tmp_file("moe-tensors.gguf", &b);
        let m = read_meta_classified(&p).expect("parses");
        assert_eq!(m.n_kv_heads, 5, "array-valued head_count_kv reads as its mean");
        assert_eq!(m.expert_bytes_per_layer, vec![300, 350]);
        assert_eq!(m.non_expert_bytes, 150);
        assert_eq!((m.split_no, m.split_count), (0, 1));
        assert_eq!(m.total_bytes, std::fs::metadata(&p).unwrap().len());

        // The same file cut inside the tensor data: header fine, a tensor
        // offset now points past the end -> damaged, not "header OK".
        let cut = &b[..b.len() - 600];
        let p2 = tmp_file("moe-tensors-cut.gguf", cut);
        assert!(matches!(read_meta_classified(&p2), Err(MetaError::Damaged(_))));
        assert!(expert_block_of("blk.12.ffn_gate_up_exps.weight") == Some(12));
        assert!(expert_block_of("blk.3.ffn_up_shexp.weight").is_none(), "shared experts stay on the GPU");
        assert!(expert_block_of("blk.3.attn_k.weight").is_none());
    }

    /// A sharded MoE model the way unsloth ships the big ones: shard 1 =
    /// metadata only (0 tensors, split 0/3), shards 2-3 = tensors with ONLY
    /// the split keys. Shard 1 is the model (sums sizes and expert bytes
    /// across its siblings); shards 2-3 are parts, never models.
    #[test]
    fn sharded_model_reads_as_one_model() {
        let dir = std::env::temp_dir().join("yoai-gguf-tests").join("shards");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let kv_u32 = |b: &mut Vec<u8>, k: &str, v: u32| {
            b.extend_from_slice(&gstr(k));
            b.extend_from_slice(&4u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        };
        let kv_u16 = |b: &mut Vec<u8>, k: &str, v: u16| {
            b.extend_from_slice(&gstr(k));
            b.extend_from_slice(&2u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        };
        let tensor = |b: &mut Vec<u8>, name: &str, off: u64| {
            b.extend_from_slice(&gstr(name));
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&16u64.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&off.to_le_bytes());
        };
        let pad = |b: &mut Vec<u8>| { while b.len() % 32 != 0 { b.push(0); } };
        // shard 1: metadata, no tensors
        let mut s1 = gguf_header(0, 6);
        s1.extend_from_slice(&gstr("general.architecture"));
        s1.extend_from_slice(&8u32.to_le_bytes());
        s1.extend_from_slice(&gstr("deepseek4"));
        kv_u32(&mut s1, "deepseek4.block_count", 2);
        kv_u32(&mut s1, "deepseek4.attention.head_count", 8);
        kv_u32(&mut s1, "deepseek4.expert_count", 256);
        kv_u16(&mut s1, "split.no", 0);
        kv_u16(&mut s1, "split.count", 3);
        pad(&mut s1);
        // shard 2: 2 tensors (expert of block 0 + attention), 400 bytes data
        let mut s2 = gguf_header(2, 3);
        kv_u16(&mut s2, "split.no", 1);
        kv_u16(&mut s2, "split.count", 3);
        kv_u32(&mut s2, "split.tensors.count", 4);
        tensor(&mut s2, "blk.0.ffn_up_exps.weight", 0);   // 300
        tensor(&mut s2, "blk.0.attn_q.weight", 300);      // remainder 100
        pad(&mut s2);
        s2.extend(std::iter::repeat(1u8).take(400));
        // shard 3: expert of block 1 + shared expert, 500 bytes data
        let mut s3 = gguf_header(2, 3);
        kv_u16(&mut s3, "split.no", 2);
        kv_u16(&mut s3, "split.count", 3);
        kv_u32(&mut s3, "split.tensors.count", 4);
        tensor(&mut s3, "blk.1.ffn_down_exps.weight", 0); // 350
        tensor(&mut s3, "blk.1.ffn_up_shexp.weight", 350); // remainder 150
        pad(&mut s3);
        s3.extend(std::iter::repeat(2u8).take(500));
        let p1 = dir.join("big-UD-Q2_K_XL-00001-of-00003.gguf");
        let p2 = dir.join("big-UD-Q2_K_XL-00002-of-00003.gguf");
        let p3 = dir.join("big-UD-Q2_K_XL-00003-of-00003.gguf");
        std::fs::write(&p1, &s1).unwrap();
        std::fs::write(&p2, &s2).unwrap();
        std::fs::write(&p3, &s3).unwrap();

        assert_eq!(shard_name_parts(&p2), Some(("big-UD-Q2_K_XL".to_string(), 2, 3)));
        assert_eq!(shard_paths(&p1, 3), vec![p1.clone(), p2.clone(), p3.clone()]);

        let m = read_meta_classified(&p1).expect("shard 1 parses");
        assert_eq!((m.split_no, m.split_count), (0, 3));
        assert!(!m.is_secondary_shard());
        assert_eq!(m.total_bytes, (s1.len() + s2.len() + s3.len()) as u64);
        assert!(m.is_moe());
        assert_eq!(m.expert_bytes_per_layer, vec![300, 350], "experts summed across data shards");
        assert_eq!(m.non_expert_bytes, 250, "attention + shared expert");

        let m2 = read_meta_classified(&p2).expect("a data shard still parses");
        assert!(m2.is_secondary_shard());
        assert!(damage(&p2).is_none(), "a data shard is a part, not a damaged model");

        // A missing sibling: shard 1 still reads, but the split is unknown
        // (the loader pins all experts) and the set is incomplete.
        std::fs::remove_file(&p3).unwrap();
        let m = read_meta_classified(&p1).expect("shard 1 still parses");
        assert!(m.expert_bytes_per_layer.is_empty());
        assert!(shard_paths(&p1, 3).iter().any(|p| !p.exists()));
    }

    /// Muse Glimmer's REAL chat template (extracted from the Unsloth GGUF
    /// header, 2026-08-11) must pass the agent blessing: it has full tool
    /// support AND a raise_exception that is NOT an alternation guard (it
    /// fires on string-typed tool arguments). The phrases below are the
    /// load-bearing excerpts - if the blessing heuristic ever changes such
    /// that this unblesses, this test says so before a release does.
    #[test]
    fn muse_glimmer_template_is_blessed() {
        let excerpt = "{%- if args is not mapping -%}{{- raise_exception('Onyx ATEM chat template requires tool_call.function.arguments to be a dict (mapping); a JSON string cannot be parsed in the HF jinja sandbox.') -}}{%- endif -%} {%- if message.get('tool_calls') -%} render_tool_defs(tools)";
        let tools = excerpt.contains("tools") || excerpt.contains("tool_call");
        let strict = excerpt.contains("raise_exception") && excerpt.contains("alternate");
        assert!(tools, "Muse template must register as tool-capable");
        assert!(!strict, "Muse's argument-type raise_exception must not read as strict alternation");
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
