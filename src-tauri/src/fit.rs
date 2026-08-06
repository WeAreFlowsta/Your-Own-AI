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
}

/// Context the server will actually run at (matches `start_llama_server`'s
/// RAM-tier policy), used for the KV-cache term.
pub fn runtime_ctx(total_ram_gb: f64) -> u64 {
    if total_ram_gb >= 32.0 {
        16384
    } else if total_ram_gb >= 12.0 {
        8192
    } else {
        4096
    }
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
    let ctx = runtime_ctx(total_ram_gb);
    let free_vram_gb = crate::llm::available_vram_mib(app)
        .await
        .map(|mib| mib as f64 / 1024.0);

    let mut out = Vec::new();
    for m in models {
        let path = dir.join(&m.name);
        let Ok(meta) = crate::gguf::read_meta(&path) else {
            continue; // skip unreadable files
        };
        if meta.is_embedding() {
            continue; // bge etc. — not a chat model; never a routing candidate
        }
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
