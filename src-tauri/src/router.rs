//! Model routing for the Auto modes.
//!
//! A single Rust router that both the in-app chat (via the `route_model`
//! command) and the inference server call, so there's one source of truth and
//! no TS/Rust drift. It resolves an `auto:*` model to a concrete model (a GGUF
//! filename or `online:<id>`) for a given query; callers then run their normal
//! offline/online logic on the resolved value.
//!
//! Step 1 (this): minimal resolver — offline picks an available model; the
//! online+offline mode uses a cheap keyword freshness gate. Later steps add the
//! capability registry, exact VRAM fit, and the bge-small semantic gate.

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Clone)]
pub struct RouteResult {
    /// The concrete model to use: a GGUF filename or `online:<id>`.
    pub model: String,
    /// Short human-readable reason (for logs).
    pub reason: String,
}

const QUERY_INSTRUCTION: &str = "Represent this sentence for searching relevant passages: ";
const EMBED_MODEL: &str = "bge-small-en-v1.5-f16.gguf";

/// How much better (on the 0–9 task scale) a candidate must be than the loaded
/// model to justify a reload. Generous so near-equal models don't thrash; small
/// enough that a real coder (coding 9) beats a general model (coding 6) on code.
pub(crate) const SWITCH_MARGIN: u8 = 2;

/// Freshness threshold (max cosine vs the reference phrases) per eagerness
/// setting. Calibrated against bge-small on real queries: fresh queries cluster
/// ≥0.57, evergreen ≤0.564. Balanced (0.58) keeps evergreen local while catching
/// clearly-fresh paraphrases; privacy-first only escalates obvious cases;
/// freshness-first leans online.
fn threshold_for(eagerness: &str) -> f32 {
    match eagerness {
        "privacy" => 0.62,
        "freshness" => 0.52,
        _ => 0.58, // balanced (default)
    }
}

/// Reference phrases that exemplify "needs current / up-to-date info." Embedded
/// once (document-style, no instruction prefix) and cached; a query is compared
/// against all of them.
const FRESH_REFERENCES: &[&str] = &[
    "what's the latest news",
    "today's headlines",
    "current weather forecast today",
    "live sports score who won the game",
    "current stock price right now",
    "what happened recently in the news this week",
    "latest release version of the software",
    "who is the current president leader right now",
    "what's trending right now",
    "recent events and announcements this week",
    "exchange rate today",
    "is it raining now",
    "newest model phone released this year",
    "what time is it currently",
    // Upcoming-fixture register ("odds of X beating Y in sundays match"
    // scored 0.557 - under balanced 0.58 - without it; 0.688 with. Chosen
    // over an "odds"-worded ref, which pulled evergreen betting-education
    // questions over the line. Measured 2026-07-17.)
    "who is favored to win this weekends game",
];

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Reference embeddings, computed once on first use (needs the embedding server).
static FRESH_REFS: tokio::sync::OnceCell<Vec<Vec<f32>>> = tokio::sync::OnceCell::const_new();

async fn fresh_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    FRESH_REFS
        .get_or_try_init(|| async {
            let state = app.state::<crate::llm::LLMState>();
            let texts = FRESH_REFERENCES.iter().map(|s| s.to_string()).collect();
            crate::llm::embed_texts(app.clone(), state, texts, EMBED_MODEL.to_string()).await
        })
        .await
        .ok()
}

/// Stage-1 semantic freshness: how close (max cosine) is the query to the
/// "needs-current-info" reference phrases? `None` if embedding is unavailable
/// (→ caller treats as not-fresh, i.e. stays offline). Catches paraphrases the
/// keyword gate misses ("what's the newest phone", "is it raining").
async fn semantic_fresh_score(
    app: &AppHandle,
    query: &str,
    provided_vec: Option<&[f32]>,
) -> Option<f32> {
    let refs = fresh_reference_vecs(app).await?;
    // The frontend embeds the turn's text ONCE (same bge query instruction)
    // and shares the vector here and with memory retrieval - reuse it rather
    // than paying a second embed-server round trip. Callers without one
    // (the inference API) fall back to embedding here.
    let qvec: Vec<f32> = match provided_vec {
        Some(v) if !v.is_empty() => v.to_vec(),
        _ => {
            let state = app.state::<crate::llm::LLMState>();
            let qtext = format!("{QUERY_INSTRUCTION}{query}");
            crate::llm::embed_texts(app.clone(), state, vec![qtext], EMBED_MODEL.to_string())
                .await
                .ok()?
                .pop()?
        }
    };
    Some(refs.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max))
}

/// Reference phrasings of health questions - the semantic medical gate scores
/// the turn against these (same mechanism as the freshness gate). Personal
/// framing on purpose: "my results", "this scan" - the gate is for someone
/// discussing THEIR health, not homework about biology.
const MEDICAL_REFERENCES: &[&str] = &[
    "what do my blood test results mean",
    "explain this x-ray image to me",
    "is it safe to take this medication with my other prescriptions",
    "I have been having these symptoms lately",
    "my doctor said I have this condition",
    "what does this diagnosis mean for me",
    "what are the side effects of this medicine",
    "my scan results show something abnormal",
    "is my blood pressure reading normal",
    "should I be worried about this mole on my skin",
    "help me understand my lab report",
    "what treatment options exist for this illness",
    "what should I eat or avoid with my medical condition",
];

/// Benign anchors: the everyday-question REGISTER (asks, tasks, info-seeking).
/// bge-small scores any short question 0.4-0.6 against any other short
/// question, so an absolute floor alone cannot separate "my knee aches after
/// runs" (0.578 vs the medical refs) from "summarize this article for me"
/// (0.594) - measured 2026-07-17, where the floor-only gate fired on 7 of 12
/// benign queries and routed "whats the latest news today" to MedGemma. The
/// gate now requires the turn to be closer to the medical refs than to these
/// by a margin. Not topic-exhaustive on purpose: they anchor the register,
/// not a topic list.
const BENIGN_REFERENCES: &[&str] = &[
    "whats the latest news today",
    "tell me a joke",
    "summarize this article for me",
    "help me write an email to my boss",
    "whats a good recipe for dinner",
    "explain how this technology works",
    "help me plan my day",
    "write some code for me",
    "how is the weather looking",
    "my car is making a strange noise",
    "recommend something to watch",
    "help me with my resume",
];

static MEDICAL_REFS: tokio::sync::OnceCell<Vec<Vec<f32>>> = tokio::sync::OnceCell::const_new();
static BENIGN_REFS: tokio::sync::OnceCell<Vec<Vec<f32>>> = tokio::sync::OnceCell::const_new();

async fn medical_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    MEDICAL_REFS
        .get_or_try_init(|| async {
            let state = app.state::<crate::llm::LLMState>();
            let texts = MEDICAL_REFERENCES.iter().map(|s| s.to_string()).collect();
            crate::llm::embed_texts(app.clone(), state, texts, EMBED_MODEL.to_string()).await
        })
        .await
        .ok()
}

async fn benign_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    BENIGN_REFS
        .get_or_try_init(|| async {
            let state = app.state::<crate::llm::LLMState>();
            let texts = BENIGN_REFERENCES.iter().map(|s| s.to_string()).collect();
            crate::llm::embed_texts(app.clone(), state, texts, EMBED_MODEL.to_string()).await
        })
        .await
        .ok()
}

/// Contrastive gate constants, MEASURED 2026-07-17 against batteries of 26
/// benign + 16 medical queries (session scratch; rerun the batteries when
/// changing refs, thresholds, or the embedding model): floor 0.50 + margin
/// 0.05 = 0/26 benign false fires, 16/16 medical recall. The margin does the
/// separating; the floor only rejects turns unrelated to everything.
const MEDICAL_THRESHOLD: f32 = 0.50;
const MEDICAL_MARGIN: f32 = 0.05;

/// Stage-0 medical gate: unambiguous health terms only. Deliberately excludes
/// words developers use about software ("symptom", "diagnose") - the semantic
/// stage catches those phrasings with context.
fn looks_medical(query: &str) -> bool {
    let q = query.to_lowercase();
    const CUES: &[&str] = &[
        "blood test", "lab result", "x-ray", "xray", " mri", "ct scan",
        "ultrasound", "biopsy", "medication", "prescription", "blood pressure",
        "cholesterol", "glucose", "hba1c", "mammogram", "colonoscopy",
        "pathology", "radiolog", "my doctor", "blood work", "vaccine",
    ];
    CUES.iter().any(|c| q.contains(c))
}

/// Is this turn about the user's health? Keyword stage first (free), then the
/// semantic stage against MEDICAL_REFERENCES using the turn's shared
/// embedding. `false` when embedding is unavailable and no keyword hits.
pub(crate) async fn is_medical_turn(app: &AppHandle, query: &str, query_vec: Option<&[f32]>) -> bool {
    if looks_medical(query) {
        return true;
    }
    let Some(refs) = medical_reference_vecs(app).await else {
        return false;
    };
    let Some(benign) = benign_reference_vecs(app).await else {
        return false;
    };
    let qvec: Vec<f32> = match query_vec {
        Some(v) if !v.is_empty() => v.to_vec(),
        _ => {
            let state = app.state::<crate::llm::LLMState>();
            let qtext = format!("{QUERY_INSTRUCTION}{query}");
            match crate::llm::embed_texts(app.clone(), state, vec![qtext], EMBED_MODEL.to_string()).await {
                Ok(mut v) if !v.is_empty() => v.remove(0),
                _ => return false,
            }
        }
    };
    let med_score = refs.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max);
    let benign_score = benign.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max);
    // Contrastive: medical only when the turn is BOTH related to the medical
    // refs AND meaningfully closer to them than to ordinary-question anchors.
    if med_score >= MEDICAL_THRESHOLD && med_score - benign_score >= MEDICAL_MARGIN {
        log::info!(
            "[router] medical semantic gate hit (med {med_score:.3}, benign {benign_score:.3}, margin {:.3})",
            med_score - benign_score
        );
        return true;
    }
    log::debug!(
        "[router] medical gate pass-through (med {med_score:.3}, benign {benign_score:.3})"
    );
    false
}

/// Stage-0 freshness gate: does the query look like it needs up-to-date info?
/// Cheap, high-precision keyword/temporal cues — deliberately conservative
/// (privacy-first: bias toward staying offline). The bge-small *semantic* gate
/// (Stage 1) catches paraphrases this misses.
fn looks_time_sensitive(query: &str) -> bool {
    let q = query.to_lowercase();
    const CUES: &[&str] = &[
        "today", "tonight", "right now", "currently", "latest", "breaking",
        "this week", "this month", "as of ", "up to date", "up-to-date",
        "weather", "forecast", "stock price", "share price", "exchange rate",
        "who won", "headline", "in the news", "recent news", "current price",
        // Betting-market phrasings (live info by nature). NOT bare "odds of" -
        // that pulls evergreen probability questions ("odds of getting struck
        // by lightning") online.
        "odds on", "betting odds", "favored to win",
        "2025", "2026", "2027",
    ];
    CUES.iter().any(|c| q.contains(c))
}

/// One offline candidate reduced to what the ranking sees — extracted from
/// `pick_offline` so the lean orderings are unit-testable without an app.
#[derive(Clone, Copy, Debug)]
struct OfflineRank {
    cap: u8,      // task capability score
    tier: u8,     // fit: green 2 / yellow 1 / red 0
    params_b: f64,
}

/// The lean-dependent ordering (greater = better; see `pick_offline` docs).
fn offline_ordering(lean: &str, a: OfflineRank, b: OfflineRank) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    match lean {
        "speed" => a
            .tier
            .cmp(&b.tier)
            .then(a.cap.cmp(&b.cap))
            // Smaller wins size ties — fewer parameters = faster tokens.
            .then(b.params_b.partial_cmp(&a.params_b).unwrap_or(Equal)),
        "quality" => a
            .cap
            .cmp(&b.cap)
            .then(a.params_b.partial_cmp(&b.params_b).unwrap_or(Equal))
            .then(a.tier.cmp(&b.tier)),
        _ => a
            .cap
            .cmp(&b.cap)
            .then(a.tier.cmp(&b.tier))
            .then(a.params_b.partial_cmp(&b.params_b).unwrap_or(Equal)),
    }
}

/// Pick the best offline model. `assess` already excludes embedding models
/// (bge etc.) and models that don't fit at all (red) are de-prioritised.
/// Capability is the **task-specific** score (`by_task` — coding for a code
/// query, etc.; `"general"` → overall). `lean` biases the ranking via
/// `offline_ordering`:
/// - `"balanced"` (default): capability, then fit tier (green = fully-on-GPU
///   over yellow = partial offload), then size — the shipped behavior.
/// - `"speed"`: fit tier first, then capability, and SMALLER wins size ties —
///   a fully-on-GPU model over a stronger one that spills to CPU.
/// - `"quality"`: capability, then size, then fit tier — willing to take
///   partial offload for the strongest model that still runs.
/// To avoid reloading on every turn, the currently-loaded model is kept unless
/// a candidate beats it on THIS task by at least `SWITCH_MARGIN` (so a real
/// specialist for a real task is worth the reload, but near-equal models
/// don't thrash).
async fn pick_offline(app: &AppHandle, task: &str, lean: &str, agent_only: bool) -> Result<String, String> {
    let mut all = crate::fit::assess(app).await;
    // Agent sessions: tool-driving is a hard filter, not a preference.
    if agent_only {
        all.retain(|f| crate::model_caps::agent_caps(&f.name) >= 6);
    }
    if all.is_empty() {
        return Err("No offline models downloaded".to_string());
    }

    // fit tier: green(2) > yellow(1) > red(0).
    let tier = |f: &crate::fit::ModelFit| match f.fit {
        crate::fit::Fit::Green => 2,
        crate::fit::Fit::Yellow => 1,
        crate::fit::Fit::Red => 0,
    };
    let cap = |name: &str| crate::model_caps::caps_for(name).by_task(task);

    // Prefer models that actually run (green/yellow); fall back to red only if
    // every model is red (better to try than to refuse). Models that already
    // OOM'd the GPU this session are excluded outright — the loader rejects
    // them instantly, so picking one would turn every auto request into an
    // error even though the pre-load fit estimate still grades them runnable.
    let usable = |f: &&crate::fit::ModelFit| !crate::llm::is_model_too_big(f.name.clone());
    let runnable: Vec<&crate::fit::ModelFit> =
        all.iter().filter(|f| tier(f) > 0).filter(usable).collect();
    let pool: Vec<&crate::fit::ModelFit> = if runnable.is_empty() {
        let any_usable: Vec<&crate::fit::ModelFit> = all.iter().filter(usable).collect();
        if any_usable.is_empty() {
            all.iter().collect() // everything proven too big — let the loader say so
        } else {
            any_usable
        }
    } else {
        runnable
    };

    // Specialists never compete for general questions - their way into Auto
    // is their task (the health gate sets task="medical"). Excluding them
    // here also RELEASES a loaded specialist: the keep-current rule below
    // only keeps models still in the pool, so the next general turn switches
    // back to a generalist instead of sticking with e.g. MedGemma forever.
    // If only specialists are installed, keep them - answering beats refusing.
    let pool: Vec<&crate::fit::ModelFit> = if task == "medical" {
        pool
    } else {
        let generalists: Vec<&crate::fit::ModelFit> = pool
            .iter()
            .copied()
            .filter(|f| !crate::model_caps::is_specialist(&f.name))
            .collect();
        if generalists.is_empty() { pool } else { generalists }
    };

    let rank = |f: &crate::fit::ModelFit| OfflineRank {
        cap: cap(&f.name),
        tier: tier(f),
        params_b: f.params_b,
    };
    let best = *pool
        .iter()
        .max_by(|a, b| offline_ordering(lean, rank(a), rank(b)))
        .unwrap(); // pool is non-empty

    // Keep the loaded model unless a candidate beats it on THIS task by at least
    // SWITCH_MARGIN — a reload is worth it for a real specialist, not a marginal gain.
    let current = app
        .state::<crate::llm::LLMState>()
        .current_model
        .lock()
        .await
        .clone();
    if let Some(cur) = current.filter(|c| !c.starts_with("online:")) {
        if let Some(cf) = pool.iter().find(|f| f.name == cur) {
            if cap(&cf.name) + SWITCH_MARGIN >= cap(&best.name) {
                return Ok(cur.clone());
            }
        }
    }
    Ok(best.name.clone())
}

/// The user's per-slot online model choices from Settings. `None` = use the
/// recommended default. Values are `online:<id>` strings exactly as returned
/// by `list_online_models`.
#[derive(Default)]
pub struct OnlinePicks {
    pub fresh: Option<String>,
    pub hard_code: Option<String>,
    pub hard_general: Option<String>,
    /// Agent (folder) turns' online model - the tool-driver slot.
    pub agent: Option<String>,
    /// Planning/helper subagents' online model (reasoning-lean tool-driver).
    pub plan: Option<String>,
}

/// Recommended defaults per routing slot. Ids match the proxy catalog; if one
/// is missing (the catalog moved on) selection falls back to the capability
/// registry, so routing never breaks. The Settings page names these same
/// models as "Recommended" - keep the two in sync.
const DEFAULT_FRESH: &str = "online:grok-4.5-search";
const DEFAULT_HARD_CODE: &str = "online:gpt-5.6-sol";
const DEFAULT_HARD_GENERAL: &str = "online:gpt-5.6-terra";
/// The agent slot: must be a PROVEN tool-driver through the proxy (kimi -
/// verified end to end; Sol drops tools until Responses passthrough).
const DEFAULT_AGENT: &str = "online:kimi-k2.6";
/// Planning leans reasoning; must still drive tools (planners read files).
const DEFAULT_PLAN: &str = "online:gpt-5.6-terra";

/// Pick an online model for one routing decision. Order: the user's explicit
/// choice for this slot (when still in the catalog) → the recommended default
/// → a registry fallback. For a FRESH query the fallback must see live web, so
/// it prefers a **web-search-capable** model (Grok-with-search → Sonar → any
/// per-search-fee → anything). For a difficulty escalation the fallback is the
/// best online model FOR THE TASK via the online capability registry.
fn select_online(
    models: &[crate::flowsta::OnlineModel],
    task: &str,
    fresh: bool,
    pref: Option<&str>,
) -> Option<String> {
    let by_id = |id: &str| models.iter().find(|m| m.id == id).map(|m| m.id.clone());
    if let Some(hit) = pref.and_then(|p| by_id(p)) {
        return Some(hit);
    }
    let text = |m: &crate::flowsta::OnlineModel| {
        format!("{} {} {}", m.id, m.display_name, m.description).to_lowercase()
    };

    if fresh {
        if let Some(hit) = by_id(DEFAULT_FRESH) {
            return Some(hit);
        }
        let has_search_fee = |m: &crate::flowsta::OnlineModel| {
            m.pricing.as_ref().and_then(|p| p.search_per_call_usd).is_some()
        };
        return models
            .iter()
            .find(|m| {
                let t = text(m);
                t.contains("grok") && (t.contains("search") || has_search_fee(m))
            })
            .or_else(|| {
                models
                    .iter()
                    .find(|m| { let t = text(m); t.contains("sonar") || t.contains("perplexity") })
            })
            .or_else(|| models.iter().find(|m| has_search_fee(m)))
            .or_else(|| models.first())
            .map(|m| m.id.clone());
    }

    let slot_default = if matches!(task, "code" | "math" | "reasoning") {
        DEFAULT_HARD_CODE
    } else {
        DEFAULT_HARD_GENERAL
    };
    if let Some(hit) = by_id(slot_default) {
        return Some(hit);
    }
    models
        .iter()
        .max_by_key(|m| crate::model_caps::online_caps_for(&text(m)).by_task(task))
        .map(|m| m.id.clone())
}

/// Online model for an agent session: the user's slot pick -> the default
/// tool-driver -> the best agent-capable model by the online registry. A
/// tools-blind model is never returned - a broken session is worse than none.
fn select_online_agent(
    models: &[crate::flowsta::OnlineModel],
    pref: Option<&str>,
) -> Option<String> {
    let by_id = |id: &str| models.iter().find(|m| m.id == id).map(|m| m.id.clone());
    if let Some(hit) = pref.and_then(|p| by_id(p)) {
        return Some(hit);
    }
    if let Some(hit) = by_id(DEFAULT_AGENT) {
        return Some(hit);
    }
    let text = |m: &crate::flowsta::OnlineModel| {
        format!("{} {} {}", m.id, m.display_name, m.description).to_lowercase()
    };
    models
        .iter()
        .filter(|m| crate::model_caps::online_agent_caps(&text(m)) >= 6)
        .max_by_key(|m| crate::model_caps::online_agent_caps(&text(m)))
        .map(|m| m.id.clone())
}

/// A slot preference from the Rust-readable settings store (mirrored there
/// by the Settings page so non-webview callers see user choices).
fn store_pref(app: &AppHandle, key: &str) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").ok()?;
    store
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
}

async fn pick_online(task: &str, fresh: bool, pref: Option<&str>) -> Option<String> {
    let models = crate::flowsta::list_online_models().await.ok()?;
    select_online(&models, task, fresh, pref)
}

/// Should the connected external server take this query instead of the
/// local pick? Pure decision (unit-tested): the external model wins only
/// when its capability is RECOGNIZED and beats the local best by
/// `SWITCH_MARGIN` (the beefier-box case), or when nothing local runs at
/// all (rescue). With capabilities within the margin, `lean == "speed"`
/// defers to the external only when its measured speed is known and beats
/// the local estimate. Unknown external families never win on confidence.
fn external_beats_local(
    lean: &str,
    local_best_cap: Option<u8>,
    external_cap: Option<u8>,
    external_tps: Option<f64>,
    local_tps_estimate: Option<f64>,
) -> bool {
    let Some(ext) = external_cap else { return local_best_cap.is_none() };
    match local_best_cap {
        None => true, // nothing local runs — any recognized external is a rescue
        Some(local) => {
            if ext >= local.saturating_add(SWITCH_MARGIN) {
                return true;
            }
            if lean == "speed" && ext + SWITCH_MARGIN > local {
                if let (Some(et), Some(lt)) = (external_tps, local_tps_estimate) {
                    return et > lt;
                }
            }
            false
        }
    }
}

/// Best external candidate for a task from the cached scan: highest
/// RECOGNIZED capability (unknown families are skipped — they can still be
/// picked explicitly, never confidently by the router).
fn best_external(models: &[String], task: &str) -> Option<(String, u8)> {
    models
        .iter()
        .filter_map(|id| {
            crate::model_caps::known_caps(id).map(|c| (id.clone(), c.by_task(task)))
        })
        .max_by_key(|(_, cap)| *cap)
}

/// Resolve an Auto mode to a concrete model for this query.
/// `mode` = `"offline"` | `"online-offline"` | `"my-hardware"` (this device
/// plus the user's connected server — never the online proxy). `eagerness`
/// tunes the online FRESHNESS threshold only: `"privacy"` | `"balanced"` |
/// `"freshness"`.
/// `lean` biases the offline pick (`"speed"` | `"balanced"` | `"quality"`).
/// Difficulty escalation is inherent to online-offline mode: choosing the
/// mode IS the consent to go online when it helps, so a hard query always
/// may use a stronger online model. `picks` carries the user's per-slot
/// online model overrides (Settings → Routing).
/// The live routing ledger: the last decisions with reasons, for the
/// Settings transparency view. The DURABLE audit is per answer in the
/// Holochain transcript (routing_reason/routing_task in provenance) - this
/// is just a fast window onto recent activity.
#[derive(serde::Serialize, Clone)]
pub struct RoutingDecision {
    pub at_ms: i64,
    pub model: String,
    pub reason: String,
}

static RECENT_DECISIONS: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<RoutingDecision>>> =
    std::sync::OnceLock::new();

fn remember_decision(model: &str, reason: &str) {
    let ledger = RECENT_DECISIONS.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()));
    if let Ok(mut d) = ledger.lock() {
        d.push_front(RoutingDecision {
            at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_millis() as i64)
                .unwrap_or(0),
            model: model.to_string(),
            reason: reason.to_string(),
        });
        d.truncate(20);
    }
}

#[tauri::command]
pub fn recent_routing_decisions() -> Vec<RoutingDecision> {
    RECENT_DECISIONS
        .get()
        .and_then(|l| l.lock().ok().map(|d| d.iter().cloned().collect()))
        .unwrap_or_default()
}

/// Session-scoped online-agent override: set when the user accepts the
/// overload offer card ("<model>'s provider is overloaded - use <alt> for
/// this session?"). It wins over the Agent slot for non-planning agent
/// turns, so the user's explicit pick holds for the rest of the workspace
/// session - an accepted card, never a silent switch. Cleared when the
/// workspace closes (and naturally on app restart).
static AGENT_ONLINE_OVERRIDE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();

fn agent_online_override() -> Option<String> {
    AGENT_ONLINE_OVERRIDE
        .get()
        .and_then(|l| l.lock().ok().and_then(|v| v.clone()))
}

pub fn clear_agent_online_override() {
    set_agent_online_override(None);
}

#[tauri::command]
pub fn set_agent_online_override(model: Option<String>) {
    let cell = AGENT_ONLINE_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut v) = cell.lock() {
        *v = model.filter(|m| !m.is_empty());
    }
}

#[derive(serde::Serialize)]
pub struct AgentAlternate {
    /// What agent routing picks right now (the model that just failed).
    pub failed: String,
    pub failed_name: String,
    /// The best agent-capable model that isn't the failing one.
    pub alt: String,
    pub alt_name: String,
}

/// The overload offer's substance: what agent routing WOULD pick right now,
/// and the next agent-capable online model to offer instead. None when the
/// catalog has no capable alternative - then there is nothing to offer.
#[tauri::command]
pub async fn alternate_online_agent(app: AppHandle) -> Option<AgentAlternate> {
    let models = crate::flowsta::list_online_models().await.ok()?;
    let pref = agent_online_override().or_else(|| store_pref(&app, "routingOnlineAgent"));
    let failed = select_online_agent(&models, pref.as_deref())?;
    let rest: Vec<crate::flowsta::OnlineModel> =
        models.iter().filter(|m| m.id != failed).cloned().collect();
    let alt = select_online_agent(&rest, None)?;
    let name = |id: &str| {
        models
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.display_name.clone())
            .unwrap_or_else(|| id.trim_start_matches("online:").to_string())
    };
    Some(AgentAlternate {
        failed_name: name(&failed),
        alt_name: name(&alt),
        failed,
        alt,
    })
}

pub async fn route(
    app: &AppHandle,
    mode: &str,
    query: &str,
    eagerness: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    query_vec: Option<&[f32]>,
    agent: bool,
    plan: bool,
) -> Result<RouteResult, String> {
    let result = route_inner(app, mode, query, eagerness, task, difficulty, lean, picks, query_vec, agent, plan).await;
    if let Ok(r) = &result {
        remember_decision(&r.model, &r.reason);
    }
    result
}

async fn route_inner(
    app: &AppHandle,
    mode: &str,
    query: &str,
    eagerness: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    query_vec: Option<&[f32]>,
    agent: bool,
    plan: bool,
) -> Result<RouteResult, String> {
    // Slot preferences: explicit params (the in-app chat path reads
    // localStorage) fall back to the Rust-readable settings store, so API
    // and agent callers see the user's choices too - Settings mirrors every
    // slot pick into settings.json.
    let picks = OnlinePicks {
        fresh: picks.fresh.clone().or_else(|| store_pref(app, "routingOnlineFresh")),
        hard_code: picks
            .hard_code
            .clone()
            .or_else(|| store_pref(app, "routingOnlineHardCode")),
        hard_general: picks
            .hard_general
            .clone()
            .or_else(|| store_pref(app, "routingOnlineHardGeneral")),
        agent: picks.agent.clone().or_else(|| store_pref(app, "routingOnlineAgent")),
        plan: picks.plan.clone().or_else(|| store_pref(app, "routingOnlinePlanning")),
    };
    let picks = &picks;
    // Agent (folder) turns: deterministic and session-stable. The per-query
    // gates (freshness, difficulty, medical) don't apply to tool calls, so
    // the same installed models + settings give the same pick every call -
    // no mid-session switches, KV caches stay warm. Tool capability is a
    // HARD FILTER. Online-offline mode routes agent work ONLINE by default
    // (the mode is the consent; agent work is where a stronger model earns
    // its keep); privacy eagerness prefers a capable local model.
    if agent {
        // The user's preference order for the online side: the Agent slot,
        // else their hard-code TYPE preference IF it can drive tools (when
        // Sol gains passthrough, a Sol hard-code preference starts applying
        // here automatically - nothing is built for one vendor), else the
        // default tool-driver, else the registry's best capable model.
        let type_pref = picks
            .hard_code
            .clone()
            .filter(|id| crate::model_caps::online_agent_caps(id) >= 6);
        let agent_pref = if plan {
            // Planning subagents: the Planning slot, else the Agent slot's
            // chain - a planner is still a tool-driver, just reasoning-lean.
            picks
                .plan
                .clone()
                .or_else(|| Some(DEFAULT_PLAN.to_string()))
        } else {
            // An accepted overload offer holds for the whole workspace
            // session - the user's explicit pick outranks the slots.
            agent_online_override().or_else(|| picks.agent.clone().or(type_pref))
        };
        let offline_task = if plan { "reasoning" } else { "code" };
        let offline = pick_offline(app, offline_task, lean, true).await.ok();
        if mode != "online-offline" {
            return offline
                .map(|m| RouteResult {
                    model: m,
                    reason: "agent work on your device".to_string(),
                })
                .ok_or_else(|| {
                    "No installed model can drive agent work - download an agentic model (Qwen 3.5, GLM, a coder) or use an online-capable AI".to_string()
                });
        }
        if eagerness == "privacy" {
            if let Some(m) = offline.clone() {
                return Ok(RouteResult {
                    model: m,
                    reason: "agent work kept on your device (privacy-first)".to_string(),
                });
            }
        }
        if let Ok(models) = crate::flowsta::list_online_models().await {
            if let Some(id) = select_online_agent(&models, agent_pref.as_deref()) {
                // The ledger and on-chain provenance must say WHY this model:
                // an accepted overload offer is the user's call, not routing's.
                let reason = if !plan && agent_online_override().as_deref() == Some(id.as_str()) {
                    "agent work on your session pick (accepted after an overloaded model)"
                } else {
                    "agent work on a stronger online model"
                };
                return Ok(RouteResult {
                    model: id,
                    reason: reason.to_string(),
                });
            }
        }
        return offline
            .map(|m| RouteResult {
                model: m,
                reason: "agent work on your device (no online tool-driver available)".to_string(),
            })
            .ok_or_else(|| {
                "No model available for agent work - download an agentic model or sign in for online models".to_string()
            });
    }
    // Health questions stay home. A turn about the user's own health never
    // auto-routes online (no freshness route, no hard-question escalation),
    // and the task becomes "medical" so an installed medical specialist
    // (MedGemma) takes it. Pinned models are untouched - pinning is explicit
    // consent - and "Try this answer online" on the receipt remains an
    // explicit user action. Auto - My hardware may still use the user's OWN
    // server: it's their hardware.
    let medical = is_medical_turn(app, query, query_vec).await;
    let task: &str = if medical { "medical" } else { task };

    if mode == "online-offline" && !medical {
        // Stage 0: cheap keyword cues. Stage 1 (only if Stage 0 is negative):
        // bge-small semantic similarity to "needs-current-info" phrases. Stage 2:
        // difficulty escalation — a HARD query goes to a stronger online model
        // (inherent to this mode; auto:offline is the never-online choice).
        // `is_fresh` = the query needs LIVE WEB (→ a search model), vs a
        // difficulty escalation (→ the best online model for the task).
        let (why, is_fresh) = if looks_time_sensitive(query) {
            (Some("looks like it needs current info"), true)
        } else if semantic_fresh_score(app, query, query_vec)
            .await
            .is_some_and(|s| s >= threshold_for(eagerness))
        {
            (Some("seems to need up-to-date info"), true)
        } else if difficulty == "hard" {
            (Some("a hard question — using a stronger model"), false)
        } else {
            (None, false)
        };
        if let Some(why) = why {
            let pref = if is_fresh {
                picks.fresh.as_deref()
            } else if matches!(task, "code" | "math" | "reasoning") {
                picks.hard_code.as_deref()
            } else {
                picks.hard_general.as_deref()
            };
            if let Some(model) = pick_online(task, is_fresh, pref).await {
                return Ok(RouteResult {
                    model,
                    reason: format!("online — {why}"),
                });
            }
            // No online model available → fall through to offline.
        }
    }
    // "My hardware": the local pick may hand off to the user's connected
    // server when the scan shows a clearly stronger (or, on the speed lean,
    // faster) RECOGNIZED model there. Never the online proxy in this mode;
    // an unreachable server falls through to local at request time.
    if mode == "my-hardware" {
        let (ext_models, ext_tps) = crate::engine::external_models_cached(app);
        if !ext_models.is_empty() {
            let local = pick_offline(app, task, lean, false).await.ok();
            let local_cap = local
                .as_deref()
                .map(|m| crate::model_caps::caps_for(m).by_task(task));
            // Only hand a chat to the server when it answers RIGHT NOW —
            // otherwise fall through to the local pick, same graceful shape
            // as the online fallthrough.
            let want_external = best_external(&ext_models, task)
                .filter(|(_, cap)| {
                    external_beats_local(lean, local_cap, Some(*cap), ext_tps, None)
                })
                .map(|(id, _)| id)
                .or_else(|| {
                    if local.is_none() {
                        ext_models.first().cloned() // rescue: nothing local runs
                    } else {
                        None
                    }
                });
            if let Some(ext_id) = want_external {
                if crate::engine::external_reachable(app).await {
                    let why = if local.is_some() {
                        "your server — stronger for this task"
                    } else {
                        "your server — no local model fits"
                    };
                    return Ok(RouteResult {
                        model: format!("external:{}", ext_id),
                        reason: why.to_string(),
                    });
                }
                log::warn!("[Router] external engine unreachable — using the local pick");
            }
            if let Some(local) = local {
                return Ok(RouteResult { model: local, reason: "offline".to_string() });
            }
        }
        // No server connected (or nothing usable) — behave like offline-only.
    }

    let model = pick_offline(app, task, lean, false).await?;
    let reason = if medical {
        // The visible promise: this is the receipt line users see. Name the
        // specialist when one took the question.
        if model.to_lowercase().contains("medgemma") {
            "a health question — kept on your device, answered by your medical model"
        } else {
            "a health question — kept on your device"
        }
    } else if mode == "online-offline" {
        "offline — no fresh info needed"
    } else {
        "offline"
    };
    Ok(RouteResult {
        model,
        reason: reason.to_string(),
    })
}

/// Which routing tasks would actually change the offline pick — i.e. some runnable
/// model beats the overall-best model on that task by `SWITCH_MARGIN`. **Empty =
/// no specialist installed → the caller should SKIP task classification** (there's
/// nothing to route to, so don't spend a classifier call). Cheap (no model loads):
/// it's the gate that keeps task routing free for users without specialists.
#[tauri::command]
pub async fn routing_specialist_tasks(app: AppHandle) -> Result<Vec<String>, String> {
    let pool: Vec<crate::fit::ModelFit> = crate::fit::assess(&app)
        .await
        .into_iter()
        .filter(|f| !matches!(f.fit, crate::fit::Fit::Red)) // can't run → can't route to
        .collect();
    if pool.len() < 2 {
        return Ok(vec![]); // 0/1 runnable model → nothing to route between
    }
    let overall_best = pool
        .iter()
        .max_by_key(|f| crate::model_caps::caps_for(&f.name).overall)
        .unwrap();
    let ob = crate::model_caps::caps_for(&overall_best.name);
    let mut out = Vec::new();
    for task in ["code", "math", "reasoning"] {
        let task_best = pool
            .iter()
            .max_by_key(|f| crate::model_caps::caps_for(&f.name).by_task(task))
            .unwrap();
        let tb = crate::model_caps::caps_for(&task_best.name).by_task(task);
        if task_best.name != overall_best.name && tb >= ob.by_task(task) + SWITCH_MARGIN {
            out.push(task.to_string());
        }
    }
    Ok(out)
}

/// Resolve an Auto mode for the in-app chat. The frontend calls this when an
/// AI's model is `auto:offline` / `auto:online-offline`.
/// Is this turn about the user's own health? Exposed so the frontend's vision
/// path can keep a health IMAGE local when no offline vision model is
/// downloaded (rather than offering a cloud model).
#[tauri::command]
pub async fn is_medical_query(
    app: AppHandle,
    query: String,
    query_vec: Option<Vec<f32>>,
) -> Result<bool, String> {
    Ok(is_medical_turn(&app, &query, query_vec.as_deref()).await)
}

#[tauri::command]
pub async fn route_model(
    app: AppHandle,
    mode: String,
    query: String,
    eagerness: Option<String>,
    task: Option<String>,
    difficulty: Option<String>,
    lean: Option<String>,
    online_fresh: Option<String>,
    online_hard_code: Option<String>,
    online_hard_general: Option<String>,
    online_agent: Option<String>,
    online_planning: Option<String>,
    query_vec: Option<Vec<f32>>,
    agent: Option<bool>,
    plan: Option<bool>,
) -> Result<RouteResult, String> {
    let picks = OnlinePicks {
        fresh: online_fresh,
        hard_code: online_hard_code,
        hard_general: online_hard_general,
        agent: online_agent,
        plan: online_planning,
    };
    route(
        &app,
        &mode,
        &query,
        eagerness.as_deref().unwrap_or("balanced"),
        task.as_deref().unwrap_or("general"),
        difficulty.as_deref().unwrap_or("easy"),
        lean.as_deref().unwrap_or("balanced"),
        &picks,
        query_vec.as_deref(),
        agent.unwrap_or(false),
        plan.unwrap_or(false),
    )
    .await
}

#[cfg(test)]
mod tests {

    fn om(id: &str, name: &str, desc: &str, search_fee: Option<f64>) -> crate::flowsta::OnlineModel {
        crate::flowsta::OnlineModel {
            id: format!("online:{id}"),
            display_name: name.to_string(),
            description: desc.to_string(),
            context_window: 0,
            vision: false,
            category: "chat".to_string(),
            pricing: Some(crate::flowsta::OnlinePricing {
                input_per_mtok: 1.0,
                output_per_mtok: 1.0,
                request_fee_usd: 0.0,
                search_per_call_usd: search_fee,
            }),
        }
    }

    fn catalog() -> Vec<crate::flowsta::OnlineModel> {
        vec![
            om("grok-4.5", "Grok 4.5", "xAI's newest", None),
            om("grok-4.5-search", "Grok 4.5 (Web)", "live web search", Some(0.005)),
            om("gpt-5.6-sol", "GPT-5.6 Sol", "OpenAI's flagship", None),
            om("gpt-5.6-terra", "GPT-5.6 Terra", "balanced flagship", None),
            om("sonar", "Sonar", "Perplexity search", Some(0.005)),
        ]
    }

    #[test]
    fn medical_keyword_gate_hits_health_not_software() {
        // Health phrasings hit...
        for q in [
            "can you explain my blood test results",
            "what does this X-ray show",
            "is this medication ok with ibuprofen",
            "my doctor mentioned high cholesterol",
        ] {
            assert!(looks_medical(q), "should be medical: {q}");
        }
        // ...developer-speak does not (the semantic stage owns ambiguity).
        for q in [
            "diagnose this bug in my parser",
            "the symptom is a crash on startup",
            "what's the latest news today",
            "write me a poem about the ocean",
        ] {
            assert!(!looks_medical(q), "should NOT be medical: {q}");
        }
    }

    #[test]
    fn medical_task_prefers_medgemma_by_switch_margin() {
        let med = crate::model_caps::caps_for("medgemma-1.5-4b-it-Q4_K_M.gguf");
        let gen = crate::model_caps::caps_for("gemma-4-E4B-it-Q4_K_M.gguf");
        // The specialist must clear the stickiness margin so a loaded general
        // model actually swaps for a medical question...
        assert!(med.by_task("medical") >= gen.by_task("medical") + 2);
        // ...while staying modest enough that general chat never prefers it.
        assert!(gen.by_task("general") > med.by_task("general"));
    }

    #[test]
    fn select_online_defaults_per_slot() {
        let models = catalog();
        // Fresh → the web-search default.
        assert_eq!(select_online(&models, "general", true, None).unwrap(), "online:grok-4.5-search");
        // Hard code / math / reasoning → the flagship.
        for task in ["code", "math", "reasoning"] {
            assert_eq!(select_online(&models, task, false, None).unwrap(), "online:gpt-5.6-sol");
        }
        // Hard general → the balanced tier.
        assert_eq!(select_online(&models, "general", false, None).unwrap(), "online:gpt-5.6-terra");
    }

    #[test]
    fn select_online_user_pref_wins_when_in_catalog() {
        let models = catalog();
        // A user who prefers Sonar for hard questions gets Sonar.
        assert_eq!(
            select_online(&models, "code", false, Some("online:sonar")).unwrap(),
            "online:sonar"
        );
        // A pref no longer in the catalog is ignored → default applies.
        assert_eq!(
            select_online(&models, "code", false, Some("online:retired-model")).unwrap(),
            "online:gpt-5.6-sol"
        );
    }

    #[test]
    fn select_online_falls_back_when_defaults_absent() {
        // A future catalog without today's default ids must still route sanely.
        let models = vec![
            om("sonar", "Sonar", "Perplexity search", Some(0.005)),
            om("newgpt-9", "NewGPT 9", "a gpt model", None),
        ];
        // Fresh → the search-capable model.
        assert_eq!(select_online(&models, "general", true, None).unwrap(), "online:sonar");
        // Hard reasoning → registry ranks the gpt above the search-first model.
        assert_eq!(select_online(&models, "reasoning", false, None).unwrap(), "online:newgpt-9");
        // Empty catalog → None (router falls through to offline).
        assert_eq!(select_online(&[], "code", false, None), None);
    }

    fn agent_catalog() -> Vec<crate::flowsta::OnlineModel> {
        vec![
            om("kimi-k2.6", "Kimi K2.6", "tool-driving flagship", None),
            om("gpt-5.6-sol", "GPT-5.6 Sol", "OpenAI's flagship", None),
            om("sonar", "Sonar", "Perplexity search", Some(0.005)),
        ]
    }

    #[test]
    fn select_agent_default_is_the_tool_driver() {
        assert_eq!(
            select_online_agent(&agent_catalog(), None).unwrap(),
            "online:kimi-k2.6"
        );
    }

    #[test]
    fn select_agent_pref_wins_including_sol() {
        // Sol is agent-eligible now that the proxy passes tools through -
        // an existing Sol preference applies with no other changes.
        assert_eq!(
            select_online_agent(&agent_catalog(), Some("online:gpt-5.6-sol")).unwrap(),
            "online:gpt-5.6-sol"
        );
        // A pref no longer in the catalog is ignored → default applies.
        assert_eq!(
            select_online_agent(&agent_catalog(), Some("online:retired")).unwrap(),
            "online:kimi-k2.6"
        );
    }

    #[test]
    fn agent_override_set_get_clear() {
        // The overload offer's session pick: set wins, empty = clear, and
        // select_online_agent honors it only while it exists in the catalog.
        set_agent_online_override(Some("online:gpt-5.6-sol".into()));
        assert_eq!(agent_online_override().as_deref(), Some("online:gpt-5.6-sol"));
        assert_eq!(
            select_online_agent(&agent_catalog(), agent_online_override().as_deref()).unwrap(),
            "online:gpt-5.6-sol"
        );
        set_agent_online_override(Some(String::new()));
        assert_eq!(agent_online_override(), None);
        set_agent_online_override(Some("online:gpt-5.6-sol".into()));
        clear_agent_online_override();
        assert_eq!(agent_online_override(), None);
    }

    #[test]
    fn select_agent_registry_fallback_skips_tools_blind_models() {
        // No default in the catalog → best agent-capable model wins...
        let models = vec![
            om("sonar", "Sonar", "Perplexity search", Some(0.005)),
            om("gpt-5.6-sol", "GPT-5.6 Sol", "OpenAI's flagship", None),
        ];
        assert_eq!(
            select_online_agent(&models, None).unwrap(),
            "online:gpt-5.6-sol"
        );
        // ...and a search-only catalog yields None rather than a broken
        // session (sonar is below the tool-capability floor).
        let searchers = vec![om("sonar", "Sonar", "Perplexity search", Some(0.005))];
        assert_eq!(select_online_agent(&searchers, None), None);
    }
    use super::*;
    use std::cmp::Ordering;

    fn c(cap: u8, tier: u8, params_b: f64) -> OfflineRank {
        OfflineRank { cap, tier, params_b }
    }

    /// The best of two candidates under a lean (mirrors max_by semantics).
    fn best(lean: &str, a: OfflineRank, b: OfflineRank) -> OfflineRank {
        if offline_ordering(lean, a, b) == Ordering::Less { b } else { a }
    }

    #[test]
    fn speed_prefers_fully_on_gpu_over_stronger_offload() {
        let green_small = c(5, 2, 8.0); // fits fully, decent
        let yellow_big = c(7, 1, 24.0); // stronger, spills to CPU
        assert_eq!(best("speed", green_small, yellow_big).params_b, 8.0);
        // balanced and quality both take the stronger model
        assert_eq!(best("balanced", green_small, yellow_big).params_b, 24.0);
        assert_eq!(best("quality", green_small, yellow_big).params_b, 24.0);
    }

    #[test]
    fn speed_smaller_wins_ties() {
        let small = c(5, 2, 4.0);
        let big = c(5, 2, 8.0);
        assert_eq!(best("speed", small, big).params_b, 4.0);
        // the other leans keep the bigger model on a full tie
        assert_eq!(best("balanced", small, big).params_b, 8.0);
        assert_eq!(best("quality", small, big).params_b, 8.0);
    }

    #[test]
    fn speed_capability_still_matters_within_a_tier() {
        let weak_small = c(3, 2, 4.0);
        let strong_big = c(7, 2, 24.0);
        assert_eq!(best("speed", weak_small, strong_big).cap, 7);
    }

    #[test]
    fn quality_takes_offload_for_the_bigger_equal_cap_model() {
        let green_small = c(8, 2, 8.0);
        let yellow_big = c(8, 1, 24.0); // same capability score, bigger
        assert_eq!(best("quality", green_small, yellow_big).params_b, 24.0);
        // balanced breaks the cap tie on fit tier instead
        assert_eq!(best("balanced", green_small, yellow_big).params_b, 8.0);
    }

    #[test]
    fn capability_dominates_for_balanced_and_quality() {
        let strong_red_risk = c(9, 1, 30.0);
        let weak_green = c(4, 2, 3.0);
        assert_eq!(best("balanced", strong_red_risk, weak_green).cap, 9);
        assert_eq!(best("quality", strong_red_risk, weak_green).cap, 9);
    }

    #[test]
    fn unknown_lean_falls_back_to_balanced_order() {
        let a = c(5, 2, 8.0);
        let b = c(7, 1, 24.0);
        assert_eq!(
            offline_ordering("nonsense", a, b),
            offline_ordering("balanced", a, b)
        );
    }

    #[test]
    fn unknown_external_never_beats_a_running_local() {
        assert!(!external_beats_local("balanced", Some(5), None, Some(50.0), None));
    }

    #[test]
    fn any_external_rescues_when_nothing_local_runs() {
        assert!(external_beats_local("balanced", None, None, None, None));
        assert!(external_beats_local("balanced", None, Some(3), None, None));
    }

    #[test]
    fn recognized_external_needs_the_switch_margin() {
        // local 5, margin 2: external 7 wins, external 6 does not.
        assert!(external_beats_local("balanced", Some(5), Some(7), None, None));
        assert!(!external_beats_local("balanced", Some(5), Some(6), None, None));
    }

    #[test]
    fn speed_lean_defers_to_measured_speed_within_margin() {
        // caps within margin; external only wins with a KNOWN faster speed.
        assert!(external_beats_local("speed", Some(5), Some(5), Some(50.0), Some(20.0)));
        assert!(!external_beats_local("speed", Some(5), Some(5), Some(10.0), Some(20.0)));
        assert!(!external_beats_local("speed", Some(5), Some(5), None, Some(20.0)));
        // other leans never speed-arbitrate
        assert!(!external_beats_local("balanced", Some(5), Some(5), Some(50.0), Some(20.0)));
    }

    #[test]
    fn best_external_skips_unknown_families() {
        let models = vec![
            "totally-unknown-model.bin".to_string(),
            "Phi-4-mini-instruct-Q4_K_M.gguf".to_string(),
            "Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf".to_string(),
        ];
        let (id, cap) = best_external(&models, "code").unwrap();
        assert_eq!(id, "Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf");
        assert_eq!(cap, 9);
        assert!(best_external(&["mystery.bin".to_string()], "general").is_none());
    }
}
