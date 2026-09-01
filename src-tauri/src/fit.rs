//! VRAM/RAM fit for downloaded models - does a model fit, and how well?
//!
//! Uses the GGUF header (exact layers / GQA kv-heads / head_dim / quant) for the
//! weights + KV-cache size, and the **free** VRAM (after the webview's usage)
//! vs total. Grades each downloaded model GREEN (fits fully on GPU → fast),
//! YELLOW (needs CPU offload → slow), RED (won't run). The router prefers GREEN;
//! the UI can badge each model.

use serde::Serialize;
use tauri::AppHandle;

use crate::gguf::GgufMeta;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Fits fully on the graphics card: full speed.
    Green,
    /// A mixture-of-experts model bigger than the card, running with its
    /// experts in main memory ("GPU + RAM"): fast for its size - a first-class
    /// grade, not a slower one. Routing treats it like green.
    Split,
    /// Runs, slower: on a CPU-only machine a model that is tight in RAM. (On a
    /// GPU machine a dense model that does not fit the card is NOT yellow - it
    /// loads fully on the card or not at all - it is red.)
    Yellow,
    /// Will not run here.
    Red,
}

impl Fit {
    /// Routing tier: higher runs better. Green and Split share the top - both
    /// answer at full usable speed on this machine.
    pub fn tier(self) -> u8 {
        match self {
            Fit::Green | Fit::Split => 2,
            Fit::Yellow => 1,
            Fit::Red => 0,
        }
    }
    /// "Runs comfortably" - what agent work and the vision pairing ask for.
    pub fn is_fast(self) -> bool {
        matches!(self, Fit::Green | Fit::Split)
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelFit {
    pub name: String,
    /// The file's chat template can host agent conversations.
    pub agent_template_ok: bool, // GGUF filename
    pub fit: Fit,
    pub need_gb: f64,
    pub weights_gb: f64,
    pub kv_gb: f64,
    /// Header-derived (exact) - replaces the often-"Unknown" filename guesses.
    pub quant: String,
    pub params_b: f64, // approx, from size_bytes / effective-bpw
    pub n_layers: u64,
    pub context_max: u64,
    /// The context the server would actually start this model with on this
    /// machine right now (choose_ctx) - what "runs at" means in the UI.
    pub context_runtime: u64,
    /// Mixture-of-experts model (the Fine-tune dialog shows the split
    /// slider from this fact, never from the current offload decision).
    pub is_moe: bool,
    /// What the automatics would pick RIGHT NOW for the expert split
    /// (0 = everything fits on the card) - always computed for a MoE with
    /// a VRAM figure, independent of any tuned decision. The dialog's
    /// "Auto (n)" label reads this, never the decision field.
    pub moe_auto_pick: Option<u32>,
    /// This model carries fine-tune overrides on this machine.
    pub tuned: bool,
    /// A mixture-of-experts model bigger than the graphics card that runs
    /// with its experts in main memory (`--cpu-moe`): fits, and fast for its
    /// size - graded yellow with this flag so the UI can say why.
    pub moe_offload: bool,
    /// Under offload: how many layers' experts the loader would pin to the
    /// CPU (`--n-cpu-moe N`); None = all of them (`--cpu-moe`), which is
    /// also the floor when the file's tensor table could not be read.
    pub moe_cpu_layers: Option<u32>,
    /// This machine's measured generation speed for the model (engine
    /// timings, moving average), when it has been used here.
    pub measured_tps: Option<f64>,
    /// Measured on the MLX engine (Apple Silicon), when it has served this
    /// model here - kept apart so the row can show the engine in use.
    pub measured_tps_mlx: Option<f64>,
    /// How long the last load took here (seconds), when known.
    pub load_secs: Option<f64>,
}

/// The context sizes the server can start at (largest first). The top
/// rungs are only ever reached VRAM-gated and clamped to the model's
/// trained context - a 1M-context model on a card that can hold 64K of
/// KV runs at 64K, and a dense model whose KV would not fit stays lower.
const CTX_LADDER: [u64; 6] = [131072, 65536, 32768, 16384, 8192, 4096];

/// The smallest ladder rung that holds `need_tokens` and that this
/// machine can afford for this model - fully on the card (the green
/// bound) or within the card plus a bounded slice of main memory (the
/// same budget the agent floor uses). CPU-only machines size against
/// main memory alone. None = nothing affordable holds it.
pub fn ctx_for_need(
    meta: &GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
    need_tokens: u64,
) -> Option<u64> {
    let cap = if meta.context_length > 0 { meta.context_length.max(4096) } else { u64::MAX };
    for &c in CTX_LADDER.iter().rev() {
        if c < need_tokens || c > cap {
            continue;
        }
        if rung_affordable(meta, size_bytes, total_ram_gb, free_vram_gb, c) {
            return Some(c);
        }
    }
    None
}

/// Whether this machine affords the model at context `ctx`: on a card, the
/// need fits 90% of free VRAM or spills into main memory; without one, 75%
/// of RAM.
fn rung_affordable(
    meta: &GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
    ctx: u64,
) -> bool {
    let need = model_need(meta, size_bytes, ctx).2;
    match free_vram_gb {
        Some(vram) => need <= 0.9 * vram || need <= vram + 0.6 * total_ram_gb,
        None => need <= 0.75 * total_ram_gb,
    }
}

/// The largest ladder rung this machine affords for the model - the reading
/// room's ceiling, so a meter can say what the app CAN make room for rather
/// than what happens to be loaded.
pub fn max_ctx(
    meta: &GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
) -> Option<u64> {
    let cap = if meta.context_length > 0 { meta.context_length.max(4096) } else { u64::MAX };
    CTX_LADDER
        .iter()
        .copied()
        .filter(|&c| c <= cap)
        .find(|&c| rung_affordable(meta, size_bytes, total_ram_gb, free_vram_gb, c))
}

/// The RAM-tier baseline (the sizes that have shipped for months). The
/// thresholds sit BELOW the installed sizes they stand for: the OS reports
/// USABLE memory, so a "32GB" machine reads ~31.8GB and a "12GB" machine
/// ~11.7GB - a >= 32.0 check can never match the hardware it targets.
pub fn ram_tier_ctx(total_ram_gb: f64, small_model: bool) -> u64 {
    if small_model {
        if total_ram_gb >= 11.0 { 32768 } else { 8192 }
    } else if total_ram_gb >= 30.0 {
        16384
    } else if total_ram_gb >= 11.0 {
        8192
    } else {
        4096
    }
}

/// The context the server should run this model at, VRAM-aware.
///
/// The RAM tier is the FLOOR - this function only ever raises it, so no
/// machine regresses below the shipped behavior. On top of that:
/// 1. The largest rung whose weights + KV + overhead fit fully in (90% of)
///    free VRAM - fully-on-GPU stays fast, so take every token it carries.
/// 2. A 16384 agent floor for partial-offload models when free VRAM plus a
///    bounded RAM slice carry it: an agent session needs >8k (its system
///    prompt alone is ~9k), and every KV byte displaces weights from VRAM,
///    so a bigger rung than 16k would slow an already-offloaded model for
///    no agent benefit.
/// Clamped to the model's trained context. CPU-only machines (no usable
/// discrete GPU) keep the RAM tier - there is no second pool to reason about.
pub fn choose_ctx(
    meta: &GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
) -> u64 {
    let small_model = (size_bytes as f64 / GIB) < 6.0;
    let mut pick = ram_tier_ctx(total_ram_gb, small_model);
    if let Some(vram) = free_vram_gb {
        let need = |ctx: u64| model_need(meta, size_bytes, ctx).2;
        // 1. Largest fully-on-GPU rung (same green bound the fit badge uses).
        for &c in &CTX_LADDER {
            if c > pick && need(c) <= 0.9 * vram {
                pick = c;
                break;
            }
        }
        // 2. Agent floor under partial offload. The RAM slice is bounded:
        //    the OS, the webview, and the conductor need the rest.
        let ram_budget = 0.6 * total_ram_gb;
        if pick < 16384 && need(16384) <= vram + ram_budget {
            pick = 16384;
        }
    }
    // Never start past the model's trained context (floor 4096 regardless -
    // below that nothing useful runs).
    if meta.context_length > 0 {
        pick = pick.min(meta.context_length.max(4096));
    }
    // 3. The card has the last word for DENSE models (fit-truth 09-01: on
    //    a 4 GB card the 32k RAM-tier floor made 2-3 GB models fail to
    //    load - the engine puts the whole cache in graphics memory). Step
    //    down the ladder until the need fits; never below 4096. MoE models
    //    keep their rung - the expert split is their pressure valve.
    if let Some(vram) = free_vram_gb {
        if !meta.is_moe() {
            while pick > 4096 && model_need(meta, size_bytes, pick).2 > 0.9 * vram {
                let lower = CTX_LADDER
                    .iter()
                    .copied()
                    .filter(|&c| c < pick)
                    .max()
                    .unwrap_or(4096);
                pick = lower;
            }
        }
    }
    pick
}

/// Memory a model needs: quantized weights (the GGUF file size) + KV-cache at
/// `ctx` (f16) + a fixed compute-buffer overhead.
pub fn model_need(meta: &GgufMeta, size_bytes: u64, ctx: u64) -> (f64, f64, f64) {
    let weights_gb = size_bytes as f64 / GIB;
    let eff_ctx = if meta.context_length > 0 {
        ctx.min(meta.context_length)
    } else {
        ctx
    } as f64;
    // KV = 2 (K and V) × layers × kv_heads × head_dim × ctx × 2 bytes (f16).
    // Only layers that carry attention hold a KV cache: hybrid models keep
    // attention on a fraction of their layers, and charging all of them
    // over-counted their context cost several times over (a 1M-context 9B
    // read as unable to afford 64K on an 8 GB card that runs it fine).
    let kv_layers = if meta.n_attn_layers > 0 && meta.n_attn_layers <= meta.n_layers {
        meta.n_attn_layers
    } else {
        meta.n_layers
    };
    // KV cache, per what the header really says (fit-truth audit 09-01):
    // - per-layer head arrays: the SUM over attention layers, not a mean
    //   diluted by recurrent layers' zeros (LFM2 was 4x under);
    // - K and V dims separately when declared (gemma4's V != K);
    // - sliding-window layers pay min(ctx, window), not the full context
    //   (gemma3/4 were ~5x over at 32k); without a pattern array a
    //   declared window is assumed on every attention layer;
    // - hybrid blocks were already excluded from kv_layers by the tensor
    //   walk (fused QKV beside recurrent tensors no longer counts).
    let heads_avg = if meta.kv_heads_sum > 0 && kv_layers > 0 {
        (meta.kv_heads_sum as f64 / kv_layers as f64).max(1.0)
    } else {
        meta.n_kv_heads as f64
    };
    let kd = meta.head_dim() as f64;
    let vd = if meta.value_length > 0 { meta.value_length as f64 } else { kd };
    let kd_swa = if meta.key_length_swa > 0 { meta.key_length_swa as f64 } else { kd };
    let vd_swa = if meta.value_length_swa > 0 { meta.value_length_swa as f64 } else { vd };
    let windowed = meta.sliding_window > 0 && (meta.sliding_window as f64) < eff_ctx;
    let swa_layers = if !windowed {
        0.0
    } else if meta.swa_pattern_read {
        (meta.swa_layers as f64).min(kv_layers as f64)
    } else {
        // A declared window without a pattern array: assume HALF the
        // layers - optimism here made medgemma claim 131k and fail to
        // load (fit-truth 09-01); pessimism only costs context.
        (kv_layers as f64 / 2.0).floor()
    };
    let full_layers = kv_layers as f64 - swa_layers;
    let swa_ctx = (meta.sliding_window as f64).min(eff_ctx);
    let kv_gb = heads_avg
        * (full_layers * (kd + vd) * eff_ctx + swa_layers * (kd_swa + vd_swa) * swa_ctx)
        * 2.0
        / GIB;
    let overhead_gb = 0.8;
    // The token-embedding table stays in system memory even under full
    // offload - the card never pays for it (fit-truth 09-01: gemma E2B
    // served from 1.94 GB while its file is 2.89 GB).
    let on_card_weights_gb = (size_bytes.saturating_sub(meta.embd_bytes)) as f64 / GIB;
    (weights_gb, kv_gb, on_card_weights_gb + kv_gb + overhead_gb)
}

/// Does this MoE model call for expert offload? Whenever its full footprint
/// does not fit (90% of) free VRAM - the same bound that makes a model
/// green. Above it, the only alternative is the driver paging VRAM over
/// PCIe (slower than the CPU alone, measured) or an outright OOM.
/// How much of the feed-forward runs on EVERY token. Catalog MoEs sit in
/// the 5-13% range - their experts are cold mass, and parking them in main
/// memory is nearly free. A post-hoc "surgery" MoE can activate most of its
/// width (Whittle-27B: 16-of-64 slivers + a 5120-wide always-on shared
/// expert = 47% of the FFN every token) - then the split ships the bulk of
/// the compute across the bus each step, and the honest grade is "runs
/// slower", not "runs well". None when the header lacks the counts.
pub fn moe_active_fraction(meta: &crate::gguf::GgufMeta) -> Option<f64> {
    if meta.n_experts == 0 || meta.n_experts_used == 0 {
        return None;
    }
    let ff_e = meta.ff_expert_len as f64;
    let ff_s = meta.ff_shared_expert_len as f64;
    if ff_e <= 0.0 {
        return Some(meta.n_experts_used as f64 / meta.n_experts as f64);
    }
    let active = meta.n_experts_used as f64 * ff_e + ff_s;
    let total = meta.n_experts as f64 * ff_e + ff_s;
    Some(active / total)
}

/// Above this active fraction a split stops counting as "runs well".
pub const MOE_HIGH_ACTIVE_FRACTION: f64 = 0.30;

pub fn moe_offload_wanted(need_gb: f64, free_vram_gb: f64) -> bool {
    need_gb > 0.9 * free_vram_gb
}

/// Can this MoE model run with its experts in main memory? The experts are
/// (nearly) the whole file, so the file must fit the RAM budget; attention +
/// KV ride on the card. A 32 GB box carries a 21 GB 35B-A3B; a 16 GB box
/// does not, and the grade says so instead of promising a crawl.
pub fn moe_offload_fits(weights_gb: f64, need_gb: f64, free_vram_gb: f64, ram_budget_gb: f64) -> bool {
    weights_gb <= ram_budget_gb && need_gb <= ram_budget_gb + 0.9 * free_vram_gb
}

/// The RAM an offloaded MoE model can count on: what is free now, or total
/// minus the OS reserve, whichever is larger. The file is memory-mapped and
/// the OS makes room for it on load, so a machine that happens to have
/// 18 GB in use this minute is still a 32 GB machine for this purpose
/// (seen live: the 4060 Ti box graded the 35B red while running it at 32
/// tok/s). The reserve mirrors the catalog's: 40% of RAM, 3..7 GB.
pub fn moe_ram_budget_gb(free_ram_gb: f64, total_ram_gb: f64) -> f64 {
    let reserve = (total_ram_gb * 0.4).clamp(3.0, 7.0);
    free_ram_gb.max(total_ram_gb - reserve)
}

/// The VRAM (GiB) an MoE model needs with the experts of its first `n`
/// layers on the CPU: everything that is not an expert tensor, plus the
/// experts of the remaining layers, plus the KV cache and the compute
/// overhead. With n = n_layers only attention/embeddings/KV remain on the
/// card - the `--cpu-moe` footprint.
pub fn moe_need_gb(meta: &GgufMeta, n_cpu_layers: usize, kv_gb: f64) -> f64 {
    let gpu_experts: u64 = meta
        .expert_bytes_per_layer
        .iter()
        .skip(n_cpu_layers)
        .sum();
    (meta.non_expert_bytes + gpu_experts) as f64 / GIB + kv_gb + 0.8
}

/// Headroom left on the card under expert offload. The bench rule: the
/// best N sat about 1 GiB short of full; every arm that filled the card
/// collapsed below the all-experts-on-CPU floor (driver paging).
pub const MOE_VRAM_HEADROOM_GB: f64 = 1.0;

/// How many layers' experts to pin to the CPU: the SMALLEST n whose
/// footprint leaves the headroom on (pooled) free VRAM. Two answers mean
/// "pin every expert" (`--cpu-moe`) and callers treat them alike on
/// purpose: `None` = the file's expert table could not be read (nothing
/// to model, the floor is the only safe move); `Some(n_layers)` = the
/// table was read and even the all-on-CPU footprint does not fit (the
/// floor is still the least bad move, and the grade says so).
pub fn moe_cpu_layers(meta: &GgufMeta, kv_gb: f64, free_vram_gb: f64) -> Option<usize> {
    moe_cpu_layers_with(meta, kv_gb, free_vram_gb, 0.0)
}

/// Same, with a per-machine correction (GB) learned from real loads: what
/// the estimate over-counted last time is handed back to the budget
/// (positive = put more on the card), what it under-counted is taken off.
pub fn moe_cpu_layers_with(meta: &GgufMeta, kv_gb: f64, free_vram_gb: f64, correction_gb: f64) -> Option<usize> {
    let n_layers = meta.expert_bytes_per_layer.len();
    if n_layers == 0 {
        return None;
    }
    let budget = free_vram_gb - MOE_VRAM_HEADROOM_GB + correction_gb;
    (0..=n_layers).find(|&n| moe_need_gb(meta, n, kv_gb) <= budget).or(Some(n_layers))
}

/// The budget correction from one measured load: predicted minus actual
/// footprint on the card, bounded so a single odd reading cannot swing the
/// split wildly (-1.0 .. +1.5 GB), and never trusted past half the headroom
/// in the risky direction.
pub fn moe_budget_correction_gb(predicted_gb: f64, actual_gb: f64) -> f64 {
    if !(predicted_gb.is_finite() && actual_gb.is_finite()) || actual_gb <= 0.0 {
        return 0.0;
    }
    (predicted_gb - actual_gb).clamp(-1.0, 1.5)
}

/// Grade fit. GPU: GREEN fits in (90% of) free VRAM, else RED - a dense
/// model loads fully on the card or not at all (the app does not partially
/// offload dense models: the field outcome is "too large", or on Windows a
/// driver-paged crawl slower than the CPU). The GPU + RAM split for MoE
/// models is graded separately by `assess` (Fit::Split). CPU-only
/// (`free_vram_gb` = None): GREEN if it comfortably fits RAM, YELLOW if
/// tight, else RED.
pub fn grade(need_gb: f64, free_vram_gb: Option<f64>, free_ram_gb: f64) -> Fit {
    match free_vram_gb {
        Some(vram) => {
            if need_gb <= 0.9 * vram {
                Fit::Green
            } else {
                Fit::Red
            }
        }
        None => {
            if need_gb <= 0.7 * free_ram_gb {
                Fit::Green
            } else if need_gb <= free_ram_gb {
                Fit::Yellow
            } else {
                Fit::Red
            }
        }
    }
}

/// What a model occupies on disk: every shard of a sharded file, not just
/// the first (the engine loads the set; the first part alone under-counts
/// a 35B by two thirds). Missing shards count zero - the inventory marks
/// such a set unusable anyway.
pub fn model_bytes_on_disk(path: &std::path::Path, meta: &GgufMeta) -> u64 {
    if meta.split_count > 1 {
        crate::gguf::shard_paths(path, meta.split_count)
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    } else {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

/// The machine as every grader must see it - ONE source, so the fit
/// badges, the router, the catalog cards and the truth matrix cannot
/// disagree about the hardware. `avail_ram_gb` is what the OS would hand
/// a new allocation NOW (vm_stat on macOS - strict free pages sit near
/// zero on any settled Mac and graded an 8 GB Air's 2.3 GB models "Too
/// large" while they loaded and ran at 29 tok/s); `free_vram_gb` is the
/// discrete cards' pooled free memory, None on CPU-only, integrated and
/// Apple unified memory (those grade against RAM).
#[derive(Clone, Copy, Debug)]
pub struct MachineFigures {
    pub total_ram_gb: f64,
    pub avail_ram_gb: f64,
    pub free_vram_gb: Option<f64>,
}

impl MachineFigures {
    /// From a free-VRAM reading the caller already holds (the matrix
    /// probes the engine binary directly; tests pass what they measured).
    pub fn with_vram(free_vram_gb: Option<f64>) -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        Self {
            total_ram_gb: sys.total_memory() as f64 / GIB,
            avail_ram_gb: crate::llm::available_memory_bytes() as f64 / GIB,
            free_vram_gb,
        }
    }
}

/// The app's own reading: cached free VRAM through the engine the app
/// would chat with, honoring GPU safe mode and the device verdict.
pub async fn machine_figures(app: &AppHandle) -> MachineFigures {
    let free_vram_gb = crate::llm::available_vram_mib(app)
        .await
        .map(|mib| mib as f64 / 1024.0);
    MachineFigures::with_vram(free_vram_gb)
}

/// Hand the incumbent's estimated footprint back to the free figures
/// candidates are graded against ("as if the slot were free"). The
/// incumbent lives in VRAM when a GPU budget exists, in RAM otherwise -
/// a CPU-loaded incumbent distorts free RAM the same way.
pub(crate) fn reclaim_adjust(
    free_vram_gb: Option<f64>,
    free_ram_gb: f64,
    incumbent_need_gb: f64,
) -> (Option<f64>, f64) {
    if incumbent_need_gb <= 0.0 {
        return (free_vram_gb, free_ram_gb);
    }
    match free_vram_gb {
        Some(free) => (Some(free + incumbent_need_gb), free_ram_gb),
        None => (None, free_ram_gb + incumbent_need_gb),
    }
}

fn models_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    // The single chokepoint - honors the user's chosen storage location.
    crate::llm::get_models_dir(app).ok()
}

/// The machine figures every grader reads, with the loaded model's
/// footprint handed back ("as if the chat slot were free") - the same
/// numbers for the downloaded rows, the catalog cards and the router.
pub async fn figures_slot_free(app: &AppHandle, dir: &std::path::Path) -> MachineFigures {
    let figures = machine_figures(app).await;
    let total_ram_gb = figures.total_ram_gb;
    let free_ram_gb = figures.avail_ram_gb;
    let free_vram_gb = figures.free_vram_gb;

    // Grade every candidate AS IF the chat slot were free. Loading any
    // candidate evicts the incumbent (loaded or mid-load), yet the free
    // figures above still include the incumbent's own footprint - so a
    // green model demotes ITSELF once loaded (its usage ate the free VRAM
    // it is graded against), every rival grades a tier too low, and the
    // tier-first balanced ordering stops discriminating (then raw
    // capability decides, which the biggest model always wins). Hand the
    // incumbent's estimated footprint back before grading. The estimate
    // can exceed the incumbent's true VRAM share when it is partially
    // offloaded - erring green is safe (the loader and the session
    // too-big memo still guard reality); erring yellow is this bug.
    let incumbent = {
        let st = tauri::Manager::state::<crate::llm::LLMState>(app);
        let loading = st.loading_model.lock().await.clone();
        let current = st.current_model.lock().await.clone();
        loading.or(current).filter(|c| !c.starts_with("online:"))
    };
    // The measured truth beats the estimate here too: a split-loaded MoE
    // occupies far less of the card than its full need (LFM: est ~5.7,
    // measured ~3.1) - crediting the estimate back inflated every grade,
    // every "runs at", and the Auto split label (fit-truth 09-01, round
    // 4). The calibration file holds what the last healthy load actually
    // took; use it when it exists.
    let reclaim_gb = incumbent
        .and_then(|name| {
            if let Some(measured) = crate::llm::moe_calibration_read(app, &name)
                .map(|c| c.actual_gb)
                .filter(|&a| a > 0.1)
            {
                return Some(measured);
            }
            let path = dir.join(&name);
            let meta = crate::gguf::read_meta(&path).ok()?;
            let size = model_bytes_on_disk(&path, &meta);
            // The incumbent runs at ITS context - a fine-tune pin included -
            // so the footprint handed back is the one it actually holds.
            let ctx = pinned_or_chosen_ctx(app, &name, &meta, size, total_ram_gb, free_vram_gb);
            let (_, _, mut need) = model_need(&meta, size, ctx);
            if let Some(proj) = crate::llm::find_projector_for(&dir, &name) {
                if let Ok(pm) = std::fs::metadata(&proj) {
                    need += pm.len() as f64 / GIB;
                }
            }
            Some(need)
        })
        .unwrap_or(0.0);
    let (free_vram_gb, free_ram_gb) = reclaim_adjust(free_vram_gb, free_ram_gb, reclaim_gb);
    MachineFigures { total_ram_gb, avail_ram_gb: free_ram_gb, free_vram_gb }
}

/// A catalog entry before download: no header to read, so the footprint
/// is a stand-in - the file plus the KV cache and compute overhead at
/// everyday context. The exact header estimate takes over once the file
/// is on disk (assess); the grade thresholds are the same either way.
pub fn estimate_need_gb(size_gb: f64) -> f64 {
    size_gb * 1.05 + 0.85
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct CatalogVariantIn {
    /// The variant's filename - the key the UI maps the answer back by.
    pub key: String,
    pub size_gb: f64,
    pub min_ram_gb: f64,
    pub is_moe: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct CatalogGrade {
    pub key: String,
    /// Where it would run: "gpu" | "moe-split" | "cpu" | "too-big".
    pub mode: String,
    pub fit: Fit,
}

/// One rule for a not-yet-downloaded model, from the same figures and
/// thresholds `grade` applies to downloaded ones. With a discrete card a
/// dense model runs on it or not at all; a mixture-of-experts file too big
/// for the card runs with its experts in main memory when RAM carries it.
/// Without a card (CPU, integrated, Apple unified memory) RAM decides:
/// "gpu" on Apple Silicon means Metal in shared memory - fast - and
/// `unified` names that case so the copy can too.
pub fn grade_catalog_variant(v: &CatalogVariantIn, f: &MachineFigures, unified: bool) -> CatalogGrade {
    let need = estimate_need_gb(v.size_gb);
    let (mode, fit) = match f.free_vram_gb {
        Some(vram) => {
            let g = grade(need, Some(vram), f.avail_ram_gb);
            if g == Fit::Green && v.min_ram_gb <= f.total_ram_gb {
                ("gpu", Fit::Green)
            } else if v.is_moe
                && moe_offload_fits(v.size_gb, need, vram, moe_ram_budget_gb(f.avail_ram_gb, f.total_ram_gb))
            {
                ("moe-split", Fit::Split)
            } else {
                ("too-big", Fit::Red)
            }
        }
        None => {
            if v.min_ram_gb > f.total_ram_gb {
                ("too-big", Fit::Red)
            } else {
                match grade(need, None, f.avail_ram_gb) {
                    Fit::Green => (if unified { "gpu" } else { "cpu" }, Fit::Green),
                    Fit::Yellow => ("cpu", Fit::Yellow),
                    _ => ("too-big", Fit::Red),
                }
            }
        }
    };
    CatalogGrade { key: v.key.clone(), mode: mode.to_string(), fit }
}

/// Apple Silicon runs models in unified memory through Metal: no discrete
/// card, but not a processor-only machine either.
pub fn unified_memory_gpu() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

/// Grade catalog entries for the UI - the cards, the pickers and the
/// welcome recommendation read this, never their own arithmetic.
#[tauri::command]
pub async fn grade_catalog(app: AppHandle, variants: Vec<CatalogVariantIn>) -> Result<Vec<CatalogGrade>, String> {
    let dir = models_dir(&app).unwrap_or_else(std::env::temp_dir);
    let figures = figures_slot_free(&app, &dir).await;
    let unified = unified_memory_gpu() && figures.free_vram_gb.is_none();
    Ok(variants.iter().map(|v| grade_catalog_variant(v, &figures, unified)).collect())
}

/// The context a model runs at here: its fine-tune pin (clamped to the
/// trained window) or the automatic sizing - one rule for the grade, the
/// reclaim and the "runs at" line.
pub(crate) fn pinned_or_chosen_ctx(
    app: &AppHandle,
    name: &str,
    meta: &GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
) -> u64 {
    crate::tuning::get(app, name)
        .context
        .map(|c| if meta.context_length > 0 { c.clamp(4096, meta.context_length.max(4096)) } else { c.max(4096) })
        .unwrap_or_else(|| choose_ctx(meta, size_bytes, total_ram_gb, free_vram_gb))
}

/// The grade for a user-pinned expert split of `tuned_n` layers on the
/// CPU, under the same headroom (and learned correction) the loader's
/// picker uses. An unreadable tensor table cannot model a split at all:
/// the loader pins every expert (`--cpu-moe`) and the honest grade is
/// "runs, slower" with the pick unknown - never a phantom 1-layer split.
pub fn tuned_split_fit(
    meta: &GgufMeta,
    tuned_n: u32,
    kv_gb: f64,
    free_vram_gb: f64,
    correction_gb: f64,
) -> (Fit, Option<u32>) {
    let n_layers = meta.expert_bytes_per_layer.len();
    if n_layers == 0 {
        return (Fit::Yellow, None);
    }
    let n = (tuned_n as usize).min(n_layers);
    let on_card = moe_need_gb(meta, n, kv_gb);
    let fit = if on_card <= free_vram_gb - MOE_VRAM_HEADROOM_GB + correction_gb { Fit::Split } else { Fit::Yellow };
    (fit, Some(n as u32))
}

/// Assess every downloaded model. Reuses free-VRAM (cached) + system RAM.
pub async fn assess(app: &AppHandle) -> Vec<ModelFit> {
    let models = crate::llm::list_local_models(app.clone()).await.unwrap_or_default();
    let dir = match models_dir(app) {
        Some(d) => d,
        None => return vec![],
    };

    let figures = figures_slot_free(app, &dir).await;
    let total_ram_gb = figures.total_ram_gb;
    let free_ram_gb = figures.avail_ram_gb;
    let free_vram_gb = figures.free_vram_gb;

    let stats = crate::model_stats::read_all(app);
    let mut out = Vec::new();
    for m in models {
        let path = dir.join(&m.name);
        let Ok(meta) = crate::gguf::read_meta(&path) else {
            continue; // skip unreadable files
        };
        if meta.is_embedding() {
            continue; // bge etc. - not a chat model; never a routing candidate
        }
        // Grade at the context the server would actually start this model
        // with - per model, since choose_ctx is VRAM- and size-aware.
        // A fine-tune pin replaces the sizing here too, so the grade, the
        // router and the "runs at" line all tell the same story.
        let ctx = pinned_or_chosen_ctx(app, &m.name, &meta, m.size_bytes, total_ram_gb, free_vram_gb);
        let (weights_gb, kv_gb, mut need_gb) = model_need(&meta, m.size_bytes, ctx);
        // A model with a downloaded projector (mmproj) auto-loads it for vision, so
        // its ~1 GB lives in VRAM whenever this model runs - count it toward fit.
        if let Some(proj) = crate::llm::find_projector_for(&dir, &m.name) {
            if let Ok(md) = std::fs::metadata(&proj) {
                need_gb += md.len() as f64 / GIB;
            }
        }
        let params_b = if meta.effective_bpw() > 0.0 {
            (m.size_bytes as f64 * 8.0 / meta.effective_bpw()) / 1e9
        } else {
            0.0
        };
        let mut fit = grade(need_gb, free_vram_gb, free_ram_gb);
        let mut moe_offload = false;
        let mut moe_cpu_layers_pick: Option<u32> = None;
        // The tuned expert split replaces the automatics here too - the
        // badge, the router and the loader must tell one story. 0 =
        // everything on the card: the plain grade above already says
        // whether THAT fits (red on a small card, honestly).
        let tuning_now = crate::tuning::get(app, &m.name);
        let tuned_moe = tuning_now.moe_cpu_layers;
        // What the last measured load on this machine taught the split
        // budget - the loader applies it, so the grade and the Auto label
        // apply the same correction or they describe a different load.
        let correction = crate::llm::moe_calibration_read(app, &m.name)
            .map(|c| moe_budget_correction_gb(c.predicted_gb, c.actual_gb))
            .unwrap_or(0.0);
        let moe_auto_pick: Option<u32> = if meta.is_moe() {
            free_vram_gb.map(|vram| {
                if !moe_offload_wanted(need_gb, vram) {
                    0
                } else {
                    moe_cpu_layers_with(&meta, kv_gb, vram, correction)
                        .unwrap_or(meta.expert_bytes_per_layer.len()) as u32
                }
            })
        } else {
            None
        };
        if meta.is_moe() && tuned_moe == Some(0) {
            // No offload will be attempted; the full-weights grade stands.
        } else if meta.is_moe() && tuned_moe.is_some() {
            if let Some(vram) = free_vram_gb {
                let (f, pick) = tuned_split_fit(&meta, tuned_moe.unwrap(), kv_gb, vram, correction);
                fit = f;
                moe_offload = true;
                moe_cpu_layers_pick = pick;
            }
        } else if meta.is_moe() {
            if let Some(vram) = free_vram_gb {
                if moe_offload_wanted(need_gb, vram)
                    && moe_offload_fits(
                        weights_gb,
                        need_gb,
                        vram,
                        moe_ram_budget_gb(free_ram_gb, total_ram_gb),
                    )
                {
                    if moe_active_fraction(&meta).map_or(true, |f| f <= MOE_HIGH_ACTIVE_FRACTION) {
                        fit = Fit::Split;
                        moe_offload = true;
                        moe_cpu_layers_pick = moe_cpu_layers_with(&meta, kv_gb, vram, correction)
                            .filter(|&n| n < meta.expert_bytes_per_layer.len())
                            .map(|n| n as u32);
                    } else {
                        // The loader will still split it if the user runs
                        // it - but most of its width fires every token, so
                        // the grade tells the truth: works, slower.
                        fit = Fit::Yellow;
                    }
                }
            }
        }
        let measured_tps = stats.get(&m.name).and_then(|s| s.tps);
        let measured_tps_mlx = stats.get(&format!("mlx:{}", m.name)).and_then(|s| s.tps);
        let load_secs = stats.get(&m.name).and_then(|s| s.load_secs);
        // Evidence beats estimate: this machine has RUN this model (a
        // recorded speed stamp, on either engine - the MLX artifact of the
        // same model lives in the same memory) - "too large" would
        // contradict its own record. An estimated shortfall may claim
        // "runs slower" at worst.
        if fit == Fit::Red && (measured_tps.is_some() || measured_tps_mlx.is_some()) {
            fit = Fit::Yellow;
        }
        out.push(ModelFit {
            name: m.name,
            agent_template_ok: meta.agent_template_ok(),
            fit,
            need_gb,
            weights_gb,
            kv_gb,
            quant: meta.quant_label().to_string(),
            params_b,
            n_layers: meta.n_layers,
            context_max: meta.context_length,
            context_runtime: ctx,
            is_moe: meta.is_moe(),
            moe_auto_pick,
            tuned: !tuning_now.is_empty(),
            moe_offload,
            moe_cpu_layers: moe_cpu_layers_pick,
            measured_tps,
            measured_tps_mlx,
            load_secs,
        });
    }
    out
}

/// Command for the UI (fit badges per downloaded model).
#[tauri::command]
pub async fn assess_model_fit(app: AppHandle) -> Result<Vec<ModelFit>, String> {
    Ok(assess(&app).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reclaim_returns_incumbent_footprint_to_the_right_budget() {
        // GPU budget: incumbent's footprint comes back as VRAM.
        assert_eq!(reclaim_adjust(Some(1.0), 16.0, 7.0), (Some(8.0), 16.0));
        // CPU-only: it comes back as RAM instead.
        assert_eq!(reclaim_adjust(None, 10.0, 5.0), (None, 15.0));
        // No incumbent: both figures untouched.
        assert_eq!(reclaim_adjust(Some(8.0), 16.0, 0.0), (Some(8.0), 16.0));
    }

    /// The 0.4.0-beta.1 field case (4060 Ti, balanced lean): with a ~7 GB
    /// model loaded, raw free VRAM collapses to ~1 GB, EVERY candidate
    /// grades yellow, and tier-first ordering stops discriminating - so raw
    /// capability picks the 30B. With the incumbent's footprint reclaimed,
    /// the incumbent-class model grades green again and out-ranks the
    /// yellow 30B under balanced ordering.
    #[test]
    fn reclaim_keeps_tiers_discriminating_while_a_model_is_loaded() {
        let (free_raw, ram) = (Some(1.0), 18.0);
        // Without reclaim: both the 7 GB incumbent-class model and the
        // 13 GB big model grade red on a GPU machine - the tier can't tell
        // them apart.
        assert_eq!(grade(7.0, free_raw, ram), Fit::Red);
        assert_eq!(grade(13.0, free_raw, ram), Fit::Red);
        // With the incumbent's 7 GB reclaimed (card is really 8 GB):
        let (free, ram) = reclaim_adjust(free_raw, ram, 7.0);
        assert_eq!(grade(7.0, free, ram), Fit::Green);
        assert_eq!(grade(13.0, free, ram), Fit::Red);
    }

    /// The 2026-08-22 bench box: 8 GB card, 32 GB RAM (~26 free), the
    /// 21 GB Qwen3.6-35B-A3B. Bigger than the card -> offload wanted; the
    /// file fits RAM -> offload fits. The same file on a 16 GB box does not.
    #[test]
    fn moe_offload_rules() {
        assert!(moe_offload_wanted(22.8, 7.8));
        assert!(!moe_offload_wanted(4.0, 7.8));
        assert!(moe_offload_fits(21.0, 22.8, 7.8, 26.0));
        assert!(!moe_offload_fits(21.0, 22.8, 7.8, 12.0));
        // gpt-oss-20b (12 GB) on the same box.
        assert!(moe_offload_fits(12.0, 13.8, 7.8, 26.0));
        // The RAM budget is total-minus-reserve when free is lower: the
        // 31.8 GB box with 18 GB in use (13.8 free) still budgets 24.8 GB,
        // so the 21 GB file fits; a 16 GB box (reserve 6.4) budgets 9.6.
        assert!((moe_ram_budget_gb(13.8, 31.8) - 24.8).abs() < 0.01);
        assert!(moe_offload_fits(21.0, 22.8, 6.9, moe_ram_budget_gb(13.8, 31.8)));
        assert!(!moe_offload_fits(21.0, 22.8, 7.8, moe_ram_budget_gb(9.0, 15.8)));
    }

    #[test]
    fn grades_make_sense() {
        // 6 GB fits in 8 GB free (≤ 0.9×8); a dense 7.5 GB does not - and a
        // dense model loads fully on the card or not at all: red, not yellow.
        assert_eq!(grade(6.0, Some(8.0), 16.0), Fit::Green);
        assert_eq!(grade(7.5, Some(8.0), 16.0), Fit::Red);
        assert_eq!(grade(6.0, Some(12.0), 16.0), Fit::Green);
        assert_eq!(grade(40.0, Some(8.0), 16.0), Fit::Red);
        // Tiers: split is a top-tier grade, yellow below, red bottom.
        assert_eq!(Fit::Split.tier(), Fit::Green.tier());
        assert!(Fit::Yellow.tier() < Fit::Split.tier());
        assert!(Fit::Split.is_fast() && !Fit::Yellow.is_fast());
        // CPU-only
        assert_eq!(grade(4.0, None, 16.0), Fit::Green);
        assert_eq!(grade(13.0, None, 16.0), Fit::Yellow);
        assert_eq!(grade(20.0, None, 16.0), Fit::Red);
    }

    /// Synthetic header approximating a Muse-Glimmer-class 30B MoE 2-bit
    /// (48 layers, GQA 8 kv-heads, head_dim 128) and a gemma-class 4B Q4.
    fn meta(n_layers: u64, n_kv_heads: u64, head_dim: u64, trained_ctx: u64) -> GgufMeta {
        GgufMeta {
            architecture: "test".into(),
            template_tools: true,
            template_strict_alternation: false,
            size_label: String::new(),
            n_layers,
            n_heads: n_kv_heads * 4,
            n_kv_heads,
            embedding_length: n_kv_heads * 4 * head_dim,
            context_length: trained_ctx,
            key_length: head_dim,
            file_type: 0,
            n_experts: 0,
            n_experts_used: 0,
            ff_expert_len: 0,
            ff_shared_expert_len: 0,
            n_attn_layers: 0,
            expert_bytes_per_layer: Vec::new(),
            non_expert_bytes: 0,
            split_no: 0,
            split_count: 1,
            total_bytes: 0,
            ..GgufMeta::default()
        }
    }

    /// A synthetic MoE meta shaped like a real file's tensor split.
    #[test]
    fn active_fraction_separates_cold_experts_from_surgery_moes() {
        // Whittle-27B: 16-of-64 slivers of width 192 + a 5120-wide shared
        // expert = 8192 of 17408 every token. A split would crawl.
        let mut m = meta(64, 2, 256, 262144);
        m.n_experts = 64;
        m.n_experts_used = 16;
        m.ff_expert_len = 192;
        m.ff_shared_expert_len = 5120;
        let f = moe_active_fraction(&m).unwrap();
        assert!((f - 8192.0 / 17408.0).abs() < 1e-9);
        assert!(f > MOE_HIGH_ACTIVE_FRACTION);
        // Nemotron 3.5: 6-of-128 of width 1856 + shared 3712 = cold experts.
        m.n_experts = 128;
        m.n_experts_used = 6;
        m.ff_expert_len = 1856;
        m.ff_shared_expert_len = 3712;
        assert!(moe_active_fraction(&m).unwrap() < MOE_HIGH_ACTIVE_FRACTION);
        // No width metadata: fall back to the plain count ratio (8-of-128).
        m.ff_expert_len = 0;
        m.ff_shared_expert_len = 0;
        m.n_experts_used = 8;
        assert!((moe_active_fraction(&m).unwrap() - 0.0625).abs() < 1e-9);
        // Dense: no verdict.
        m.n_experts = 0;
        assert!(moe_active_fraction(&m).is_none());
    }

    fn moe_meta(n_layers: usize, expert_layer_gib: f64, non_expert_gib: f64) -> GgufMeta {
        let mut m = meta(n_layers as u64, 2, 256, 262144);
        m.n_experts = 256;
        m.n_experts_used = 8;
        m.expert_bytes_per_layer = vec![(expert_layer_gib * GIB) as u64; n_layers];
        m.non_expert_bytes = (non_expert_gib * GIB) as u64;
        m
    }

    /// The N-picker against the 2026-08-23 audit numbers (real tensor tables
    /// of the three bench models, 7.8 GiB free on the 4060 Ti): it must land
    /// on or just above the measured best N (34 vs 32, 14 vs 12, 27 vs 24) -
    /// never below, which is the oversubscription side.
    #[test]
    fn moe_cpu_layers_matches_the_audit() {
        // Qwen3.6-35B-A3B: 40 layers x ~0.48 GiB experts, 2.38 GiB other, KV@8k 0.62
        let qwen = moe_meta(40, 18.22 / 40.0, 2.38);
        assert_eq!(moe_cpu_layers(&qwen, 0.62, 7.8), Some(34));
        assert_eq!(moe_cpu_layers(&qwen, 0.62, 15.5), Some(17));
        assert_eq!(moe_cpu_layers(&qwen, 0.62, 23.5), Some(0));
        // gpt-oss-20b: 24 x 0.395, 1.33 other, KV 0.38
        let oss = moe_meta(24, 9.48 / 24.0, 1.33);
        assert_eq!(moe_cpu_layers(&oss, 0.38, 7.8), Some(14));
        assert_eq!(moe_cpu_layers(&oss, 0.38, 15.5), Some(0));
        // Gemma-4 26B-A4B: 30 x 0.399, 1.47 other, KV 3.28 (mean kv-heads)
        let gemma = moe_meta(30, 11.96 / 30.0, 1.47);
        assert_eq!(moe_cpu_layers(&gemma, 3.28, 7.8), Some(27));
        // A learned correction moves the pick: +1.5 GB (the estimate
        // over-counted) puts ~3 more layers on the card; -1 GB takes 2 off.
        assert_eq!(moe_cpu_layers_with(&qwen, 0.62, 7.8, 1.5), Some(31));
        assert_eq!(moe_cpu_layers_with(&qwen, 0.62, 7.8, -1.0), Some(36));
        assert!((moe_budget_correction_gb(6.6, 5.9) - 0.7).abs() < 1e-9);
        assert_eq!(moe_budget_correction_gb(6.6, 9.0), -1.0);
        assert_eq!(moe_budget_correction_gb(6.6, 3.0), 1.5);
        assert_eq!(moe_budget_correction_gb(6.6, 0.0), 0.0);
        // Unknown split -> None (caller pins everything).
        let dense_like = meta(40, 2, 256, 262144);
        assert_eq!(moe_cpu_layers(&dense_like, 0.62, 7.8), None);
        // Even all-on-CPU too big for the card -> Some(n_layers), still pin all.
        let tiny_card = moe_cpu_layers(&gemma, 3.28, 4.0);
        assert_eq!(tiny_card, Some(30));
    }

    #[test]
    fn kv_estimate_reads_windows_and_per_layer_heads() {
        // gemma4-shaped: 35 layers, 28 sliding at 512 tokens, K/V 512
        // full / 256 windowed, 1 KV head. At 32k the old math charged all
        // 35 layers at full context (~2.2 GB); the honest figure is the 7
        // global layers plus a whisper for the windowed ones.
        let mut g = meta(35, 1, 512, 131072);
        g.value_length = 512;
        g.sliding_window = 512;
        g.swa_pattern_read = true;
        g.swa_layers = 28;
        g.key_length_swa = 256;
        g.value_length_swa = 256;
        let (_, kv, _) = model_need(&g, 3 * 1024 * 1024 * 1024, 32768);
        assert!(kv < 0.55, "windowed layers must not pay full context: {kv}");
        assert!(kv > 0.40, "the 7 global layers still count: {kv}");
        // Same header without the window: all 35 at full context.
        let mut g2 = meta(35, 1, 512, 131072);
        g2.value_length = 512;
        let (_, kv_full, _) = model_need(&g2, 3 * 1024 * 1024 * 1024, 32768);
        assert!(kv_full > 2.0, "full attention keeps the old cost: {kv_full}");

        // lfm2-shaped hybrid: 24 layers, only 6 attention, per-layer head
        // sum 48 (mean 2 diluted by zeros). The sum wins: 8 heads on the
        // 6 real layers, not 2 on 24.
        let mut l = meta(24, 2, 64, 128000);
        l.n_attn_layers = 6;
        l.kv_heads_sum = 48;
        let (_, kv_l, _) = model_need(&l, 4 * 1024 * 1024 * 1024, 8192);
        let expected = 8.0 * 6.0 * (64.0 + 64.0) * 8192.0 * 2.0 / (1024.0 * 1024.0 * 1024.0);
        assert!((kv_l - expected).abs() < 0.01, "sum-based KV: {kv_l} vs {expected}");
    }

    #[test]
    fn choose_ctx_covers_the_configs_that_matter() {
        const GB: u64 = 1024 * 1024 * 1024;
        let muse = meta(48, 8, 128, 131072); // ~12GB file below
        let small = meta(30, 8, 256, 32768); // ~4.6GB file below

        // The flagship: 32GB machine (reports 31.8) + 8GB card (7.8 free).
        // A DENSE 12GB model cannot load on an 8GB card at any context
        // (fit-truth 09-01: the engine puts weights + cache in graphics
        // memory); the card-aware floor bottoms out at 4096 and the grade
        // is red regardless. (Real Muse is MoE and keeps its 16k via the
        // expert split - the MoE case below.)
        assert_eq!(choose_ctx(&muse, 12 * GB, 31.8, Some(7.8)), 4096);

        // Same machine, CPU-only (safe mode): RAM tier holds - 16384 for a
        // big model on a 30GB+ box.
        assert_eq!(choose_ctx(&muse, 12 * GB, 31.8, None), 16384);

        // Muse on a 24GB card: fully-on-GPU at 32k (weights 12 + kv ~6.4
        // + overhead < 21.6) - take the green rung.
        assert_eq!(choose_ctx(&muse, 12 * GB, 31.8, Some(24.0)), 32768);

        // Muse on a 16GB-RAM / 4GB-VRAM box: dense past the card - the
        // card-aware floor steps to the bottom rung (red either way).
        assert_eq!(choose_ctx(&muse, 12 * GB, 15.8, Some(4.0)), 4096);

        // Small model on an 8GB-RAM box (reports 7.8): tier says 8k, but a
        // 12GB card carries 16k+ KV fully on GPU - upgraded, RAM untouched.
        assert!(choose_ctx(&small, 46 * GB / 10, 7.8, Some(12.0)) >= 16384);

        // Small model, 12GB machine (reports 11.7), no GPU: the 32k small-
        // model tier (the agent runway) survives the usable-RAM reporting.
        assert_eq!(choose_ctx(&small, 46 * GB / 10, 11.7, None), 32768);

        // Never past the trained context: an 8k-trained model stays 8k on
        // any hardware.
        let short = meta(30, 8, 256, 8192);
        assert_eq!(choose_ctx(&short, 46 * GB / 10, 31.8, Some(24.0)), 8192);
    }

    #[test]
    fn model_need_on_real_models() {
        let dir = std::path::Path::new("/home/solar/.local/share/com.solar.yourowai/models");
        for name in ["gemma-4-E2B-it-Q4_K_M.gguf", "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf"] {
            let p = dir.join(name);
            if !p.exists() {
                continue;
            }
            let meta = crate::gguf::read_meta(&p).unwrap();
            let size = std::fs::metadata(&p).unwrap().len();
            let (w, kv, need) = model_need(&meta, size, 8192);
            eprintln!("{name}: weights={w:.2}GB kv@8k={kv:.2}GB need={need:.2}GB");
            assert!(w > 0.1 && need > w);
        }
    }
}

#[cfg(test)]
mod catalog_grade_tests {
    use super::*;

    fn v(size: f64, min_ram: f64, moe: bool) -> CatalogVariantIn {
        CatalogVariantIn { key: "x".into(), size_gb: size, min_ram_gb: min_ram, is_moe: moe }
    }

    #[test]
    fn dense_on_a_card_is_gpu_or_too_big_never_cpu() {
        let f = MachineFigures { total_ram_gb: 32.0, avail_ram_gb: 20.0, free_vram_gb: Some(7.0) };
        assert_eq!(grade_catalog_variant(&v(4.0, 8.0, false), &f, false).mode, "gpu");
        assert_eq!(grade_catalog_variant(&v(6.0, 8.0, false), &f, false).mode, "too-big");
    }

    #[test]
    fn moe_bigger_than_the_card_splits_when_ram_carries_it() {
        let f = MachineFigures { total_ram_gb: 32.0, avail_ram_gb: 20.0, free_vram_gb: Some(7.0) };
        assert_eq!(grade_catalog_variant(&v(21.0, 16.0, true), &f, false).mode, "moe-split");
        let small = MachineFigures { total_ram_gb: 16.0, avail_ram_gb: 8.0, free_vram_gb: Some(7.0) };
        assert_eq!(grade_catalog_variant(&v(21.0, 16.0, true), &small, false).mode, "too-big");
    }

    #[test]
    fn no_card_grades_against_available_ram_and_names_unified_gpu() {
        // The 8 GB Air: 2.3 GB models graded "Too large" from strict free
        // pages; against vm_stat's available figure they are green - and
        // the matrix loaded them at 29 tok/s.
        let air = MachineFigures { total_ram_gb: 8.0, avail_ram_gb: 5.5, free_vram_gb: None };
        let g = grade_catalog_variant(&v(2.3, 8.0, false), &air, true);
        assert_eq!((g.mode.as_str(), g.fit), ("gpu", Fit::Green));
        let cpu_box = MachineFigures { total_ram_gb: 8.0, avail_ram_gb: 5.5, free_vram_gb: None };
        assert_eq!(grade_catalog_variant(&v(2.3, 8.0, false), &cpu_box, false).mode, "cpu");
        assert_eq!(grade_catalog_variant(&v(4.0, 8.0, false), &cpu_box, false).fit, Fit::Yellow);
        assert_eq!(grade_catalog_variant(&v(9.0, 8.0, false), &cpu_box, false).mode, "too-big");
        assert_eq!(grade_catalog_variant(&v(2.3, 16.0, false), &cpu_box, false).mode, "too-big");
    }

    #[test]
    fn catalog_estimate_and_header_estimate_share_thresholds() {
        // Same grade() call underneath: a stand-in need that the card holds
        // at 90% is green exactly like a header-measured one.
        let f = MachineFigures { total_ram_gb: 32.0, avail_ram_gb: 20.0, free_vram_gb: Some(7.0) };
        let need = estimate_need_gb(5.0);
        assert_eq!(grade(need, f.free_vram_gb, f.avail_ram_gb), grade_catalog_variant(&v(5.0, 8.0, false), &f, false).fit);
    }
}

#[cfg(test)]
mod tuned_split_tests {
    use super::*;

    #[test]
    fn unreadable_table_is_runs_slower_with_pick_unknown_not_one_layer() {
        let meta = GgufMeta { n_layers: 24, n_experts: 32, n_experts_used: 4, ..Default::default() };
        assert_eq!(tuned_split_fit(&meta, 5, 0.3, 7.0, 0.0), (Fit::Yellow, None));
    }

    #[test]
    fn tuned_split_uses_the_picker_headroom_and_correction() {
        let meta = GgufMeta {
            n_layers: 24,
            n_experts: 32,
            n_experts_used: 4,
            expert_bytes_per_layer: vec![150_000_000; 24],
            non_expert_bytes: 1_000_000_000,
            ..Default::default()
        };
        // All experts on the CPU: ~0.93 GB non-expert + 0.3 kv + 0.8 = 2.03 on the card.
        let (fit, pick) = tuned_split_fit(&meta, 24, 0.3, 3.5, 0.0);
        assert_eq!((fit, pick), (Fit::Split, Some(24)));
        // Zero on the CPU on a small card: over the headroom - runs slower, pick kept.
        let (fit, pick) = tuned_split_fit(&meta, 0, 0.3, 3.5, 0.0);
        assert_eq!((fit, pick), (Fit::Yellow, Some(0)));
        // A learned correction widens the budget exactly like the loader's picker.
        let (fit, _) = tuned_split_fit(&meta, 0, 0.3, 3.5, 3.0);
        assert_eq!(fit, Fit::Split);
        // A pin past the layer count clamps to the table, never below it.
        assert_eq!(tuned_split_fit(&meta, 99, 0.3, 7.0, 0.0).1, Some(24));
    }
}
