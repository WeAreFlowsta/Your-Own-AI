//! Coarse capability ranking by model family — the "capability registry."
//!
//! Scores are **benchmark-informed, not param-count** (distillation/over-training
//! break the size↔quality link). They're intentionally coarse (0–10 overall,
//! 0–9 per task) — enough to rank a user's handful of downloaded models. The one
//! maintenance task: re-check these when model families update (same discipline
//! as re-measuring extraction on model bumps).

#[derive(Debug, Clone, Copy)]
pub struct Caps {
    /// Overall capability tier (0–10) — the primary offline-ranking signal.
    pub overall: u8,
    pub coding: u8,
    pub reasoning: u8,
    pub math: u8,
    pub vision: u8,
    /// Medical/clinical knowledge (labs, imaging, medications). Drives the
    /// "medical" routing task; high only for medical specialists.
    pub medical: u8,
}

impl Caps {
    /// The score that matters for a routing task. `"general"`/unknown → `overall`.
    /// Keep the task strings in sync with the router's task taxonomy.
    pub fn by_task(&self, task: &str) -> u8 {
        match task {
            "code" => self.coding,
            "math" => self.math,
            "reasoning" => self.reasoning,
            "vision" => self.vision,
            "medical" => self.medical,
            _ => self.overall,
        }
    }
}

/// Capability scores for a model, matched by family from its filename.
/// Caps when the family is RECOGNIZED; `None` for unknown names. External-
/// server scans use this to tell a real registry match from the default
/// (an unknown external model ranks conservatively, not confidently).
pub fn known_caps(model_name: &str) -> Option<Caps> {
    let n = model_name.to_lowercase();

    // Meta's Muse Glimmer (2026-08) - agent-FIRST local model: reliable
    // tool schemas over long workflows, error recovery, vision encoder.
    // Scores track Meta's agentic-suite results vs same-class peers.
    // Early llama.cpp support - re-rank after real field use.
    if n.contains("muse-glimmer") {
        return Some(Caps { overall: 8, coding: 8, reasoning: 8, math: 7, vision: 7, medical: 3 });
    }
    // Agentic coders built on a reasoning base (coding-first, also strong reasoning).
    // Listed before the generic coder rule so they keep their higher reasoning score.
    // 1.5's 35B carries a vision projector; the 9B does not.
    if n.contains("ornith-1.5-35b") {
        return Some(Caps { overall: 8, coding: 9, reasoning: 8, math: 7, vision: 7, medical: 3 });
    }
    if n.contains("ornith") {
        return Some(Caps { overall: 8, coding: 9, reasoning: 8, math: 7, vision: 0, medical: 3 });
    }
    // OpenAI open-weight — reasoning + agentic + function-calling (not a pure coder).
    if n.contains("gpt-oss") || n.contains("gpt_oss") || n.contains("gptoss") {
        return Some(Caps { overall: 8, coding: 7, reasoning: 8, math: 7, vision: 0, medical: 6 });
    }
    // NVIDIA Nemotron 3.5 Lightning (2026-08): 30B-A3B hybrid reasoning model -
    // reasoning/agentic/coding in the gpt-oss class. Re-rank after field use.
    if n.contains("nemotron-3.5") || n.contains("nemotron-3-5") {
        return Some(Caps { overall: 8, coding: 8, reasoning: 8, math: 7, vision: 0, medical: 3 });
    }
    // Small MoEs for ordinary machines (2026 catalog adds): capable chat /
    // instruction-following / tool use for their size, not frontier coders.
    if n.contains("lfm2") {
        return Some(Caps { overall: 6, coding: 5, reasoning: 6, math: 5, vision: 0, medical: 3 });
    }
    if n.contains("granite-4") {
        return Some(Caps { overall: 6, coding: 6, reasoning: 6, math: 5, vision: 0, medical: 3 });
    }
    if n.contains("ling-mini") {
        return Some(Caps { overall: 6, coding: 6, reasoning: 6, math: 6, vision: 0, medical: 3 });
    }
    // GLM (Zhipu) — strong all-rounder, agentic coding + reasoning + tool use.
    if n.contains("glm") {
        return Some(Caps { overall: 8, coding: 8, reasoning: 8, math: 7, vision: 0, medical: 3 });
    }
    // Qwythos — a Qwen3.5-VL community merge: multimodal + reasoning/agentic (not a coder).
    if n.contains("qwythos") {
        return Some(Caps { overall: 7, coding: 5, reasoning: 7, math: 5, vision: 7, medical: 3 });
    }
    // Coding specialists + agentic coders (Qwen-Coder, Codestral, Devstral, …).
    if n.contains("coder") || n.contains("-code") || n.contains("codestral") || n.contains("devstral") {
        return Some(Caps { overall: 7, coding: 9, reasoning: 6, math: 6, vision: 0, medical: 3 });
    }
    // Medical specialists (MedGemma). Deliberately MODEST overall so the Auto
    // modes never prefer them for general chat - they're clinically flavored
    // and meant to be pinned to a dedicated health AI. Strong vision (their
    // image encoder is trained on X-rays/derm/path/fundus). Before the gemma
    // arms: "medgemma" filenames would otherwise match plain "gemma".
    if n.contains("medgemma") {
        return Some(if n.contains("27b") {
            Caps { overall: 6, coding: 3, reasoning: 7, math: 5, vision: 8, medical: 10 }
        } else {
            Caps { overall: 5, coding: 3, reasoning: 6, math: 4, vision: 8, medical: 9 }
        });
    }
    // Reasoning / chain-of-thought (distills) — strong math/reasoning, high token cost
    if n.contains("deepseek") || n.contains("-r1") || n.contains("qwq") {
        return Some(Caps { overall: 8, coding: 6, reasoning: 9, math: 9, vision: 0, medical: 3 });
    }
    // Empero's Qwen 3.8 distills (Aug 2026): the flagship's knowledge pressed
    // into 2B/4B/9B on the Qwen3.5 arch - strong FOR SIZE, text-only. Must
    // precede the flagship arm below or a 2B would score like a frontier 27B
    // and hijack Auto's rankings.
    if n.contains("qwen3.8-9b") {
        return Some(Caps { overall: 8, coding: 7, reasoning: 8, math: 8, vision: 0, medical: 3 });
    }
    if n.contains("qwen3.8-4b") {
        return Some(Caps { overall: 7, coding: 6, reasoning: 7, math: 7, vision: 0, medical: 3 });
    }
    if n.contains("qwen3.8-2b") {
        return Some(Caps { overall: 6, coding: 5, reasoning: 6, math: 6, vision: 0, medical: 3 });
    }
    // Qwen 3.8 (Aug 2026): frontier-class dense 27B with native vision.
    // Must precede the generic qwen3 arm ("qwen3.8" contains "qwen3").
    if n.contains("qwen3.8") || n.contains("qwen-3.8") {
        return Some(Caps { overall: 9, coding: 9, reasoning: 9, math: 9, vision: 7, medical: 3 });
    }
    // Strong all-rounders (current best per-size)
    if n.contains("qwen3") || n.contains("qwen-3") {
        return Some(Caps { overall: 8, coding: 8, reasoning: 8, math: 8, vision: 0, medical: 3 });
    }
    if n.contains("phi-4") || n.contains("phi4") {
        return Some(Caps { overall: 7, coding: 6, reasoning: 7, math: 8, vision: 0, medical: 3 });
    }
    if n.contains("gemma-4") || n.contains("gemma4") || n.contains("gemma-3") || n.contains("gemma3") {
        return Some(Caps { overall: 7, coding: 6, reasoning: 6, math: 6, vision: 7, medical: 3 });
    }
    if n.contains("ministral") || n.contains("mistral") {
        return Some(Caps { overall: 6, coding: 6, reasoning: 6, math: 5, vision: 0, medical: 3 });
    }
    if n.contains("llama-3") || n.contains("llama3") || n.contains("llama-4") || n.contains("llama4") {
        return Some(Caps { overall: 6, coding: 5, reasoning: 6, math: 5, vision: 0, medical: 3 });
    }
    if n.contains("qwen2") {
        return Some(Caps { overall: 6, coding: 7, reasoning: 6, math: 6, vision: 0, medical: 3 });
    }
    if n.contains("gemma") {
        return Some(Caps { overall: 5, coding: 5, reasoning: 5, math: 5, vision: 3, medical: 3 });
    }
    if n.contains("phi") {
        return Some(Caps { overall: 5, coding: 5, reasoning: 6, math: 6, vision: 0, medical: 3 });
    }
    None
}

/// Agent/tool-use capability (0-9): can this family DRIVE tools reliably
/// in an agent loop - function calling plus multi-step discipline? Kept
/// separate from `Caps` so the router's task scoring is untouched; the
/// folder guard is the consumer. Formalizes what the registry comments
/// above already know. Same maintenance discipline: re-check on family
/// updates, and PROMOTE a family only after a live folder test.
pub fn agent_caps(model_name: &str) -> u8 {
    let n = model_name.to_lowercase();
    if n.contains("ornith") {
        return 9;
    }
    if n.contains("gpt-oss") || n.contains("gpt_oss") || n.contains("gptoss") {
        return 8;
    }
    if n.contains("glm") {
        return 8;
    }
    // Conservative until a live folder test promotes them (registry rule).
    if n.contains("nemotron-3.5") || n.contains("nemotron-3-5") {
        return 7;
    }
    if n.contains("lfm2") {
        return 6;
    }
    if n.contains("granite-4") || n.contains("ling-mini") {
        return 5;
    }
    if n.contains("coder") || n.contains("-code") || n.contains("codestral") || n.contains("devstral") {
        return 8;
    }
    // The 2B/4B/9B distills are Qwen3.5-ARCH (its chat template, not the
    // flagship's Developer-role format) - the generic qwen3 tier fits them.
    if n.contains("qwen3.8-9b") || n.contains("qwen3.8-4b") || n.contains("qwen3.8-2b") {
        return 7;
    }
    // Qwen 3.8 shipped a dedicated tool-call format ("Developer role") and
    // upstream llama.cpp grew a qwen3 parser for it. Above generic qwen3.
    if n.contains("qwen3.8") || n.contains("qwen-3.8") {
        return 8;
    }
    if n.contains("qwen3") || n.contains("qwen-3") {
        return 7;
    }
    if n.contains("qwythos") {
        return 6;
    }
    if n.contains("deepseek") || n.contains("-r1") || n.contains("qwq") {
        return 5;
    }
    if n.contains("ministral") || n.contains("mistral") {
        return 5;
    }
    if n.contains("medgemma") {
        return 1;
    }
    if n.contains("gemma-4") || n.contains("gemma4") {
        return 5;
    }
    if n.contains("llama") {
        return 4;
    }
    3
}

/// Online agent capability: what the PROXY PATH delivers today, not the
/// raw model.
pub fn online_agent_caps(text: &str) -> u8 {
    let t = text.to_lowercase();
    if t.contains("claude") {
        return 9;
    }
    if t.contains("kimi") {
        return 8;
    }
    if t.contains("coder") || t.contains("codestral") {
        return 8;
    }
    if t.contains("gpt") || t.contains("openai") {
        return 8;
    }
    if t.contains("grok") {
        return 8;
    }
    if t.contains("gemini") {
        return 7;
    }
    if t.contains("qwen") {
        return 7;
    }
    if t.contains("sonar") || t.contains("perplexity") {
        return 1;
    }
    5
}

/// Offline agent readiness for the folder guard's AUTO case: with an
/// offline-only auto mode, can any downloaded model drive agent work at
/// all - and comfortably? Cheap since GGUF metadata is cached.
#[derive(serde::Serialize)]
pub struct OfflineAgentReadiness {
    /// Some downloaded model is tool-capable (agent_caps >= 6).
    pub capable: bool,
    /// ...and at least one such model fits green on this hardware.
    pub comfortable: bool,
}

#[tauri::command]
pub async fn offline_agent_readiness(app: tauri::AppHandle) -> OfflineAgentReadiness {
    let fits = crate::fit::assess(&app).await;
    let capable_models: Vec<_> = fits
        .iter()
        .filter(|f| agent_caps(&f.name) >= 6 && f.agent_template_ok)
        .collect();
    OfflineAgentReadiness {
        capable: !capable_models.is_empty(),
        comfortable: capable_models
            .iter()
            .any(|f| f.fit.is_fast()),
    }
}

/// The folder guard's one question: can this AI's configured model drive
/// agent work? `model` is UserDefinedAI.model - a gguf filename,
/// "online:<id>", "external:<id>", or "auto:*" (router decides per turn,
/// returns -1: the caller treats auto as "depends, do not block").
#[tauri::command]
pub fn agent_capability(model: String) -> i32 {
    if model.starts_with("auto:") {
        return -1;
    }
    if let Some(id) = model.strip_prefix("online:") {
        return online_agent_caps(id) as i32;
    }
    if let Some(id) = model.strip_prefix("external:") {
        return agent_caps(id) as i32;
    }
    agent_caps(&model) as i32
}

/// Capability scores for a model, matched by family from its filename.
/// Unknown family — middling default.
pub fn caps_for(model_name: &str) -> Caps {
    known_caps(model_name)
        .unwrap_or(Caps { overall: 4, coding: 4, reasoning: 4, math: 4, vision: 0, medical: 3 })
}

/// A specialist's value is one task, not general chat (today: medical
/// models). Auto's GENERAL picks exclude them - score tuning alone can't:
/// on a machine whose generalists are all small, a 4B medical model topped
/// the general pool and the keep-loaded rule then held it for every turn.
/// The task router still brings a specialist in when its task fires.
pub fn is_specialist(model_name: &str) -> bool {
    caps_for(model_name).medical >= 8
}

/// Capability scores for an ONLINE (cloud) model, matched by family from its
/// id/display-name/description. Frontier models cluster high; what matters is the
/// RELATIVE rank used to pick the best online model for a task. Benchmark-informed
/// and intentionally coarse — RE-CHECK when the proxy's model line-up changes
/// (same discipline as the offline registry). NOT a freshness signal: search
/// capability is handled separately by the router (a Sonar etc. can be a weak
/// reasoner but the right pick for a live-web query).
/// Capability scores for a catalog model: the catalog's own block when it
/// carries one, else the name-based registry below. A new model or a new
/// provider gets routed right the day the catalog says so.
pub fn online_caps_of(m: &crate::flowsta::OnlineModel) -> Caps {
    if let Some([overall, coding, reasoning, math, vision, medical]) = m.routing.as_ref().and_then(|r| r.caps) {
        return Caps { overall, coding, reasoning, math, vision, medical };
    }
    online_caps_for(&format!("{} {} {}", m.id, m.display_name, m.description))
}

/// Tool-driving capability for a catalog model: the catalog's `tools`
/// when declared, else the name-based table.
pub fn online_agent_caps_of(m: &crate::flowsta::OnlineModel) -> u8 {
    if let Some(t) = m.routing.as_ref().and_then(|r| r.tools) {
        return t;
    }
    online_agent_caps(&format!("{} {} {}", m.id, m.display_name, m.description))
}

pub fn online_caps_for(text: &str) -> Caps {
    let t = text.to_lowercase();
    // Coding specialists
    if t.contains("coder") || t.contains("codestral") {
        return Caps { overall: 8, coding: 9, reasoning: 7, math: 7, vision: 0, medical: 3 };
    }
    // Everyday tier: the fast, low-cost siblings of the flagships (Luna,
    // Flash, -mini, lite). A notch under their flagship so an Everyday slot
    // is a real rung below Hard. "-mini", not "mini": gemini.
    if t.contains("luna") || t.contains("flash") || t.contains("-mini") || t.contains("-lite") || t.contains(" lite") {
        return Caps { overall: 8, coding: 8, reasoning: 8, math: 8, vision: 8, medical: 3 };
    }
    if t.contains("kimi") || t.contains("moonshot") {
        return Caps { overall: 8, coding: 8, reasoning: 8, math: 7, vision: 7, medical: 3 };
    }
    // Reasoning / math distills (DeepSeek-R1, QwQ, …)
    if t.contains("deepseek") || t.contains("-r1") || t.contains("qwq") {
        return Caps { overall: 9, coding: 8, reasoning: 9, math: 9, vision: 0, medical: 3 };
    }
    if t.contains("claude") {
        return Caps { overall: 9, coding: 9, reasoning: 9, math: 8, vision: 8, medical: 3 };
    }
    if t.contains("gpt") || t.contains("openai") {
        return Caps { overall: 9, coding: 8, reasoning: 9, math: 8, vision: 8, medical: 3 };
    }
    if t.contains("gemini") {
        return Caps { overall: 8, coding: 7, reasoning: 8, math: 8, vision: 8, medical: 3 };
    }
    if t.contains("grok") {
        return Caps { overall: 8, coding: 8, reasoning: 8, math: 8, vision: 7, medical: 3 };
    }
    if t.contains("qwen") {
        return Caps { overall: 8, coding: 8, reasoning: 8, math: 8, vision: 0, medical: 3 };
    }
    if t.contains("mistral") || t.contains("mixtral") {
        return Caps { overall: 7, coding: 7, reasoning: 7, math: 6, vision: 0, medical: 3 };
    }
    if t.contains("llama") {
        return Caps { overall: 7, coding: 6, reasoning: 7, math: 6, vision: 0, medical: 3 };
    }
    // Search-first models (Sonar/Perplexity) — strong at live web, weaker pure reasoners.
    if t.contains("sonar") || t.contains("perplexity") {
        return Caps { overall: 6, coding: 5, reasoning: 6, math: 5, vision: 0, medical: 3 };
    }
    Caps { overall: 6, coding: 6, reasoning: 6, math: 6, vision: 0, medical: 3 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_task_prefers_a_coder_for_code() {
        let coder = caps_for("qwen2.5-coder-7b-instruct-q4_k_m.gguf");
        let general = caps_for("Phi-4-mini-instruct-Q4_K_M.gguf");
        // The coder wins on a CODE task...
        assert_eq!(coder.by_task("code"), 9);
        assert_eq!(general.by_task("code"), 6);
        // ...by more than SWITCH_MARGIN (2) → worth a reload.
        assert!(coder.by_task("code") >= general.by_task("code") + 2);
    }

    #[test]
    fn by_task_general_and_unknown_fall_back_to_overall() {
        let m = caps_for("Phi-4-mini-instruct-Q4_K_M.gguf");
        assert_eq!(m.by_task("general"), m.overall);
        assert_eq!(m.by_task("anything-else"), m.overall);
        // A general task must NOT spuriously rank one general model over another
        // any differently than overall does.
        let a = caps_for("gemma-4-E2B.gguf");
        assert_eq!(a.by_task("general"), a.overall);
    }

    #[test]
    fn online_caps_rank_the_right_model_per_task() {
        let coder = online_caps_for("mistral/codestral-latest");
        let claude = online_caps_for("anthropic/claude-3.7-sonnet");
        let sonar = online_caps_for("perplexity/sonar-pro");
        let deepseek = online_caps_for("deepseek/deepseek-r1");
        // For CODE, a coder or Claude beats a search-first model.
        assert!(coder.by_task("code") > sonar.by_task("code"));
        assert!(claude.by_task("code") > sonar.by_task("code"));
        // For hard REASONING/MATH, DeepSeek-R1 is top-tier (>= the all-rounders).
        assert!(deepseek.by_task("reasoning") >= claude.by_task("reasoning"));
        assert!(deepseek.by_task("math") > sonar.by_task("math"));
        // Unknown online model → middling default, not zero.
        assert_eq!(online_caps_for("some-new-model-x").overall, 6);
    }

    #[test]
    fn agentic_coders_in_catalog_rank_as_coders_not_unknown() {
        // Regression: these all ship in the offline catalog with a "coding" chip but
        // used to fall through caps_for to the unknown default (4), so the router
        // would never pick them for code. Each must now rank as a real coder.
        for f in [
            "Devstral-Small-2-24B-Instruct-2512-Q4_K_M.gguf",
            "ornith-1.0-9b-Q4_K_M.gguf",
            "Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf",
        ] {
            assert_eq!(caps_for(f).by_task("code"), 9, "{f} should rank as a coder");
        }
        // GPT-OSS is reasoning/agentic, not a pure coder, but must beat 'unknown'.
        let oss = caps_for("gpt-oss-20b-Q4_K_M.gguf");
        assert!(oss.overall >= 7 && oss.by_task("reasoning") >= 8);
        assert!(oss.overall > caps_for("some-unknown-model.gguf").overall);
        // GLM is a strong all-rounder, not 'unknown'.
        let glm = caps_for("GLM-4.7-Flash-Q4_K_M.gguf");
        assert!(glm.overall >= 8 && glm.by_task("code") >= 8);
    }
}

#[cfg(test)]
mod specialist_tests {
    use super::is_specialist;

    #[test]
    fn qwen38_distills_score_below_the_flagship() {
        let flagship = super::caps_for("Qwen3.8-27B-Q4_K_M.gguf");
        let d9 = super::caps_for("Qwen3.8-9B-Q4_K_M.gguf");
        let d2 = super::caps_for("Qwen3.8-2B-Q4_K_M.gguf");
        assert!(d9.overall < flagship.overall, "9B distill must rank below the 27B");
        assert!(d2.overall < d9.overall, "2B must rank below 9B");
        assert_eq!(d9.vision, 0, "distills are text-only");
    }

    #[test]
    fn medgemma_is_a_specialist_generalists_are_not() {
        assert!(is_specialist("medgemma-1.5-4b-it-Q4_K_M.gguf"));
        assert!(is_specialist("medgemma-27b-it-Q4_K_M.gguf"));
        assert!(!is_specialist("gemma-4-E2B-it-Q4_K_M.gguf"));
        assert!(!is_specialist("Phi-4-mini-instruct-Q4_K_M.gguf"));
        assert!(!is_specialist("Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf"));
        assert!(!is_specialist("Ministral-3-3B-Instruct-2512-Q4_K_M.gguf"));
        assert!(!is_specialist("totally-unknown-model.gguf"));
    }
}


/// One model's routing view for the Settings overview: the capability
/// registry's scores plus the agent tier. Read-only; the numbers the router
/// actually ranks on.
#[derive(serde::Serialize, Clone, Debug)]
pub struct CapsRow {
    pub name: String,
    pub overall: u8,
    pub coding: u8,
    pub reasoning: u8,
    pub math: u8,
    pub vision: u8,
    pub medical: u8,
    pub agent: u8,
    /// True when the family is in the registry (else middling defaults).
    pub known: bool,
}

#[tauri::command]
pub fn model_caps_for(names: Vec<String>) -> Vec<CapsRow> {
    names
        .into_iter()
        .map(|name| {
            let c = caps_for(&name);
            CapsRow {
                known: known_caps(&name).is_some(),
                overall: c.overall,
                coding: c.coding,
                reasoning: c.reasoning,
                math: c.math,
                vision: c.vision,
                medical: c.medical,
                agent: agent_caps(&name),
                name,
            }
        })
        .collect()
}
