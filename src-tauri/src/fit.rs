//! VRAM/RAM fit for downloaded models — does a model fit, and how well?
//!
//! Uses the GGUF header (exact layers / GQA kv-heads / head_dim / quant) for the
//! weights + KV-cache size, and the **free** VRAM (after the webview's usage)
//! vs total. Grades each downloaded model GREEN (fits fully on GPU → fast),
//! YELLOW (needs CPU offload → slow), RED (won't run). The router prefers GREEN;
//! the UI can badge each model.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::gguf::GgufMeta;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Green,
    Yellow,
    Red,
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
    /// Header-derived (exact) — replaces the often-"Unknown" filename guesses.
    pub quant: String,
    pub params_b: f64, // approx, from size_bytes / effective-bpw
    pub n_layers: u64,
    pub context_max: u64,
    /// The context the server would actually start this model with on this
    /// machine right now (choose_ctx) - what "runs at" means in the UI.
    pub context_runtime: u64,
}

/// The context sizes the server can start at.
const CTX_LADDER: [u64; 4] = [32768, 16384, 8192, 4096];

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
    let kv_gb = 2.0
        * meta.n_layers as f64
        * meta.n_kv_heads as f64
        * meta.head_dim() as f64
        * eff_ctx
        * 2.0
        / GIB;
    let overhead_gb = 0.8;
    (weights_gb, kv_gb, weights_gb + kv_gb + overhead_gb)
}

/// Grade fit. GPU: GREEN fits in (90% of) free VRAM, YELLOW fits VRAM+RAM
/// (partial offload), else RED. CPU-only (`free_vram_gb` = None): GREEN if it
/// comfortably fits RAM, YELLOW if tight, else RED.
pub fn grade(need_gb: f64, free_vram_gb: Option<f64>, free_ram_gb: f64) -> Fit {
    match free_vram_gb {
        Some(vram) => {
            if need_gb <= 0.9 * vram {
                Fit::Green
            } else if need_gb <= vram + free_ram_gb {
                Fit::Yellow
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
    Some(app.path().app_data_dir().ok()?.join("models"))
}

/// Assess every downloaded model. Reuses free-VRAM (cached) + system RAM.
pub async fn assess(app: &AppHandle) -> Vec<ModelFit> {
    let models = crate::llm::list_local_models(app.clone()).await.unwrap_or_default();
    let dir = match models_dir(app) {
        Some(d) => d,
        None => return vec![],
    };

    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_ram_gb = sys.total_memory() as f64 / GIB;
    let free_ram_gb = sys.available_memory() as f64 / GIB;
    let free_vram_gb = crate::llm::available_vram_mib(app)
        .await
        .map(|mib| mib as f64 / 1024.0);

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
    let reclaim_gb = incumbent
        .and_then(|name| {
            let path = dir.join(&name);
            let meta = crate::gguf::read_meta(&path).ok()?;
            let size = std::fs::metadata(&path).ok()?.len();
            let ctx = choose_ctx(&meta, size, total_ram_gb, free_vram_gb);
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

    let mut out = Vec::new();
    for m in models {
        let path = dir.join(&m.name);
        let Ok(meta) = crate::gguf::read_meta(&path) else {
            continue; // skip unreadable files
        };
        if meta.is_embedding() {
            continue; // bge etc. — not a chat model; never a routing candidate
        }
        // Grade at the context the server would actually start this model
        // with - per model, since choose_ctx is VRAM- and size-aware.
        let ctx = choose_ctx(&meta, m.size_bytes, total_ram_gb, free_vram_gb);
        let (weights_gb, kv_gb, mut need_gb) = model_need(&meta, m.size_bytes, ctx);
        // A model with a downloaded projector (mmproj) auto-loads it for vision, so
        // its ~1 GB lives in VRAM whenever this model runs — count it toward fit.
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
        out.push(ModelFit {
            name: m.name,
            agent_template_ok: meta.agent_template_ok(),
            fit: grade(need_gb, free_vram_gb, free_ram_gb),
            need_gb,
            weights_gb,
            kv_gb,
            quant: meta.quant_label().to_string(),
            params_b,
            n_layers: meta.n_layers,
            context_max: meta.context_length,
            context_runtime: ctx,
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
        // 13 GB big model grade yellow - the tier can't tell them apart.
        assert_eq!(grade(7.0, free_raw, ram), Fit::Yellow);
        assert_eq!(grade(13.0, free_raw, ram), Fit::Yellow);
        // With the incumbent's 7 GB reclaimed (card is really 8 GB):
        let (free, ram) = reclaim_adjust(free_raw, ram, 7.0);
        assert_eq!(grade(7.0, free, ram), Fit::Green);
        assert_eq!(grade(13.0, free, ram), Fit::Yellow);
    }

    #[test]
    fn grades_make_sense() {
        // 6 GB fits in 8 GB free (≤ 0.9×8); 7.5 GB needs offload (yellow).
        assert_eq!(grade(6.0, Some(8.0), 16.0), Fit::Green);
        assert_eq!(grade(7.5, Some(8.0), 16.0), Fit::Yellow);
        assert_eq!(grade(6.0, Some(12.0), 16.0), Fit::Green);
        assert_eq!(grade(40.0, Some(8.0), 16.0), Fit::Red);
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
        }
    }

    #[test]
    fn choose_ctx_covers_the_configs_that_matter() {
        const GB: u64 = 1024 * 1024 * 1024;
        let muse = meta(48, 8, 128, 131072); // ~12GB file below
        let small = meta(30, 8, 256, 32768); // ~4.6GB file below

        // The flagship: 32GB machine (reports 31.8) + 8GB card (7.8 free).
        // Muse partial-offloads; the agent floor must give it 16k.
        assert_eq!(choose_ctx(&muse, 12 * GB, 31.8, Some(7.8)), 16384);

        // Same machine, CPU-only (safe mode): RAM tier holds - 16384 for a
        // big model on a 30GB+ box.
        assert_eq!(choose_ctx(&muse, 12 * GB, 31.8, None), 16384);

        // Muse on a 24GB card: fully-on-GPU at 32k (weights 12 + kv ~6.4
        // + overhead < 21.6) - take the green rung.
        assert_eq!(choose_ctx(&muse, 12 * GB, 31.8, Some(24.0)), 32768);

        // Muse on a 16GB-RAM / 4GB-VRAM box: 16k does not fit the pools
        // (need ~19GB vs 4 + 9.6) - stays at the 8k RAM tier.
        assert_eq!(choose_ctx(&muse, 12 * GB, 15.8, Some(4.0)), 8192);

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
