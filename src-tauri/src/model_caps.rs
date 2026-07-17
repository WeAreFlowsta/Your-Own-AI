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

    // Agentic coders built on a reasoning base (coding-first, also strong reasoning).
    // Listed before the generic coder rule so they keep their higher reasoning score.
    if n.contains("ornith") {
        return Some(Caps { overall: 8, coding: 9, reasoning: 8, math: 7, vision: 0, medical: 3 });
    }
    // OpenAI open-weight — reasoning + agentic + function-calling (not a pure coder).
    if n.contains("gpt-oss") || n.contains("gpt_oss") || n.contains("gptoss") {
        return Some(Caps { overall: 8, coding: 7, reasoning: 8, math: 7, vision: 0, medical: 6 });
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
pub fn online_caps_for(text: &str) -> Caps {
    let t = text.to_lowercase();
    // Coding specialists
    if t.contains("coder") || t.contains("codestral") {
        return Caps { overall: 8, coding: 9, reasoning: 7, math: 7, vision: 0, medical: 3 };
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
