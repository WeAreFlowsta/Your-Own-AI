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
    /// Reasoning budget as a routing output: Some(true) = think (a hard
    /// question on a model whose reasoning can be switched), Some(false) =
    /// answer directly (known easy), None = no verdict (unknown difficulty,
    /// a model with no switch, an agent turn) - the caller keeps its own rule.
    pub think: Option<bool>,
}

/// The previous completed turn, for follow-up stickiness: which side
/// answered, its routing task, the model, and the previous user turn's
/// embedding (for topic-change decay). All optional; absent = no history.
#[derive(Default, Clone)]
pub struct PrevTurn {
    pub side: Option<String>,
    pub task: Option<String>,
    pub model: Option<String>,
    pub vec: Option<Vec<f32>>,
}

/// A short, anaphoric follow-up ("more on that", "why?", "and the second
/// one?", "shorter please") that carries no signal of its own. Such a turn
/// inherits the previous turn's side and task instead of being re-routed
/// from scratch - a live-news thread stays on the search model, a coding
/// thread stays on the coder. Pure; unit-tested.
pub(crate) fn is_followup(query: &str) -> bool {
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() || words.len() > 9 {
        return false;
    }
    const CUES: &[&str] = &[
        "more", "why", "how so", "and ", "what about", "that", "it ", "it?", "this",
        "shorter", "longer", "again", "expand", "elaborate", "explain that", "the second",
        "the first", "the other", "those", "them", "same", "also", "go on", "continue",
        "example", "simpler", "in detail", "sure", "yes", "ok", "please",
    ];
    let padded = format!(" {q} ");
    CUES.iter().any(|c| padded.contains(&format!(" {}", c.trim_end())) || q.starts_with(c.trim()))
}

/// Topic-change decay: the follow-up must still sit near the previous user
/// turn (cosine of the two query embeddings). Without a previous vector the
/// wording alone decides.
const FOLLOWUP_TOPIC_MIN: f32 = 0.45;

/// The think verdict for a routing decision. Pure; unit-tested.
fn think_for(difficulty: &str, capable: bool, agent: bool) -> Option<bool> {
    if agent || !capable {
        return None;
    }
    match difficulty {
        "hard" => Some(true),
        "easy" => Some(false),
        _ => None,
    }
}

pub(crate) const QUERY_INSTRUCTION: &str = "Represent this sentence for searching relevant passages: ";
pub(crate) const EMBED_MODEL: &str = "bge-small-en-v1.5-f16.gguf";

/// Every embed the router makes goes through here. A wedged embed server
/// (up but never healthy) used to cost each call a 30 s wait inside
/// ensure_embedding_server - on every routed turn, and thousands of times
/// in the matrix's routing leg (the Windows beta.16 "stuck" report). Now:
/// a 15 s ceiling per call, and after a failure or timeout embedding is
/// treated as unavailable for a minute (the gates fall back to keywords
/// and the fail-safe, exactly as with no embedding model) before it is
/// tried again.
static EMBED_BROKEN_UNTIL: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
const EMBED_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const EMBED_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

async fn embed_guarded(app: &AppHandle, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
    if let Ok(g) = EMBED_BROKEN_UNTIL.lock() {
        if let Some(until) = *g {
            if std::time::Instant::now() < until {
                return Err("embedding paused after a failure (retrying in under a minute)".into());
            }
        }
    }
    let state = app.state::<crate::llm::LLMState>();
    let result = match tokio::time::timeout(
        EMBED_CALL_TIMEOUT,
        crate::llm::embed_texts(app.clone(), state, texts, EMBED_MODEL.to_string()),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => Err(format!("embedding took longer than {} s", EMBED_CALL_TIMEOUT.as_secs())),
    };
    if let Err(e) = &result {
        log::warn!("[router] embedding unavailable: {e} - keyword gates only for the next minute");
        if let Ok(mut g) = EMBED_BROKEN_UNTIL.lock() {
            *g = Some(std::time::Instant::now() + EMBED_RETRY_AFTER);
        }
    }
    result
}

/// How much better (on the 0–9 task scale) a candidate must be than the loaded
/// model to justify a reload. Generous so near-equal models don't thrash; small
/// enough that a real coder (coding 9) beats a general model (coding 6) on code.
pub(crate) const SWITCH_MARGIN: u8 = 2;

/// The one online dial ("How much goes online", Settings > Routing):
/// `frontier` (default - ordinary questions go to the Everyday online
/// model, the device answers only when it is as good) | `balanced` (the
/// device answers when it is nearly as good) | `local` (online only for
/// live-web needs, hard questions stay home). The pre-0.7.0 "eagerness"
/// values map onto it so old callers and stored settings keep working.
/// Returns "" for an unknown/absent value (the caller falls back to the
/// store, then to frontier).
pub(crate) fn normalize_share(s: &str) -> &'static str {
    match s.trim().to_lowercase().as_str() {
        "frontier" | "freshness" => "frontier",
        "local" | "privacy" => "local",
        "balanced" => "balanced",
        _ => "",
    }
}

/// Freshness threshold (max cosine vs the reference phrases) per dial
/// position. Calibrated against bge-small on real queries: fresh queries
/// cluster ≥0.57, evergreen ≤0.564. Balanced (0.58) keeps evergreen local
/// while catching clearly-fresh paraphrases; local-first only escalates
/// obvious cases; frontier leans online.
fn threshold_for(share: &str) -> f32 {
    match share {
        "local" => 0.62,
        "frontier" => 0.55,
        _ => 0.58, // balanced
    }
}

/// Does the device take an easy question instead of the Everyday online
/// model? Frontier-first: only when the model that would answer here is
/// genuinely BETTER on this task (Eric's rule: replace online where offline
/// is genuinely better). Balanced: when it is as good. Local-first never
/// decides on capability (freshness alone sends a question out).
fn local_wins(share: &str, local_cap: u8, everyday_cap: u8) -> bool {
    match share {
        "frontier" => local_cap > everyday_cap,
        "balanced" => local_cap >= everyday_cap,
        _ => false,
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
    // Paraphrase-shaped (no cue words) - the battery's fresh_paraphrase
    // bucket scored below every threshold before these were added.
    "which phone did they announce most recently",
    "is the bridge still closed for repairs",
    "which team is top of the league table",
    "how much does petrol cost at the moment",
    "has the new line opened yet",
    "what is the newest model available",
    "are flights running normally today",
    "who is the prime minister right now",
    "what is the price of bitcoin",
    "has the new movie come out yet",
    "is the park open again after the flooding",
    "which browser just shipped a major release",
    "what did the central bank decide at its last meeting",
    "will it rain tomorrow",
    "which country most recently joined",
    "what is the weather forecast for this weekend",
    "who won the match this weekend",
    "what is the latest score in the game",
    "is there a storm warning this week",
    "who won the election",
];

pub(crate) fn cosine(a: &[f32], b: &[f32]) -> f32 {
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

/// Ordinary-question anchors for the freshness gate (its own list - the
/// health gate's anchors include news-shaped phrasings that would sit next
/// to genuine live-web questions). A turn is fresh only when it is closer
/// to the fresh references than to these by FRESH_MARGIN.
const FRESH_BENIGN_REFERENCES: &[&str] = &[
    "hi, how are you doing today",
    "write a short poem for me",
    "rewrite this to sound friendlier",
    "suggest a fun activity for a rainy day",
    "explain how this works in simple terms",
    "what should I pack for a trip",
    "tell me a joke",
    "help me write an email to my boss",
    "what is a good recipe for dinner",
    "what is the capital of this country",
    "how many ounces are in a cup",
];
static FRESH_BENIGN_REFS: tokio::sync::OnceCell<Vec<Vec<f32>>> = tokio::sync::OnceCell::const_new();

async fn fresh_benign_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    FRESH_BENIGN_REFS
        .get_or_try_init(|| async {
            let texts = FRESH_BENIGN_REFERENCES.iter().map(|s| s.to_string()).collect();
            embed_guarded(app, texts).await
        })
        .await
        .map_err(|e| log::warn!("[router] freshness anchors could not be embedded: {e}"))
        .ok()
}

async fn fresh_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    FRESH_REFS
        .get_or_try_init(|| async {
            let texts = FRESH_REFERENCES.iter().map(|s| s.to_string()).collect();
            embed_guarded(app, texts).await
        })
        .await
        .map_err(|e| log::warn!("[router] freshness references could not be embedded: {e}"))
        .ok()
}

/// Stage-1 semantic freshness: how close (max cosine) is the query to the
/// "needs-current-info" reference phrases? `None` if embedding is unavailable
/// (→ caller treats as not-fresh, i.e. stays offline). Catches paraphrases the
/// keyword gate misses ("what's the newest phone", "is it raining").
/// Contrastive freshness: the fresh score, minus how close the turn also
/// sits to ordinary-question anchors. A greeting with "today" in it or a
/// creative ask scores high on the fresh references alone; against the
/// benign anchors it scores higher still, and the margin sends it home.
const FRESH_MARGIN: f32 = 0.05;

async fn semantic_fresh_score(
    app: &AppHandle,
    query: &str,
    provided_vec: Option<&[f32]>,
) -> Option<f32> {
    let (fresh, benign) = fresh_scores(app, query, provided_vec).await?;
    if fresh - benign < FRESH_MARGIN {
        log::debug!("[router] freshness pass-through (fresh {fresh:.3}, benign {benign:.3})");
        return Some(0.0);
    }
    Some(fresh)
}

/// Dev preview / calibration: both raw scores.
pub async fn fresh_scores_preview(app: &AppHandle, query: &str) -> Option<(f32, f32)> {
    fresh_scores(app, query, None).await
}

async fn fresh_scores(
    app: &AppHandle,
    query: &str,
    provided_vec: Option<&[f32]>,
) -> Option<(f32, f32)> {
    let refs = fresh_reference_vecs(app).await?;
    let benign = fresh_benign_reference_vecs(app).await?;
    // The frontend embeds the turn's text ONCE (same bge query instruction)
    // and shares the vector here and with memory retrieval - reuse it rather
    // than paying a second embed-server round trip. Callers without one
    // (the inference API) fall back to embedding here.
    let qvec: Vec<f32> = match provided_vec {
        Some(v) if !v.is_empty() => v.to_vec(),
        _ => {
            let qtext = format!("{QUERY_INSTRUCTION}{query}");
            embed_guarded(app, vec![qtext])
                .await
                .ok()?
                .pop()?
        }
    };
    let fresh = refs.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max);
    let benign_score = benign.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max);
    Some((fresh, benign_score))
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
    // Generic image and document questions: without these, "what's in this
    // image?" had no benign neighbor and sat next to "explain this x-ray
    // image to me" - a random photo routed to the medical model.
    "what is in this image",
    "describe this picture for me",
    "what does this photo show",
    "what is in this pdf",
    "summarize this document for me",
    "what is this file about",
    // Math and code anchors: "the integral of x squared" sat next to
    // "explain this x-ray" with nothing math-shaped on this side (battery
    // math bucket, 2/10 flagged as health).
    "what is the integral of this function",
    "compute the limit of this expression as x approaches zero",
    "solve this equation for x step by step",
    "what is the derivative of x squared",
    "write a function in python that parses a file",
];

static MEDICAL_REFS: tokio::sync::OnceCell<Vec<Vec<f32>>> = tokio::sync::OnceCell::const_new();
static BENIGN_REFS: tokio::sync::OnceCell<Vec<Vec<f32>>> = tokio::sync::OnceCell::const_new();

async fn medical_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    MEDICAL_REFS
        .get_or_try_init(|| async {
            let texts = MEDICAL_REFERENCES.iter().map(|s| s.to_string()).collect();
            embed_guarded(app, texts).await
        })
        .await
        .ok()
}

async fn benign_reference_vecs(app: &AppHandle) -> Option<&'static Vec<Vec<f32>>> {
    BENIGN_REFS
        .get_or_try_init(|| async {
            let texts = BENIGN_REFERENCES.iter().map(|s| s.to_string()).collect();
            embed_guarded(app, texts).await
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
        // Clinical terms with no software meaning (the battery's health
        // bucket found these slipping past the keyword stage).
        "bloodwork", "lab report", "white blood cell", "hemoglobin", "haemoglobin",
        "liver panel", "thyroid", "vitamin d", "side effects", "dosage",
        "acetaminophen", "paracetamol", "ibuprofen", "antibiotic", "metformin",
        "diagnosed with", "prediabetes", "diabetes", "migraine", "neurologist",
        "cardiologist", "bone density", "t-score", "urinary tract", "see a doctor",
        "vaccination", "chest tightness", "chest pain", "persistent cough",
        "low-grade fever", "symptoms of a", " ecg", " ekg", "antihistamine",
    ];
    CUES.iter().any(|c| q.contains(c))
}

/// Is this turn about the user's health? Keyword stage first (free), then the
/// semantic stage against MEDICAL_REFERENCES using the turn's shared
/// embedding. `false` when embedding is unavailable and no keyword hits.
pub(crate) async fn is_medical_turn(app: &AppHandle, query: &str, query_vec: Option<&[f32]>) -> bool {
    is_medical_turn_with_margin(app, query, query_vec, MEDICAL_MARGIN).await
}

/// The vision picker's bar: a text-routing false positive only keeps a
/// question home (the safe direction); a vision-pick false positive swaps
/// a general vision model for the medical one on a holiday photo.
pub(crate) const VISION_MEDICAL_MARGIN: f32 = 0.15;

pub(crate) async fn is_medical_turn_with_margin(
    app: &AppHandle,
    query: &str,
    query_vec: Option<&[f32]>,
    min_margin: f32,
) -> bool {
    medical_check(app, query, query_vec, min_margin).await.0
}

/// The health gate with its own honesty flag: `(medical, semantic_ran)`.
/// `semantic_ran == false` means only the keyword stage could run (no
/// embedding model, or the embed server would not start) - the router must
/// then not send an unchecked question online.
pub(crate) async fn medical_check(
    app: &AppHandle,
    query: &str,
    query_vec: Option<&[f32]>,
    min_margin: f32,
) -> (bool, bool) {
    if looks_medical(query) {
        return (true, true);
    }
    let Some(refs) = medical_reference_vecs(app).await else {
        log::warn!("[router] health check: medical references unavailable (embedding down?)");
        return (false, false);
    };
    let Some(benign) = benign_reference_vecs(app).await else {
        log::warn!("[router] health check: benign references unavailable (embedding down?)");
        return (false, false);
    };
    let qvec: Vec<f32> = match query_vec {
        Some(v) if !v.is_empty() => v.to_vec(),
        _ => {
            let qtext = format!("{QUERY_INSTRUCTION}{query}");
            match embed_guarded(app, vec![qtext]).await {
                Ok(mut v) if !v.is_empty() => v.remove(0),
                Err(e) => {
                    log::warn!("[router] health check: the turn could not be embedded: {e}");
                    return (false, false);
                }
                _ => return (false, false),
            }
        }
    };
    let med_score = refs.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max);
    let benign_score = benign.iter().map(|r| cosine(&qvec, r)).fold(0.0f32, f32::max);
    // Contrastive: medical only when the turn is BOTH related to the medical
    // refs AND meaningfully closer to them than to ordinary-question anchors.
    if med_score >= MEDICAL_THRESHOLD && med_score - benign_score >= min_margin {
        log::info!(
            "[router] medical semantic gate hit (med {med_score:.3}, benign {benign_score:.3}, margin {:.3})",
            med_score - benign_score
        );
        return (true, true);
    }
    log::debug!(
        "[router] medical gate pass-through (med {med_score:.3}, benign {benign_score:.3})"
    );
    (false, true)
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
    ];
    CUES.iter().any(|c| q.contains(c))
}

/// One offline candidate reduced to what the ranking sees — extracted from
/// `pick_offline` so the lean orderings are unit-testable without an app.
#[derive(Clone, Copy, Debug)]
struct OfflineRank {
    cap: u8,      // task capability score
    tier: u8,     // fit: green/split 2 / yellow 1 / red 0
    params_b: f64,
    /// This machine's measured tokens/sec for the model, when it has been
    /// used here. The "speed" lean ranks by it when both sides have one;
    /// size stays the proxy until then.
    tps: Option<f64>,
}

/// Compare two measured speeds (greater = faster); Equal when either is
/// unknown, so the size proxy decides.
fn faster(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => std::cmp::Ordering::Equal,
    }
}

/// Capability used to rank agent candidates: the tool-driving tier
/// dominates, the task score (coding for agent turns, reasoning for
/// planning) breaks ties.
fn agent_rank_cap(agent_caps: u8, task_cap: u8) -> u8 {
    agent_caps.saturating_mul(10).saturating_add(task_cap.min(9))
}

/// Slowest measured speed at which a local model still counts as ready for
/// agent work. Agent loops burn tokens for many steps; below this a
/// session is a crawl however well the model fits.
pub const AGENT_MIN_TPS: f64 = 8.0;

/// Is this local model ready to carry agent work here: a fast fit (fully on
/// the card, or GPU + RAM) AND, once it has been measured on this machine,
/// at least `AGENT_MIN_TPS`. Unmeasured = the fit decides.
pub fn agent_ready(f: &crate::fit::ModelFit) -> bool {
    f.fit.is_fast() && agent_speed_ok(f.measured_tps)
}

fn agent_speed_ok(measured_tps: Option<f64>) -> bool {
    measured_tps.map_or(true, |t| t >= AGENT_MIN_TPS)
}

/// The lean-dependent ordering (greater = better; see `pick_offline` docs).
fn offline_ordering(lean: &str, a: OfflineRank, b: OfflineRank) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    match lean {
        // Speed: measured tokens/sec on THIS machine decides when known for
        // both (a split 35B at 30 tok/s beats a dense 9B at 22); otherwise
        // the fit tier, then capability, then SMALLER (the old proxy).
        "speed" => faster(a.tps, b.tps)
            .then(a.tier.cmp(&b.tier))
            .then(a.cap.cmp(&b.cap))
            // Smaller wins size ties — fewer parameters = faster tokens.
            .then(b.params_b.partial_cmp(&a.params_b).unwrap_or(Equal)),
        "quality" => a
            .cap
            .cmp(&b.cap)
            .then(a.params_b.partial_cmp(&b.params_b).unwrap_or(Equal))
            .then(a.tier.cmp(&b.tier)),
        // Balanced: a model that runs COMFORTABLY beats a smarter one that
        // barely loads - on small hardware cap-first systematically chose
        // partial-offload models that crawled or timed out at load. Within
        // a tier, capability decides; size breaks ties upward (quality).
        _ => a
            .tier
            .cmp(&b.tier)
            .then(a.cap.cmp(&b.cap))
            // Equal capability in the same tier: the measured-faster one,
            // else the bigger (quality).
            .then(faster(a.tps, b.tps))
            .then(a.params_b.partial_cmp(&b.params_b).unwrap_or(Equal)),
    }
}

/// Pick the best offline model. `assess` already excludes embedding models
/// (bge etc.) and models that don't fit at all (red) are de-prioritised.
/// Capability is the **task-specific** score (`by_task` — coding for a code
/// query, etc.; `"general"` → overall). `lean` biases the ranking via
/// `offline_ordering`:
/// - `"balanced"` (default): fit tier first (green = fully-on-GPU over
///   yellow = partial offload), then capability, then size — a model that
///   runs comfortably beats a smarter one that barely loads.
/// - `"speed"`: fit tier first, then capability, and SMALLER wins size ties —
///   a fully-on-GPU model over a stronger one that spills to CPU.
/// - `"quality"`: capability, then size, then fit tier — willing to take
///   partial offload for the strongest model that still runs.
/// To avoid reloading on every turn, the currently-loaded model is kept unless
/// a candidate beats it on THIS task by at least `SWITCH_MARGIN` (so a real
/// specialist for a real task is worth the reload, but near-equal models
/// don't thrash).
async fn pick_offline(app: &AppHandle, task: &str, lean: &str, agent_only: bool) -> Result<String, String> {
    pick_offline_for(app, task, lean, agent_only, None).await
}

/// Room a reply needs beyond the prompt when the turn's size is known.
const REPLY_ROOM_TOKENS: u32 = 1024;

/// `pick_offline` with the turn's size: candidates whose runtime context
/// cannot hold prompt + reply room are set aside ("needs a longer context");
/// if none can, the largest-context candidates stay so the user gets the
/// best that exists rather than a refusal.
async fn pick_offline_for(app: &AppHandle, task: &str, lean: &str, agent_only: bool, turn_tokens: Option<u32>) -> Result<String, String> {
    pick_offline_detail(app, task, lean, agent_only, turn_tokens).await.map(|p| p.name)
}

/// The offline pick with what the "as good as online" rung needs to know
/// about it: whether it runs at full speed here, and its score on THIS
/// task. Computed on the model that will actually serve (after the
/// keep-loaded rule), never on the ranked best.
pub(crate) struct OfflinePick {
    pub name: String,
    pub fast: bool,
    pub cap: u8,
}

async fn pick_offline_detail(app: &AppHandle, task: &str, lean: &str, agent_only: bool, turn_tokens: Option<u32>) -> Result<OfflinePick, String> {
    let mut all = crate::fit::assess(app).await;
    // Agent sessions: tool-driving is a hard filter, not a preference.
    if agent_only {
        // Family capability AND the file's actual template: community
        // builds ship templates that hard-reject agent-shaped
        // conversations (strict role alternation) or lack tool support -
        // a 400 at the first step, whatever the family name promises.
        all.retain(|f| crate::model_caps::agent_caps(&f.name) >= 6 && f.agent_template_ok);
    }
    if all.is_empty() {
        return Err("No offline models downloaded".to_string());
    }
    if let Some(need) = turn_tokens {
        let need = need.saturating_add(REPLY_ROOM_TOKENS) as u64;
        let fits_ctx: Vec<crate::fit::ModelFit> = all
            .iter()
            .filter(|f| f.context_runtime == 0 || f.context_runtime >= need)
            .cloned()
            .collect();
        if !fits_ctx.is_empty() {
            if fits_ctx.len() < all.len() {
                log::info!(
                    "[router] turn needs ~{need} tokens of context - {} of {} models set aside (too short a runtime context)",
                    all.len() - fits_ctx.len(),
                    all.len()
                );
            }
            all = fits_ctx;
        } else {
            let max_ctx = all.iter().map(|f| f.context_runtime).max().unwrap_or(0);
            all.retain(|f| f.context_runtime == max_ctx);
            log::warn!("[router] no installed model holds ~{need} tokens; keeping the largest-context ones ({max_ctx})");
        }
    }

    // fit tier: green / split (fast) = 2 > yellow = 1 > red = 0.
    let tier = |f: &crate::fit::ModelFit| f.fit.tier();
    // Agent sessions rank on tool-driving capability first (the task score
    // breaks ties) and always in the balanced order - a weak driver wastes
    // a whole session, so the speed lean only decides between equals.
    // Learned family shifts (bounded, this machine's verdicts) ride the task
    // score; the health task never learns.
    let adj = if task == "medical" { Adjustments::default() } else { learned_adjustments(app) };
    let cap = |name: &str| {
        if agent_only {
            agent_rank_cap(crate::model_caps::agent_caps(name), crate::model_caps::caps_for(name).by_task(task))
        } else {
            shifted(crate::model_caps::caps_for(name).by_task(task), adj.family(name))
        }
    };
    let lean = if agent_only { "balanced" } else { lean };

    // Prefer models that actually run (green/yellow); fall back to red only if
    // every model is red (better to try than to refuse). Models that already
    // OOM'd the GPU this session are excluded outright — the loader rejects
    // them instantly, so picking one would turn every auto request into an
    // error even though the pre-load fit estimate still grades them runnable.
    let usable = |f: &&crate::fit::ModelFit| {
        !crate::llm::is_model_too_big(f.name.clone()) && !crate::llm::is_model_rejected(f.name.clone())
    };
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

    // Paused models are out of Auto's reach. Same fallback rule as the
    // specialist filter: if EVERYTHING is paused, answering beats refusing
    // (the user paused models, not the app), and the loaded-model keep
    // below only keeps pool members - so pausing the loaded model also
    // RELEASES it on the next pick.
    let paused = paused_models(app);
    let pool: Vec<&crate::fit::ModelFit> = if paused.is_empty() {
        pool
    } else {
        let unpaused: Vec<&crate::fit::ModelFit> =
            pool.iter().copied().filter(|f| !paused.contains(&f.name)).collect();
        if unpaused.is_empty() { pool } else { unpaused }
    };

    // Health questions honor the user's explicit model choice (Settings >
    // Routing) before any ranking: their pick answers whenever it is
    // installed, runnable, and not paused (= in the pool). Absent or not
    // usable right now, ranking decides exactly as before - the specialist
    // wins by capability. The choice is a preference, never a wall.
    if task == "medical" {
        if let Some(pref) = medical_preferred_model(app) {
            if let Some(chosen) = pool.iter().find(|f| f.name == pref) {
                // Honored when it runs well here. A pick that would crawl
                // (partial offload) yields to a health specialist that fits
                // fully - the preference is for the answer, not a slow one.
                let faster_specialist = !chosen.fit.is_fast()
                    && pool.iter().any(|f| {
                        f.name != chosen.name
                            && f.fit.is_fast()
                            && crate::model_caps::caps_for(&f.name).medical >= 8
                    });
                if !faster_specialist {
                    log::info!("[router] medical turn -> user's preferred model {}", chosen.name);
                    return Ok(OfflinePick { name: chosen.name.clone(), fast: chosen.fit.is_fast(), cap: cap(&chosen.name) });
                }
                log::info!(
                    "[router] medical pick {} would run slowly here - a health specialist that fits fully answers instead",
                    chosen.name
                );
            }
        }
    }

    let rank = |f: &crate::fit::ModelFit| OfflineRank { cap: cap(&f.name), tier: tier(f), params_b: f.params_b,
        tps: f.measured_tps,
    };
    let best = *pool
        .iter()
        .max_by(|a, b| offline_ordering(lean, rank(a), rank(b)))
        .unwrap(); // pool is non-empty

    // Keep the loaded model unless a candidate beats it on THIS task by at least
    // SWITCH_MARGIN — a reload is worth it for a real specialist, not a marginal
    // gain. Fit-aware since beta.10: see keep_loaded. A load IN FLIGHT counts
    // as the incumbent too - current_model is only written when a load
    // finishes, and an unprotected 20s load window let a bigger model hijack
    // the pick mid-load (0.4.0-beta.1 field find).
    let current = {
        let st = app.state::<crate::llm::LLMState>();
        let loading = st.loading_model.lock().await.clone();
        let loaded = st.current_model.lock().await.clone();
        loading.or(loaded)
    };
    if let Some(cur) = current.filter(|c| !c.starts_with("online:")) {
        if let Some(cf) = pool.iter().find(|f| f.name == cur) {
            let folder_open = app
                .state::<crate::agent_bridge::AgentBridgeState>()
                .has_open_folder()
                .await;
            if keep_loaded(cap(&cf.name), cap(&best.name), tier(cf), tier(best), folder_open, best.load_secs) {
                return Ok(OfflinePick { name: cur.clone(), fast: cf.fit.is_fast(), cap: cap(&cf.name) });
            }
        }
    }
    Ok(OfflinePick { name: best.name.clone(), fast: best.fit.is_fast(), cap: cap(&best.name) })
}

/// Should the loaded model stay loaded instead of switching to the
/// ranked-best candidate?
///
/// Capability: a reload is worth it for a real specialist (SWITCH_MARGIN),
/// never for a marginal gain. Fit: a partially-offloaded (yellow) loaded
/// model does NOT hold the slot against a fully-on-GPU (green) candidate -
/// every reply it keeps is a slow one (seen live: a 30B partial-offload
/// answering "why is the sky blue" because nothing could out-CAP it) -
/// UNLESS a project folder is open: agent sessions are deliberately
/// session-stable, and evicting the project's model for a stray chat
/// question costs a minutes-long reload on the way back.
/// Extra switch margin for a candidate that is slow to load here: a reload
/// that costs most of a minute must buy a clearly better model. Measured
/// load time (model-stats) above ~10 s adds one point; above ~40 s, two.
pub(crate) fn reload_margin(load_secs: Option<f64>) -> u8 {
    match load_secs {
        Some(s) if s >= 40.0 => 2,
        Some(s) if s >= 10.0 => 1,
        _ => 0,
    }
}

pub(crate) fn keep_loaded(
    cap_cur: u8,
    cap_best: u8,
    tier_cur: u8,
    tier_best: u8,
    folder_open: bool,
    load_secs_best: Option<f64>,
) -> bool {
    // A real specialist wins regardless of fit or folders - but a candidate
    // that is slow to load here must win by more (reload_margin).
    if cap_cur + SWITCH_MARGIN + reload_margin(load_secs_best) < cap_best {
        return false;
    }
    tier_cur >= tier_best || folder_open
}

/// The user's per-slot online model choices from Settings. `None` = use the
/// recommended default. Values are `online:<id>` strings exactly as returned
/// by `list_online_models`.
#[derive(Default)]
pub struct OnlinePicks {
    pub fresh: Option<String>,
    /// The Everyday slot: ordinary turns in Online and Offline mode.
    pub everyday: Option<String>,
    /// The one Hard slot (Settings shows one picker). The two legacy
    /// per-task keys below still apply when set and this is not.
    pub hard: Option<String>,
    pub hard_code: Option<String>,
    pub hard_general: Option<String>,
    /// Agent (folder) turns' online model - the tool-driver slot.
    pub agent: Option<String>,
    /// Planning/helper subagents' online model (reasoning-lean tool-driver).
    pub plan: Option<String>,
}

impl OnlinePicks {
    /// The user's Settings picks, read from the app store - for callers with
    /// no webview in sight (the inference API, and through it every agent
    /// session). Default::default() here silently discarded a user's chosen
    /// agent model.
    pub fn from_store(app: &AppHandle) -> Self {
        Self {
            fresh: store_pref(app, "routingOnlineFresh"),
            everyday: store_pref(app, "routingOnlineEveryday"),
            hard: store_pref(app, "routingOnlineHard"),
            hard_code: store_pref(app, "routingOnlineHardCode"),
            hard_general: store_pref(app, "routingOnlineHardGeneral"),
            agent: store_pref(app, "routingOnlineAgent"),
            plan: store_pref(app, "routingOnlinePlanning"),
        }
    }
}

/// Recommended defaults per routing slot. Ids match the proxy catalog; if one
/// is missing (the catalog moved on) selection falls back to the capability
/// registry, so routing never breaks. The Settings page names these same
/// models as "Recommended" - keep the two in sync.
pub(crate) const DEFAULT_FRESH: &str = "online:grok-4.6-search";
/// The Everyday slot: frontier-class quality at the lowest price in the
/// catalog - what an ordinary question gets in Online and Offline mode.
pub(crate) const DEFAULT_EVERYDAY: &str = "online:gpt-5.6-luna";
const DEFAULT_HARD_CODE: &str = "online:gpt-5.6-sol";
const DEFAULT_HARD_GENERAL: &str = "online:gpt-5.6-terra";
/// The agent slot: the strongest proven tool-driver. Sol runs tools
/// through the Responses passthrough (forced-tool calls measured ~1.7s
/// with zero reasoning tokens on simple steps - it scales thinking to
/// the step), so the flagship drives projects by default.
const DEFAULT_AGENT: &str = "online:gpt-5.6-sol";
/// Planning leans reasoning; must still drive tools (planners read files).
const DEFAULT_PLAN: &str = "online:gpt-5.6-sol";

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
    // Ids arrive in two shapes: list_online_models prefixes "online:",
    // stored picks and slot defaults may carry it or not. Normalize BOTH
    // sides - comparing a stripped needle against prefixed ids (or vice
    // versa) silently kills every match and the alphabetical fallback
    // takes over.
    let by_id = |id: &str| {
        let want = id.strip_prefix("online:").unwrap_or(id);
        models
            .iter()
            .find(|m| m.id.strip_prefix("online:").unwrap_or(&m.id) == want)
            .map(|m| m.id.clone())
    };
    let text = |m: &crate::flowsta::OnlineModel| {
        format!("{} {} {}", m.id, m.display_name, m.description).to_lowercase()
    };
    let has_search_fee = |m: &crate::flowsta::OnlineModel| {
        m.pricing.as_ref().and_then(|p| p.search_per_call_usd).is_some()
    };
    // The live-web slot needs a model that can actually search: a stored
    // pick that cannot (a plain chat model) is not honored for fresh turns.
    let search_capable = |id: &str| {
        models.iter().find(|m| m.id == id).map_or(false, |m| {
            let t = text(m);
            t.contains("search") || t.contains("sonar") || t.contains("perplexity") || has_search_fee(m)
        })
    };
    if let Some(hit) = pref.and_then(|p| by_id(p)) {
        if !fresh || search_capable(&hit) {
            return Some(hit);
        }
        log::info!("[router] live-web pick {hit} cannot search - using the recommended search model");
    }

    if fresh {
        if let Some(hit) = by_id(DEFAULT_FRESH) {
            return Some(hit);
        }
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
    let by_id = |id: &str| {
        let want = id.strip_prefix("online:").unwrap_or(id);
        models
            .iter()
            .find(|m| m.id.strip_prefix("online:").unwrap_or(&m.id) == want)
            .map(|m| m.id.clone())
    };
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

/// The context window the agent should believe for a folder session on
/// this AI: the SERVING model's window, resolved the same deterministic
/// way agent turns route. Local serving keeps the true loaded size
/// (compaction must fire before a real 16k window overflows); online
/// serving takes the catalog's number - Sol is a million-token model, and
/// believing a local 16k there compacted constantly for nothing.
pub async fn agent_serving_context(
    app: &AppHandle,
    ai_model: &str,
    eagerness: &str,
    plan: bool,
) -> u64 {
    let local = crate::llm::current_ctx_size() as u64;
    let catalog_ctx = |models: &[crate::flowsta::OnlineModel], id: &str| -> Option<u64> {
        models
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.context_window)
            .filter(|c| *c > 0)
    };
    if ai_model.starts_with("online:") {
        if let Ok(models) = crate::flowsta::list_online_models().await {
            if let Some(c) = catalog_ctx(&models, ai_model) {
                return c;
            }
        }
        return local;
    }
    let Some(mode) = ai_model.strip_prefix("auto:") else {
        return local; // pinned local model (or external server): local truth
    };
    if mode != "online-offline" {
        return local;
    }
    // Mirror route_inner's agent branch: privacy with a capable local model
    // serves locally; otherwise the Agent/Planning slot chain decides.
    let offline_task = if plan { "reasoning" } else { "code" };
    // Same bar as route_inner: the picked local model must be READY for
    // agent work (fast fit, measured speed), not merely installed.
    let offline_ok = match pick_offline(app, offline_task, "balanced", true).await {
        Ok(name) => crate::fit::assess(app).await.iter().any(|f| f.name == name && agent_ready(f)),
        Err(_) => false,
    };
    if (normalize_share(eagerness) == "local" || store_pref(app, "routingProjectThrifty").as_deref() == Some("1"))
        && offline_ok
    {
        return local;
    }
    if let Ok(models) = crate::flowsta::list_online_models().await {
        let pref = if plan {
            store_pref(app, "routingOnlinePlanning").or_else(|| Some(DEFAULT_PLAN.to_string()))
        } else {
            agent_online_override()
                .or_else(|| store_pref(app, "routingOnlineAgent"))
                .or_else(|| {
                    store_pref(app, "routingOnlineHardCode")
                        .filter(|id| crate::model_caps::online_agent_caps(id) >= 6)
                })
        };
        if let Some(id) = select_online_agent(&models, pref.as_deref()) {
            if let Some(c) = catalog_ctx(&models, &id) {
                return c;
            }
        }
    }
    local
}

/// Should simple project side-work (explore subagents) run on the device?
/// Defaults ON - but only takes effect when a capable local model runs
/// COMFORTABLY: green fit (fully on GPU). "Capable and loads" is not
/// enough - a partial-offload model turns every explore fan-out into
/// minutes of critical-path crawl, worse than the cheap online driver it
/// replaces. Free and private when it engages; the leader stays in charge.
/// Documented in the routing explainer.
pub async fn device_subagents_enabled(app: &AppHandle) -> bool {
    if store_pref(app, "routingProjectDeviceSubagents").as_deref() == Some("0") {
        return false;
    }
    crate::fit::assess(app)
        .await
        .iter()
        .any(|f| crate::model_caps::agent_caps(&f.name) >= 6 && agent_ready(f))
}

/// A slot preference from the Rust-readable settings store (mirrored there
/// by the Settings page so non-webview callers see user choices).
/// Models the user PAUSED on the models pages (mirrored from the
/// frontend's localStorage into the store on every change + at launch).
/// Pause hides a model from every chooser; the router honors it too -
/// "hide it from me" and "don't hand it to me" are the same intent.
/// The user's preferred offline model for health questions (Settings >
/// Routing). Empty/absent = no explicit choice - ranking decides (the
/// medical specialist wins by capability when installed).
fn medical_preferred_model(app: &AppHandle) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").ok()?;
    store
        .get("medicalPreferredModel")
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
}

fn paused_models(app: &AppHandle) -> std::collections::HashSet<String> {
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store("settings.json") else { return Default::default() };
    store
        .get("pausedModels")
        .and_then(|v| v.as_array().cloned())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Boolean settings mirrored from the frontend (stored as "true"/"false"
/// strings or JSON bools). Absent = false.
pub(crate) fn store_pref_bool(app: &AppHandle, key: &str) -> bool {
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store("settings.json") else { return false };
    match store.get(key) {
        Some(v) => v.as_bool().unwrap_or_else(|| v.as_str() == Some("true")),
        None => false,
    }
}

fn store_pref(app: &AppHandle, key: &str) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").ok()?;
    store
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
}

/// Can any runnable model on this device hold `need` tokens - at the
/// context it runs at, or at a rung it could grow to (the same test
/// `ensure_context` applies before a send)?
async fn local_can_hold(app: &AppHandle, need: u64) -> bool {
    let runnable: Vec<crate::fit::ModelFit> = crate::fit::assess(app)
        .await
        .into_iter()
        .filter(|f| !matches!(f.fit, crate::fit::Fit::Red))
        .collect();
    if runnable.is_empty() {
        return false;
    }
    if runnable.iter().any(|f| f.context_runtime >= need) {
        return true;
    }
    let Ok(dir) = crate::llm::get_models_dir(app) else {
        return false;
    };
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let free_vram_gb = crate::llm::available_vram_mib(app).await.map(|m| m as f64 / 1024.0);
    for f in runnable {
        let path = dir.join(&f.name);
        let Ok(meta) = crate::gguf::read_meta(&path) else { continue };
        let Ok(size) = std::fs::metadata(&path).map(|m| m.len()) else { continue };
        if crate::fit::ctx_for_need(&meta, size, total_ram_gb, free_vram_gb, need).is_some() {
            return true;
        }
    }
    false
}

/// An online model whose context holds `need` tokens: the user's slot pick
/// when it does, else the best general pick among those that do.
async fn pick_online_holding(need: u64, task: &str, pref: Option<&str>) -> Option<String> {
    let models = crate::flowsta::list_online_models().await.ok()?;
    let roomy: Vec<crate::flowsta::OnlineModel> = models
        .into_iter()
        .filter(|m| m.context_window >= need)
        .collect();
    if roomy.is_empty() {
        return None;
    }
    select_online(&roomy, task, false, pref)
}

async fn pick_online(task: &str, fresh: bool, pref: Option<&str>) -> Option<String> {
    let models = crate::flowsta::list_online_models().await.ok()?;
    select_online(&models, task, fresh, pref)
}

/// Follow-up inheritance (see is_followup). Returns the decision when the
/// turn is a same-topic follow-up with usable history, else None so the
/// normal ladder runs.
async fn inherit_followup(
    app: &AppHandle,
    query: &str,
    query_vec: Option<&[f32]>,
    task: &str,
    lean: &str,
    turn_tokens: Option<u32>,
    prev: &PrevTurn,
    picks: &OnlinePicks,
) -> Option<RouteResult> {
    let prev_side = prev.side.as_deref()?;
    if !is_followup(query) || looks_time_sensitive(query) {
        return None;
    }
    if let (Some(pv), Some(qv)) = (prev.vec.as_deref(), query_vec) {
        if !pv.is_empty() && !qv.is_empty() && cosine(qv, pv) < FOLLOWUP_TOPIC_MIN {
            log::info!("[router] follow-up wording but the topic moved - routing afresh");
            return None;
        }
    }
    let prev_task = prev.task.as_deref().filter(|t| !t.is_empty()).unwrap_or(task);
    match prev_side {
        "online" => {
            let models = crate::flowsta::list_online_models().await.ok()?;
            let by_id = |id: &str| {
                let want = id.strip_prefix("online:").unwrap_or(id);
                models.iter().find(|m| m.id.strip_prefix("online:").unwrap_or(&m.id) == want).map(|m| m.id.clone())
            };
            // The same model when it is still there; else the slot for the
            // previous task (a search model stays a search model).
            let same = prev.model.as_deref().and_then(by_id);
            let model = match same {
                Some(m) => m,
                None => {
                    let was_search = prev.model.as_deref().is_some_and(|m| m.contains("search") || m.contains("sonar"));
                    select_online(&models, prev_task, was_search, if was_search { picks.fresh.as_deref() } else { picks.everyday.as_deref() })?
                }
            };
            Some(RouteResult { think: None, model, reason: "online — continuing the conversation".to_string() })
        }
        "device" => {
            let pick = pick_offline_detail(app, prev_task, lean, false, turn_tokens).await.ok()?;
            // The model that answered, when it still runs here; else the
            // best for the previous task.
            let model = match prev.model.as_deref() {
                Some(m) if crate::fit::assess(app).await.iter().any(|f| f.name == m && f.fit.tier() > 0) => m.to_string(),
                _ => pick.name,
            };
            Some(RouteResult { think: None, model, reason: "kept on your device — continuing the conversation".to_string() })
        }
        _ => None,
    }
}

/// The Everyday slot's model. Order: the user's pick → the recommended
/// default → the cheapest priced chat model that is not a search model →
/// None (the device answers). Deliberately never the Hard slot: an absent
/// Everyday default must not cost Hard-slot prices.
fn select_online_everyday(
    models: &[crate::flowsta::OnlineModel],
    pref: Option<&str>,
) -> Option<crate::flowsta::OnlineModel> {
    let by_id = |id: &str| {
        let want = id.strip_prefix("online:").unwrap_or(id);
        models
            .iter()
            .find(|m| m.id.strip_prefix("online:").unwrap_or(&m.id) == want)
            .cloned()
    };
    if let Some(hit) = pref.and_then(|p| by_id(p)) {
        return Some(hit);
    }
    if let Some(hit) = by_id(DEFAULT_EVERYDAY) {
        return Some(hit);
    }
    let is_search = |m: &crate::flowsta::OnlineModel| {
        let t = format!("{} {} {}", m.id, m.display_name, m.description).to_lowercase();
        t.contains("search")
            || t.contains("sonar")
            || t.contains("perplexity")
            || m.pricing.as_ref().and_then(|p| p.search_per_call_usd).is_some()
    };
    let cost = |m: &crate::flowsta::OnlineModel| {
        m.pricing.as_ref().map(|p| p.input_per_mtok + p.output_per_mtok)
    };
    models
        .iter()
        .filter(|m| !is_search(m) && cost(m).is_some())
        .min_by(|a, b| {
            cost(a)
                .unwrap_or(f64::MAX)
                .partial_cmp(&cost(b).unwrap_or(f64::MAX))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

/// The Everyday model and its capability scores (for the as-good rung).
async fn pick_online_everyday(pref: Option<&str>) -> Option<(String, crate::model_caps::Caps)> {
    let models = crate::flowsta::list_online_models().await.ok()?;
    let m = select_online_everyday(&models, pref)?;
    let text = format!("{} {} {}", m.id, m.display_name, m.description);
    Some((m.id.clone(), crate::model_caps::online_caps_for(&text)))
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
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct RoutingDecision {
    pub at_ms: i64,
    pub model: String,
    pub reason: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub share: String,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub difficulty: String,
    /// "online" | "device" | "server"
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub think: Option<bool>,
    /// Learned adjustments were in effect for this decision.
    #[serde(default)]
    pub adjusted: bool,
}

/// On-disk ring of real routing decisions (last LOG_KEEP), the source for
/// the Settings line "about N in 10 questions went online" and, later, for
/// per-machine learning. Preview/matrix calls never write here.
const LOG_KEEP: usize = 200;
fn routing_log_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("routing-log.json"))
}
fn read_routing_log(app: &AppHandle) -> Vec<RoutingDecision> {
    routing_log_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}
fn append_routing_log(app: &AppHandle, d: &RoutingDecision) {
    let Some(path) = routing_log_path(app) else { return };
    let mut log = read_routing_log(app);
    log.insert(0, d.clone());
    log.truncate(LOG_KEEP);
    if let Ok(text) = serde_json::to_string(&log) {
        let _ = std::fs::write(path, text);
    }
}

// ── The evidence loop ────────────────────────────────────────────────────
// What the user does with an answer is the only ground truth the router
// gets: "Redo on your device" says the online answer was not worth it,
// "Try this answer online" says the device answer was not good enough,
// regenerate says the answer missed. These land in a local file and move
// two numbers per machine, slowly and within bounds: the as-good margin
// (±1 after 20 verdicts) and a model family's task score (−1 after 10 bad
// verdicts that are a third of its answers). Never across the health gate,
// never a side decision in Offline Only (it may reorder the offline pick).

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct RoutingVerdict {
    pub at_ms: i64,
    /// "redo_device" | "try_online" | "regenerate"
    pub verdict: String,
    pub task: String,
    /// "online" | "device"
    pub side: String,
    pub model: String,
    pub family: String,
}

const FEEDBACK_KEEP: usize = 1000;
fn routing_feedback_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("routing-feedback.json"))
}
fn read_routing_feedback(app: &AppHandle) -> Vec<RoutingVerdict> {
    routing_feedback_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// A stable family key for a model id: online ids as they are (sans
/// prefix); local files by their first two name tokens ("qwen3.5-4b").
pub(crate) fn family_key(model: &str) -> String {
    let m = model.to_lowercase();
    if let Some(id) = m.strip_prefix("online:").or_else(|| m.strip_prefix("external:")) {
        return id.to_string();
    }
    let stem = m.trim_end_matches(".gguf");
    stem.split('-').take(2).collect::<Vec<_>>().join("-")
}

/// Record what the user did with an answer.
#[tauri::command]
pub fn routing_feedback(app: AppHandle, verdict: String, task: String, side: String, model: String) -> Result<(), String> {
    if !matches!(verdict.as_str(), "redo_device" | "try_online" | "regenerate") {
        return Err("unknown verdict".into());
    }
    let path = routing_feedback_path(&app).ok_or("no app data dir")?;
    let mut list = read_routing_feedback(&app);
    list.insert(
        0,
        RoutingVerdict {
            at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_millis() as i64)
                .unwrap_or(0),
            verdict,
            task,
            side,
            family: family_key(&model),
            model,
        },
    );
    list.truncate(FEEDBACK_KEEP);
    let text = serde_json::to_string(&list).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Forget what routing has learned on this machine.
#[tauri::command]
pub fn routing_feedback_reset(app: AppHandle) -> Result<(), String> {
    if let Some(p) = routing_feedback_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// The as-good margin shift from redo-vs-try counts. Pure; unit-tested.
pub(crate) fn margin_shift(redo_device: u32, try_online: u32) -> i8 {
    let n = redo_device + try_online;
    if n < 20 {
        return 0;
    }
    let quarter = n / 4;
    if redo_device >= try_online + quarter {
        1
    } else if try_online >= redo_device + quarter {
        -1
    } else {
        0
    }
}

/// A family's task-score shift from bad verdicts on its device answers.
pub(crate) fn family_shift(bad: u32, answers: u32) -> i8 {
    if bad >= 10 && bad * 10 >= answers * 3 {
        -1
    } else {
        0
    }
}

#[derive(Default, Clone)]
pub(crate) struct Adjustments {
    pub margin: i8,
    pub families: std::collections::HashMap<String, i8>,
}
impl Adjustments {
    pub fn any(&self) -> bool {
        self.margin != 0 || self.families.values().any(|v| *v != 0)
    }
    pub fn family(&self, model: &str) -> i8 {
        self.families.get(&family_key(model)).copied().unwrap_or(0)
    }
}

/// What this machine's verdicts say, bounded (see the section comment).
pub(crate) fn learned_adjustments(app: &AppHandle) -> Adjustments {
    let verdicts = read_routing_feedback(app);
    if verdicts.is_empty() {
        return Adjustments::default();
    }
    let redo = verdicts.iter().filter(|v| v.verdict == "redo_device").count() as u32;
    let try_online = verdicts.iter().filter(|v| v.verdict == "try_online").count() as u32;
    let mut families = std::collections::HashMap::new();
    let log = read_routing_log(app);
    let mut fams: std::collections::HashSet<String> = verdicts.iter().map(|v| v.family.clone()).collect();
    fams.retain(|f| !f.is_empty());
    for f in fams {
        let bad = verdicts
            .iter()
            .filter(|v| v.family == f && v.side == "device" && matches!(v.verdict.as_str(), "try_online" | "regenerate"))
            .count() as u32;
        let answers = log.iter().filter(|d| d.side == "device" && family_key(&d.model) == f).count() as u32;
        let shift = family_shift(bad, answers.max(bad));
        if shift != 0 {
            families.insert(f, shift);
        }
    }
    Adjustments { margin: margin_shift(redo, try_online), families }
}

fn shifted(cap: u8, shift: i8) -> u8 {
    (cap as i16 + shift as i16).clamp(0, 10) as u8
}

/// The share of recent real Online-and-Offline decisions that went online.
#[derive(serde::Serialize)]
pub struct OnlineShareRecent {
    pub online: u32,
    pub total: u32,
}

#[tauri::command]
pub fn routing_online_share_recent(app: AppHandle) -> OnlineShareRecent {
    let mut online = 0u32;
    let mut total = 0u32;
    for d in read_routing_log(&app).iter().filter(|d| d.mode == "online-offline").take(50) {
        total += 1;
        if d.side == "online" {
            online += 1;
        }
    }
    OnlineShareRecent { online, total }
}

static RECENT_DECISIONS: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<RoutingDecision>>> =
    std::sync::OnceLock::new();

fn remember_decision(app: &AppHandle, mode: &str, share: &str, task: &str, difficulty: &str, model: &str, reason: &str, think: Option<bool>) {
    let adjusted = learned_adjustments(app).any();
    let side = if model.starts_with("online:") {
        "online"
    } else if model.starts_with("external:") {
        "server"
    } else {
        "device"
    };
    let d = RoutingDecision {
        at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|t| t.as_millis() as i64)
            .unwrap_or(0),
        model: model.to_string(),
        reason: reason.to_string(),
        mode: mode.to_string(),
        share: share.to_string(),
        task: task.to_string(),
        difficulty: difficulty.to_string(),
        side: side.to_string(),
        think,
        adjusted,
    };
    let ledger = RECENT_DECISIONS.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()));
    if let Ok(mut q) = ledger.lock() {
        q.push_front(d.clone());
        q.truncate(20);
    }
    append_routing_log(app, &d);
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
    share: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    query_vec: Option<&[f32]>,
    agent: bool,
    plan: bool,
) -> Result<RouteResult, String> {
    route_with(app, mode, query, share, task, difficulty, lean, picks, query_vec, agent, plan, None, &PrevTurn::default()).await
}

/// `route_with` for the dev preview / matrix / battery: identical decision,
/// nothing recorded (a sweep must not fill the decision log).
pub async fn route_dry(
    app: &AppHandle,
    mode: &str,
    query: &str,
    share: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    agent: bool,
    plan: bool,
    turn_tokens: Option<u32>,
    prev: &PrevTurn,
) -> Result<RouteResult, String> {
    route_inner(app, mode, query, share, task, difficulty, lean, picks, None, agent, plan, turn_tokens, prev).await
}

/// `route` with the turn's size (prompt + history + attachments, tokens),
/// so the offline pick can set aside models whose runtime context is too
/// short for it.
pub async fn route_with(
    app: &AppHandle,
    mode: &str,
    query: &str,
    share: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    query_vec: Option<&[f32]>,
    agent: bool,
    plan: bool,
    turn_tokens: Option<u32>,
    prev: &PrevTurn,
) -> Result<RouteResult, String> {
    let result = route_inner(app, mode, query, share, task, difficulty, lean, picks, query_vec, agent, plan, turn_tokens, prev).await;
    if let Ok(r) = &result {
        remember_decision(app, mode, effective_share(app, share).as_str(), task, difficulty, &r.model, &r.reason, r.think);
    }
    result
}

/// The dial as Settings has it (for reports).
pub fn current_share(app: &AppHandle) -> String {
    effective_share(app, "")
}

/// The mirrored entitlement says this user cannot use online models.
pub fn known_not_entitled(app: &AppHandle) -> bool {
    store_pref(app, "onlineEntitled").as_deref() == Some("no")
}

/// The dial position this call runs under: an explicit value (legacy
/// eagerness names accepted) → the Settings choice from the store →
/// frontier, the default.
fn effective_share(app: &AppHandle, share: &str) -> String {
    match normalize_share(share) {
        "" => store_pref(app, "routingOnlineShare")
            .map(|s| normalize_share(&s).to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "frontier".to_string()),
        s => s.to_string(),
    }
}

async fn route_inner(
    app: &AppHandle,
    mode: &str,
    query: &str,
    share: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    query_vec: Option<&[f32]>,
    agent: bool,
    plan: bool,
    turn_tokens: Option<u32>,
    prev: &PrevTurn,
) -> Result<RouteResult, String> {
    let mut r = route_core(app, mode, query, share, task, difficulty, lean, picks, query_vec, agent, plan, turn_tokens, prev).await?;
    // Reasoning budget rides with the pick: online models take an effort
    // dial (the proxy forwards it where supported); a local model only
    // when its template has a switch.
    let capable = if r.model.starts_with("online:") {
        true
    } else if r.model.starts_with("external:") {
        false
    } else {
        crate::llm::model_thinks_on_request(&r.model)
    };
    r.think = think_for(difficulty, capable, agent || plan);
    Ok(r)
}

async fn route_core(
    app: &AppHandle,
    mode: &str,
    query: &str,
    share: &str,
    task: &str,
    difficulty: &str,
    lean: &str,
    picks: &OnlinePicks,
    query_vec: Option<&[f32]>,
    agent: bool,
    plan: bool,
    turn_tokens: Option<u32>,
    prev: &PrevTurn,
) -> Result<RouteResult, String> {
    let share_owned = effective_share(app, share);
    let share = share_owned.as_str();
    // One embedding per decision. The chat path hands its vector in; API
    // callers and the preview/battery did not, and the three gates (health,
    // cue, semantic freshness) each embedded the query again - three round
    // trips per decision, 0.86 s each on Windows loopback (beta.17 matrix
    // hit the leg's time cap at 1,043 decisions).
    let embedded_here: Option<Vec<f32>> = match query_vec {
        Some(v) if !v.is_empty() => None,
        _ if mode == "online-offline" || mode == "my-hardware" || !agent => {
            let qtext = format!("{QUERY_INSTRUCTION}{query}");
            embed_guarded(app, vec![qtext]).await.ok().and_then(|mut v| v.pop())
        }
        _ => None,
    };
    let query_vec: Option<&[f32]> = match (query_vec, embedded_here.as_deref()) {
        (Some(v), _) if !v.is_empty() => Some(v),
        (_, Some(v)) => Some(v),
        _ => None,
    };
    // The offline lean: an explicit value wins; absent (API and agent
    // callers, which never see the webview's storage) → the user's
    // Settings choice from the store → balanced.
    let lean_from_store;
    let lean = if lean.is_empty() {
        lean_from_store = store_pref(app, "routingOfflineLean").unwrap_or_else(|| "balanced".to_string());
        lean_from_store.as_str()
    } else {
        lean
    };
    // Slot preferences: explicit params (the in-app chat path reads
    // localStorage) fall back to the Rust-readable settings store, so API
    // and agent callers see the user's choices too - Settings mirrors every
    // slot pick into settings.json.
    let picks = OnlinePicks {
        fresh: picks.fresh.clone().or_else(|| store_pref(app, "routingOnlineFresh")),
        everyday: picks.everyday.clone().or_else(|| store_pref(app, "routingOnlineEveryday")),
        hard: picks.hard.clone().or_else(|| store_pref(app, "routingOnlineHard")),
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
        let t0 = std::time::Instant::now();
        let offline = pick_offline_for(app, offline_task, lean, true, turn_tokens).await.ok();
        let t_offline = t0.elapsed().as_millis();
        // Agent sessions hammer their model for many steps - "might load"
        // is not good enough. Green = fully comfortable on this hardware;
        // anything less turns a whole session into load-timeouts and
        // crawl (a 4B distill got picked by capability and then timed out
        // on a small-GPU machine).
        let tg = std::time::Instant::now();
        let offline_green = match &offline {
            Some(name) => crate::fit::assess(app)
                .await
                .iter()
                .any(|f| f.name == *name && agent_ready(f)),
            None => false,
        };
        let t_green = tg.elapsed().as_millis();
        if mode != "online-offline" {
            return offline
                .map(|m| RouteResult {
                    think: None,
                    model: m,
                    reason: "agent work on your device".to_string(),
                })
                .ok_or_else(|| {
                    "No installed model can drive agent work - download an agentic model (Ornith, Qwen 3.6, GLM, Nemotron 3.5, a coder) or use an online-capable AI".to_string()
                });
        }
        if share == "local" {
            // Privacy never silently goes online - but a model that cannot
            // truly run here would only produce timeouts, so refuse plainly.
            if let Some(m) = offline.clone().filter(|_| offline_green) {
                return Ok(RouteResult {
                    think: None,
                    model: m,
                    reason: "agent work kept on your device (privacy-first)".to_string(),
                });
            }
            return Err(
                "No local model runs comfortably enough for private agent work on this hardware (a fast fit at 8+ tokens/s) - download a smaller agentic model, or allow online for projects".to_string(),
            );
        }
        // The cost-saver setting: whole project sessions stay on the device
        // whenever a capable model runs COMFORTABLY (green fit - the same
        // bar as device workers). A setting, not a default; documented in
        // the routing explainer. When the bar isn't met, fall through to
        // online rather than fail the session.
        if store_pref(app, "routingProjectThrifty").as_deref() == Some("1") {
            if let Some(m) = offline.clone().filter(|_| offline_green) {
                return Ok(RouteResult {
                    think: None,
                    model: m,
                    reason: "project work on your device (your cost-saver setting)".to_string(),
                });
            }
            log::info!("[router] cost-saver set but no green-fit agent model - routing online");
        }
        let t1 = std::time::Instant::now();
        let models_res = crate::flowsta::list_online_models().await;
        log::info!(
            "[router] agent pick timings: offline {}ms, green-check {}ms, online-list {}ms",
            t_offline,
            t_green,
            t1.elapsed().as_millis()
        );
        if let Ok(models) = models_res {
            if let Some(id) = select_online_agent(&models, agent_pref.as_deref()) {
                // The ledger and on-chain provenance must say WHY this model:
                // an accepted overload offer is the user's call, not routing's.
                let reason = if !plan && agent_online_override().as_deref() == Some(id.as_str()) {
                    "agent work on your session pick (accepted after an overloaded model)".to_string()
                } else if let Some(local) = offline
                    .as_deref()
                    .filter(|_| offline_green)
                    .filter(|m| crate::model_caps::agent_caps(m) >= crate::model_caps::online_agent_caps(&id))
                {
                    // Online by default, but an equally capable model runs well
                    // here: say so, so the user can choose local (free, private)
                    // in Settings > Routing. A nudge by information, never a
                    // silent switch.
                    let local = local.trim_end_matches(".gguf");
                    format!("agent work online by default ({local} on your device is as capable - Settings > Routing)")
                } else {
                    "agent work on a stronger online model".to_string()
                };
                return Ok(RouteResult { think: None, model: id, reason });
            }
        }
        return offline
            .map(|m| RouteResult {
                think: None,
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
    let (medical, health_check_ran) = medical_check(app, query, query_vec, MEDICAL_MARGIN).await;
    let task: &str = if medical { "medical" } else { task };

    if mode == "online-offline" && !medical {
        let local_first = share == "local";
        // A user the app knows cannot use online models is never routed
        // there: the banner explains instead.
        let known_not_entitled = store_pref(app, "onlineEntitled").as_deref() == Some("no");
        let hard_pref = || {
            picks.hard.as_deref().or(if matches!(task, "code" | "math" | "reasoning") {
                picks.hard_code.as_deref()
            } else {
                picks.hard_general.as_deref()
            })
        };
        // Size first: a turn no model on this device can hold - even after
        // growing its context - goes to an online model that can. The mode
        // permits it; an attachment only reaches this mode with consent
        // (the frontend routes offline otherwise), so nothing leaves
        // unasked.
        if let Some(need) = turn_tokens {
            let need = (need as u64).saturating_add(REPLY_ROOM_TOKENS as u64);
            if !known_not_entitled && !local_can_hold(app, need).await {
                if let Some(model) = pick_online_holding(need, task, hard_pref()).await {
                    log::info!("[router] ~{need} tokens is more than any model here holds - online");
                    return Ok(RouteResult {
                        think: None,
                        model,
                        reason: "online — too long for any model on this device".to_string(),
                    });
                }
            }
        }
        // Follow-up: a short anaphoric turn on the same topic inherits the
        // previous turn's side and task (the mode is the consent; the
        // previous decision was made under the same dial). A turn with a
        // live-web cue of its own is not a follow-up.
        if let Some(r) = inherit_followup(app, query, query_vec, task, lean, turn_tokens, prev, &picks).await {
            return Ok(r);
        }
        // Fresh: Stage 0 keyword cues, Stage 1 bge-small similarity to
        // "needs-current-info" phrases. Local-first makes the dial mean
        // what it says: a keyword cue alone doesn't clear the bar - it must
        // also read as fresh at the balanced threshold (an evergreen question
        // with the word "latest" in it stays home); a cue-less question
        // needs the strict one.
        // A keyword cue is confirmed against the anchors: the turn must sit at
        // least as close to the fresh references as to an ordinary question
        // (a greeting with "today" in it fails this), and under Local-first
        // it must also read as fresh at the balanced threshold. With no
        // embeddings the cue alone counts on every dial: the cue IS the
        // explicit live-web ask, and a Local-first user without the memory
        // model would otherwise never reach the live web at all (the Air
        // matrix, 2026-09-03).
        let cue_confirmed = if looks_time_sensitive(query) {
            match fresh_scores(app, query, query_vec).await {
                Some((f, b)) => f >= b && (!local_first || f >= threshold_for("balanced")),
                None => true,
            }
        } else {
            false
        };
        let fresh_why = if cue_confirmed {
            Some("looks like it needs current info")
        } else if semantic_fresh_score(app, query, query_vec)
            .await
            .is_some_and(|s| s >= threshold_for(share))
        {
            Some("seems to need up-to-date info")
        } else {
            None
        };
        if let Some(why) = fresh_why {
            if let Some(model) = pick_online(task, true, picks.fresh.as_deref()).await {
                return Ok(RouteResult { think: None, model, reason: format!("online — {why}") });
            }
            // No online model available → fall through.
        }
        // The promise before the default: a question the health gate could
        // not fully check (no embedding model, embed server down) is not
        // sent out on capability grounds - the device answers and the
        // receipt says why. Live-web keyword cues still go out (as before).
        if !local_first && !known_not_entitled && !health_check_ran {
            let model = pick_offline_for(app, task, lean, false, turn_tokens).await?;
            log::warn!("[router] health check could not run - keeping the turn on the device");
            return Ok(RouteResult {
                think: None,
                model,
                reason: "kept on your device — the health check could not run (memory component)".to_string(),
            });
        }
        // Hard: a known-hard question goes to the Hard slot. Never in
        // local-first (a weaker local answer beats sending it out), and
        // never on an unknown verdict.
        if difficulty == "hard" && !local_first && !known_not_entitled {
            if let Some(model) = pick_online(task, false, hard_pref()).await {
                return Ok(RouteResult {
                    think: None,
                    model,
                    reason: "online — a hard question, using a stronger model".to_string(),
                });
            }
        }
        // Frontier by default: an ordinary question goes to the Everyday
        // online model, unless the device is as good for it - the model
        // that will answer here runs at full speed, scores within the
        // dial's margin of the Everyday model on this task, and the
        // question is known easy. Unknown difficulty (no helper model)
        // goes online: the safe direction for quality.
        if !local_first && !known_not_entitled {
            match pick_online_everyday(picks.everyday.as_deref()).await {
                Some((everyday, ecaps)) => {
                    let local = pick_offline_detail(app, task, lean, false, turn_tokens).await.ok();
                    let adj = learned_adjustments(app);
                    let as_good = local.as_ref().is_some_and(|l| {
                        l.fast && difficulty == "easy" && local_wins(share, shifted(l.cap, adj.margin), ecaps.by_task(task))
                    });
                    if let Some(l) = &local {
                        log::info!(
                            "[router] as-good check ({share}, {task}, {difficulty}): {} fast={} cap={} vs {} cap={} -> {}",
                            l.name, l.fast, l.cap, everyday, ecaps.by_task(task), if as_good { "device" } else { "online" }
                        );
                    } else {
                        log::info!("[router] as-good check: no local model runs here -> online");
                    }
                    if let Some(l) = local.filter(|_| as_good) {
                        return Ok(RouteResult {
                            think: None,
                            model: l.name,
                            reason: "kept on your device — as good for this question".to_string(),
                        });
                    }
                    return Ok(RouteResult { think: None, model: everyday, reason: "online — everyday model".to_string() });
                }
                None => {
                    // Catalog unreachable (offline, proxy down): the device
                    // answers, and the receipt says why.
                    let model = pick_offline_for(app, task, lean, false, turn_tokens).await?;
                    return Ok(RouteResult {
                        think: None,
                        model,
                        reason: "offline — online models unavailable right now".to_string(),
                    });
                }
            }
        }
    }
    // "My hardware": the local pick may hand off to the user's connected
    // server when the scan shows a clearly stronger (or, on the speed lean,
    // faster) RECOGNIZED model there. Never the online proxy in this mode;
    // an unreachable server falls through to local at request time.
    if mode == "my-hardware" {
        let (ext_models, ext_tps) = crate::engine::external_models_cached(app);
        if !ext_models.is_empty() {
            let local = pick_offline_for(app, task, lean, false, turn_tokens).await.ok();
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
                        think: None,
                        model: format!("external:{}", ext_id),
                        reason: why.to_string(),
                    });
                }
                log::warn!("[Router] external engine unreachable — using the local pick");
            }
            if let Some(local) = local {
                return Ok(RouteResult { think: None, model: local, reason: "offline".to_string() });
            }
        }
        // No server connected (or nothing usable) — behave like offline-only.
    }

    // Offline Only: a follow-up stays with the model that answered (a
    // coding thread keeps the coder even when the hint reads "general").
    if mode == "offline" && !medical {
        if let Some(r) = inherit_followup(app, query, query_vec, task, lean, turn_tokens, prev, &picks).await {
            return Ok(r);
        }
    }
    let model = pick_offline_for(app, task, lean, false, turn_tokens).await?;
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
        think: None,
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
        // Any edge on the task counts: the classifier's verdict can change
        // the pick whenever the task-best is not the overall-best.
        if task_best.name != overall_best.name && tb > ob.by_task(task) {
            out.push(task.to_string());
        }
    }
    Ok(out)
}

/// The router's recommended online model per slot - Settings names them
/// from here, so the labels can never drift from the defaults again.
#[tauri::command]
pub fn routing_defaults() -> std::collections::HashMap<String, String> {
    [
        ("fresh", DEFAULT_FRESH),
        ("everyday", DEFAULT_EVERYDAY),
        ("hard_code", DEFAULT_HARD_CODE),
        ("hard_general", DEFAULT_HARD_GENERAL),
        ("agent", DEFAULT_AGENT),
        ("plan", DEFAULT_PLAN),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
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
    online_everyday: Option<String>,
    online_hard: Option<String>,
    online_hard_code: Option<String>,
    online_hard_general: Option<String>,
    online_agent: Option<String>,
    online_planning: Option<String>,
    query_vec: Option<Vec<f32>>,
    agent: Option<bool>,
    plan: Option<bool>,
    turn_tokens: Option<u32>,
    // The dial: frontier | balanced | local (legacy eagerness accepted).
    online_share: Option<String>,
    prev_side: Option<String>,
    prev_task: Option<String>,
    prev_model: Option<String>,
    prev_vec: Option<Vec<f32>>,
) -> Result<RouteResult, String> {
    let prev = PrevTurn { side: prev_side, task: prev_task, model: prev_model, vec: prev_vec };
    let picks = OnlinePicks {
        fresh: online_fresh,
        everyday: online_everyday,
        hard: online_hard,
        hard_code: online_hard_code,
        hard_general: online_hard_general,
        agent: online_agent,
        plan: online_planning,
    };
    let share = online_share.or(eagerness).unwrap_or_default();
    route_with(
        &app,
        &mode,
        &query,
        &share,
        task.as_deref().unwrap_or("general"),
        difficulty.as_deref().unwrap_or("unknown"),
        lean.as_deref().unwrap_or(""),
        &picks,
        query_vec.as_deref(),
        agent.unwrap_or(false),
        plan.unwrap_or(false),
        turn_tokens,
        &prev,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pm(id: &str, name: &str, desc: &str, input: f64, output: f64, search: Option<f64>) -> crate::flowsta::OnlineModel {
        crate::flowsta::OnlineModel {
            id: id.to_string(),
            display_name: name.to_string(),
            description: desc.to_string(),
            context_window: 128_000,
            vision: false,
            category: "chat".to_string(),
            categories: vec!["chat".to_string()],
            released: None,
            pricing: Some(crate::flowsta::OnlinePricing {
                input_per_mtok: input,
                output_per_mtok: output,
                request_fee_usd: 0.0,
                search_per_call_usd: search,
                cached_input_per_mtok: None,
            }),
        }
    }

    #[test]
    fn dial_names_map_and_unknown_is_empty() {
        assert_eq!(normalize_share("frontier"), "frontier");
        assert_eq!(normalize_share("freshness"), "frontier");
        assert_eq!(normalize_share("Local"), "local");
        assert_eq!(normalize_share("privacy"), "local");
        assert_eq!(normalize_share("balanced"), "balanced");
        assert_eq!(normalize_share(""), "");
        assert_eq!(normalize_share("whatever"), "");
    }

    #[test]
    fn dial_thresholds_and_margins() {
        assert!(threshold_for("local") > threshold_for("balanced"));
        assert!(threshold_for("balanced") > threshold_for("frontier"));
        // Frontier: the device must be genuinely better; balanced: as good.
        assert!(local_wins("frontier", 9, 8));
        assert!(!local_wins("frontier", 8, 8));
        assert!(local_wins("balanced", 8, 8));
        assert!(!local_wins("balanced", 7, 8));
        assert!(!local_wins("local", 9, 8));
    }

    #[test]
    fn everyday_prefers_pick_then_default_then_cheapest_chat() {
        let luna = pm("online:gpt-5.6-luna", "GPT-5.6 Luna", "fast, low-cost", 0.2, 1.2, None);
        let terra = pm("online:gpt-5.6-terra", "GPT-5.6 Terra", "flagship", 2.0, 12.0, None);
        let flash = pm("online:deepseek-v4-flash", "DeepSeek V4 Flash", "cheap", 0.44, 1.32, None);
        let search = pm("online:sonar", "Perplexity Sonar", "live web", 0.1, 0.1, Some(0.005));
        // The user's pick wins.
        let all = vec![terra.clone(), luna.clone(), flash.clone(), search.clone()];
        assert_eq!(select_online_everyday(&all, Some("gpt-5.6-terra")).map(|m| m.id), Some(terra.id.clone()));
        // Then the recommended default.
        assert_eq!(select_online_everyday(&all, None).map(|m| m.id), Some(luna.id.clone()));
        // Default absent: the cheapest priced chat model, never a search model.
        let no_luna = vec![terra.clone(), flash.clone(), search.clone()];
        assert_eq!(select_online_everyday(&no_luna, None).map(|m| m.id), Some(flash.id.clone()));
        // Only search models: nothing (the device answers).
        assert!(select_online_everyday(&[search], None).is_none());
    }

    #[test]
    fn fresh_pick_must_be_able_to_search() {
        let luna = pm("online:gpt-5.6-luna", "GPT-5.6 Luna", "fast, low-cost", 0.2, 1.2, None);
        let grok = pm("online:grok-4.6-search", "Grok 4.6 (Web)", "live web search", 2.0, 6.0, Some(0.005));
        let all = vec![luna.clone(), grok.clone()];
        // A chat-only pick is not honored for a fresh turn ...
        assert_eq!(select_online(&all, "general", true, Some("gpt-5.6-luna")), Some(grok.id.clone()));
        // ... but is for a hard turn.
        assert_eq!(select_online(&all, "general", false, Some("gpt-5.6-luna")), Some(luna.id.clone()));
    }

    #[test]
    fn health_keyword_stage_catches_clinical_terms_not_software() {
        assert!(looks_medical("what does elevated ALT on a liver panel mean"));
        assert!(looks_medical("my thyroid came back high, is that bad"));
        assert!(looks_medical("what are the common side effects of metformin"));
        assert!(looks_medical("I was diagnosed with prediabetes, what diet changes help"));
        assert!(!looks_medical("diagnose why my rust build fails"));
        assert!(!looks_medical("the API returns a 500 under load"));
        assert!(!looks_medical("write a poem about a rash decision"));
    }

    #[test]
    fn followup_wording_is_short_and_anaphoric() {
        assert!(is_followup("more on that"));
        assert!(is_followup("why?"));
        assert!(is_followup("and the second one?"));
        assert!(is_followup("shorter please"));
        assert!(is_followup("can you give an example"));
        assert!(!is_followup("what is the capital of France"));
        assert!(!is_followup("write a python function that parses a csv file and returns the rows"));
    }

    #[test]
    fn learning_is_slow_and_bounded() {
        assert_eq!(margin_shift(5, 3), 0, "under 20 verdicts nothing moves");
        assert_eq!(margin_shift(18, 4), 1, "device redos dominate -> local deserves a point");
        assert_eq!(margin_shift(4, 18), -1, "try-online dominates -> local loses a point");
        assert_eq!(margin_shift(11, 10), 0, "a near tie moves nothing");
        assert_eq!(family_shift(9, 20), 0);
        assert_eq!(family_shift(10, 20), -1);
        assert_eq!(family_shift(10, 100), 0, "a tenth of the answers is not a pattern");
        assert_eq!(family_key("Qwen3.5-4B-Opus-Distilled-Q4_K_M.gguf"), "qwen3.5-4b");
        assert_eq!(family_key("online:gpt-5.6-luna"), "gpt-5.6-luna");
        assert_eq!(shifted(8, -1), 7);
        assert_eq!(shifted(0, -1), 0);
    }

    #[test]
    fn think_verdict_follows_difficulty_and_capability() {
        assert_eq!(think_for("hard", true, false), Some(true));
        assert_eq!(think_for("easy", true, false), Some(false));
        assert_eq!(think_for("unknown", true, false), None);
        assert_eq!(think_for("hard", false, false), None);
        assert_eq!(think_for("hard", true, true), None);
    }

    #[test]
    fn everyday_tier_scores_under_the_flagship() {
        let flagship = crate::model_caps::online_caps_for("online:gpt-5.6-terra gpt-5.6 terra flagship");
        let luna = crate::model_caps::online_caps_for("online:gpt-5.6-luna gpt-5.6 luna fast low-cost");
        let kimi = crate::model_caps::online_caps_for("online:kimi-k2.6 kimi k2.6 standout value");
        assert!(luna.overall < flagship.overall);
        assert_eq!(luna.overall, 8);
        assert_eq!(kimi.overall, 8);
        // "gemini" must not read as "-mini".
        assert_eq!(crate::model_caps::online_caps_for("online:gemini-3 gemini").overall, 8);
    }
    #[test]
    fn agent_rank_prefers_tool_tier_then_task() {
        // Ornith (9) beats a coder (8) even when the coder's task score is higher.
        assert!(super::agent_rank_cap(9, 7) > super::agent_rank_cap(8, 9));
        // Same tool tier: the task score decides.
        assert!(super::agent_rank_cap(8, 8) > super::agent_rank_cap(8, 6));
    }

    #[test]
    fn agent_speed_floor() {
        assert!(super::agent_speed_ok(None), "unmeasured: the fit decides");
        assert!(super::agent_speed_ok(Some(30.0)));
        assert!(!super::agent_speed_ok(Some(5.0)), "a measured crawl is not agent-ready");
    }


    fn om(id: &str, name: &str, desc: &str, search_fee: Option<f64>) -> crate::flowsta::OnlineModel {
        crate::flowsta::OnlineModel {
            id: format!("online:{id}"),
            display_name: name.to_string(),
            description: desc.to_string(),
            context_window: 0,
            vision: false,
            category: "chat".to_string(),
            categories: vec!["chat".to_string()],
            released: None,
            pricing: Some(crate::flowsta::OnlinePricing {
                input_per_mtok: 1.0,
                output_per_mtok: 1.0,
                request_fee_usd: 0.0,
                search_per_call_usd: search_fee,
                cached_input_per_mtok: None,
            }),
        }
    }

    fn catalog() -> Vec<crate::flowsta::OnlineModel> {
        vec![
            om("grok-4.6", "Grok 4.6", "xAI's frontier model", None),
            om("grok-4.6-search", "Grok 4.6 (Web)", "live web search", Some(0.005)),
            om("gpt-5.6-sol", "GPT-5.6 Sol", "OpenAI's flagship", None),
            om("gpt-5.6-terra", "GPT-5.6 Terra", "balanced flagship", None),
            om("sonar", "Sonar", "Perplexity search", Some(0.005)),
        ]
    }

    #[test]
    fn keep_loaded_is_fit_aware_with_a_project_guard() {
        // Green loaded, green best, near-equal caps: classic stickiness holds.
        assert!(keep_loaded(8, 8, 2, 2, false, None));
        // Yellow loaded vs green best, no folder open: switch - the
        // sky-is-blue case (a slow partial-offload must not hold the slot).
        assert!(!keep_loaded(8, 8, 1, 2, false, None));
        // Same, but a project folder is open: the project's model stays warm.
        assert!(keep_loaded(8, 8, 1, 2, true, None));
        // A real specialist (beats the margin: gap of 3+) evicts regardless
        // of fit or folders...
        assert!(!keep_loaded(6, 9, 2, 2, true, None));
        // ...but a 2-point gap is inside the margin - stickiness holds,
        // matching the shipped inclusive-margin semantics.
        assert!(keep_loaded(6, 8, 2, 2, false, None));
        // Yellow loaded vs yellow best: no fit win to be had - stay.
        assert!(keep_loaded(8, 8, 1, 1, false, None));
        // Loaded model BETTER fit than best candidate: stay.
        assert!(keep_loaded(7, 8, 2, 1, false, None));
    }

    /// A slow-loading candidate must win by more: 6 -> 9 (margin 3) switches
    /// when the candidate loads fast, not when it takes a minute.
    #[test]
    fn slow_loads_need_a_bigger_margin() {
        assert!(!keep_loaded(6, 9, 2, 2, true, Some(3.0)), "fast load: +3 is worth a reload");
        assert!(keep_loaded(6, 9, 2, 2, true, Some(45.0)), "a 45 s reload needs more than +3");
        assert!(!keep_loaded(4, 9, 2, 2, true, Some(45.0)), "+5 still wins against a 45 s reload");
        assert_eq!(reload_margin(None), 0);
        assert_eq!(reload_margin(Some(12.0)), 1);
        assert_eq!(reload_margin(Some(60.0)), 2);
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

    /// Today's regression, immortalized: catalog ids are prefixed, prefs
    /// and defaults arrive in either shape - every combination must match.
    #[test]
    fn agent_selection_matches_every_id_shape() {
        let mut models = catalog();
        models.push(om("kimi-k2.6", "Kimi K2.6", "value tool-driver", None));
        models.push(om("kimi-k3", "Kimi K3", "flagship", None));
        // No pref -> the Sol default, never the alphabetical fallback.
        assert_eq!(select_online_agent(&models, None).unwrap(), "online:gpt-5.6-sol");
        // Raw pref matches prefixed ids.
        assert_eq!(
            select_online_agent(&models, Some("kimi-k2.6")).unwrap(),
            "online:kimi-k2.6"
        );
        // Prefixed pref matches too.
        assert_eq!(
            select_online_agent(&models, Some("online:kimi-k2.6")).unwrap(),
            "online:kimi-k2.6"
        );
        // Unknown pref falls back to the default, not to silence.
        assert_eq!(
            select_online_agent(&models, Some("online:claude-nope")).unwrap(),
            "online:gpt-5.6-sol"
        );
        // Default missing from the catalog -> capability fallback, which
        // must never hand agents a tools-blind model (sonar scores 1).
        let no_sol: Vec<_> = models
            .iter()
            .filter(|m| !m.id.contains("sol"))
            .cloned()
            .collect();
        let fb = select_online_agent(&no_sol, None).unwrap();
        assert!(!fb.contains("sonar"), "agents must never get a search-only model, got {fb}");
    }

    /// Balanced lean: a comfortable (green) model beats a smarter one that
    /// barely loads - the ordering that chose a timing-out 4B over a green
    /// small model on 4GB hardware.
    #[test]
    fn balanced_ordering_puts_fit_before_capability() {
        use std::cmp::Ordering;
        let green_modest = OfflineRank { cap: 5, tier: 2, params_b: 2.0, tps: None };
        let yellow_smart = OfflineRank { cap: 8, tier: 1, params_b: 4.0, tps: None };
        assert_eq!(
            offline_ordering("balanced", green_modest, yellow_smart),
            Ordering::Greater,
            "balanced must prefer the green fit"
        );
        // Quality lean remains the explicit capability-chaser.
        assert_eq!(
            offline_ordering("quality", green_modest, yellow_smart),
            Ordering::Less,
            "quality lean still chases capability"
        );
        // Within a tier, capability decides on balanced.
        let green_smart = OfflineRank { cap: 8, tier: 2, params_b: 4.0, tps: None };
        assert_eq!(
            offline_ordering("balanced", green_smart, green_modest),
            Ordering::Greater
        );
    }

    #[test]
    fn select_online_defaults_per_slot() {
        let models = catalog();
        // Fresh → the web-search default.
        assert_eq!(select_online(&models, "general", true, None).unwrap(), "online:grok-4.6-search");
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
    fn select_agent_default_is_sol() {
        // The flagship drives projects by default (Eric's call, 2026-08-06;
        // measured fast on simple tool steps - it scales thinking to need).
        assert_eq!(
            select_online_agent(&agent_catalog(), None).unwrap(),
            "online:gpt-5.6-sol"
        );
    }

    #[test]
    fn select_agent_pref_wins() {
        // An explicit pick beats the default, in either id shape.
        assert_eq!(
            select_online_agent(&agent_catalog(), Some("online:kimi-k2.6")).unwrap(),
            "online:kimi-k2.6"
        );
        // A pref no longer in the catalog is ignored → default applies.
        assert_eq!(
            select_online_agent(&agent_catalog(), Some("online:retired")).unwrap(),
            "online:gpt-5.6-sol"
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
        OfflineRank { cap, tier, params_b, tps: None }
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
        // Balanced now sides with the comfortable fit too (partial offload
        // picked by capability produced load timeouts on small hardware);
        // quality remains the explicit capability-chaser.
        assert_eq!(best("balanced", green_small, yellow_big).params_b, 8.0);
        assert_eq!(best("quality", green_small, yellow_big).params_b, 24.0);
    }

    /// Measured speed on this machine outranks the size proxy: a split 35B
    /// MoE at 30 tok/s beats a dense 9B at 22 under the speed lean; without
    /// measurements the old order (tier, cap, smaller) holds.
    #[test]
    fn speed_lean_ranks_by_measured_tps_when_known() {
        let moe = OfflineRank { cap: 8, tier: 2, params_b: 35.0, tps: Some(30.0) };
        let dense = OfflineRank { cap: 7, tier: 2, params_b: 9.0, tps: Some(22.0) };
        assert_eq!(offline_ordering("speed", moe, dense), std::cmp::Ordering::Greater);
        // No measurement on either side and equal capability: the size
        // proxy decides (smaller = faster), as before.
        let unknown_big = OfflineRank { cap: 7, tier: 2, params_b: 35.0, tps: None };
        let unknown_small = OfflineRank { cap: 7, tier: 2, params_b: 9.0, tps: None };
        assert_eq!(offline_ordering("speed", unknown_big, unknown_small), std::cmp::Ordering::Less, "no measurement: smaller wins");
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
    fn capability_dominates_only_for_quality() {
        let strong_red_risk = c(9, 1, 30.0);
        let weak_green = c(4, 2, 3.0);
        // Balanced takes what runs comfortably; quality chases the brain.
        assert_eq!(best("balanced", strong_red_risk, weak_green).cap, 4);
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
