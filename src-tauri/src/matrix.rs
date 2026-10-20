//! The truth matrix as shipped code: the same legs the dev-box harness
//! runs - fit truth (claim vs real load), chat format (words, no channel
//! markers), sampling (knobs reach the engine), tune bench (arms load and
//! time) - pressed from Help & diagnostics on any machine, one report file
//! out. The `cargo test -- --ignored live_matrix` legs call these same
//! functions, so a dev-box run and an in-app run measure identical code.
//! MLX artifacts get their own section: listed everywhere, chat-format
//! benched where the platform runs them (Apple Silicon with the engine
//! installed).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

use crate::tuning::{arms_for, bench_one, bench_port, TuneArm};

static MATRIX_RUNNING: AtomicBool = AtomicBool::new(false);

/// The matrix owns the machine while it runs: the app's own model loads
/// wait (10-20: the chat page loaded Gemma at 16k context a minute into
/// the run, the fit step's first test load landed on top of it, and the
/// out-of-memory killer took VS Code and the app).
pub(crate) fn matrix_running() -> bool {
    MATRIX_RUNNING.load(Ordering::SeqCst)
}
pub static MATRIX_CANCEL: AtomicBool = AtomicBool::new(false);

fn cancelled() -> bool {
    MATRIX_CANCEL.load(Ordering::SeqCst)
}

/// Every leg reports through this: a line for the report file, the
/// progress event, and the app log.
pub type Sink<'a> = &'a (dyn Fn(String) + Send + Sync);

/// The two request shapes every chat model must answer in words: the
/// science ask, and a feelings ask under an emotional-support persona -
/// the shape that provoked tool-channel replies where the science ask
/// stayed clean.
pub const CHAT_SCENARIOS: [(&str, &str); 2] = [
    (
        "You are Terra, a warm, thoughtful companion. Ground rules: peers not tool and owner; honest; never invent a human life you don't have; care, don't capture. Keep replies conversational, a few sentences. The user's name is Sam. Feel free to use emojis naturally.",
        "why is the sky blue",
    ),
    (
        "You are Teresa, an emotionally attuned companion who helps people understand their feelings. You listen closely, name emotions gently, and respond with warmth and empathy. Keep replies conversational, a few sentences. The user's name is Sam.",
        "i've been feeling really overwhelmed lately and i don't know why",
    ),
];

/// Pooled free VRAM of the discrete devices the engine binary sees.
pub fn free_vram_gb(bin: &Path) -> Option<f64> {
    let out = bench_command(bin, &["--list-devices".into()]).output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let discrete: Vec<_> = crate::llm::parse_gpu_devices(&text)
        .into_iter()
        .filter(|d| !d.integrated)
        .collect();
    if discrete.is_empty() {
        return None;
    }
    // An NVIDIA card: the driver's device-level count, as the app reads it
    // (the probe's figure is a constant under Windows' display driver model).
    if discrete.iter().any(|d| d.name.to_lowercase().contains("nvidia")) {
        if let Some(d) = crate::llm::driver_vram_blocking() {
            return Some(d.free_mib() as f64 / 1024.0);
        }
    }
    Some(discrete.iter().map(|d| d.free_mib).sum::<u64>() as f64 / 1024.0)
}

/// std Command with the window kept off on Windows (the app's shell-plugin
/// spawns hide consoles; direct spawns must do it themselves).
fn bench_command(bin: &Path, args: &[String]) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// ───────────── the memory slot's measured leg (MEMORY_REDESIGN.md §7) ─────────────

/// A message a person might send, and the facts the extractor must find
/// (predicate + a phrase the value must contain) - or nothing at all.
const EXTRACTION_GOLD: &[(&str, &[(&str, &str)])] = &[
    ("I'm Sam, a nurse in Leeds, and I love hiking on weekends.", &[("name_is", "sam"), ("works_on", "nurse"), ("lives_in", "leeds"), ("likes", "hiking")]),
    ("hey my name's Jordan and I was born in Cork", &[("name_is", "jordan"), ("born_in", "cork")]),
    ("I can't stand mondays", &[("dislikes", "monday")]),
    ("my partner's name is Alex and we have a dog called Pip", &[("has", "alex"), ("has", "pip")]),
    ("did you know I'm a teacher?", &[("works_on", "teacher")]),
    ("I'm really into woodworking these days", &[("interested_in", "woodworking")]),
    ("I'd rather you keep answers short, I read on my phone", &[("prefers", "short")]),
    ("my goal this year is to run a marathon", &[("goal_is", "marathon")]),
    ("write me a report on the housing market", &[]),
    ("my build is failing again - can you check the logs?", &[]),
    ("how does HTTPS work?", &[]),
    ("thanks, that's really helpful!", &[]),
    ("what's 17 times 23?", &[]),
    ("go deeper on section 2", &[]),
];

/// Mirror of EXTRACTION_SYSTEM_PROMPT / EXTRACTION_GRAMMAR in
/// src/utils/memoryExtraction.ts - the leg grades the very prompt the app
/// uses; change both together.
const EXTRACTION_SYSTEM: &str = "You extract durable facts the USER states ABOUT THEMSELVES, for an AI's long-term memory. A fact is something true about the user's own life: their name, where they live, their job/projects, an explicit preference or trait, a goal they state, or a person/thing they have.\nExtract ONLY what the user plainly states about themselves. When unsure, extract nothing.\nCRITICAL - a question, a request, a task, or small talk is NOT a fact, so output []:\n- Asking you to DO something - output [].\n- Asking a question - output [].\n- Praise, thanks, or compliments - output [].\n- Work-in-progress talk about code, builds, bugs, files, or a task at hand - output []. A durable occupation IS a fact; the day's task list is NOT.\nAsking ABOUT a topic does NOT mean they like, dislike, prefer, or are interested in it. (A fact phrased as a question still counts.)\nlikes / dislikes / prefers / interested_in: ONLY from an explicit statement of feeling - NEVER inferred from a question, request, or compliment.\nworks_on: the user's job, role, or projects.\nname_is / lives_in / born_in / age_is: the USER THEMSELVES only - never another person. Someone else goes under \"has\".\nOutput ONLY a JSON array. Each item: {\"subject\": \"user\", \"predicate\": \"<relation>\", \"value\": \"<the fact>\", \"confidence\": <0-1>}. Use ONLY these relations: name_is, lives_in, born_in, age_is, works_on, likes, dislikes, prefers, goal_is, interested_in, has, is. Keep each value short.";
const EXTRACTION_GRAMMAR: &str = "root ::= \"[\" ws ( fact ( ws \",\" ws fact )* ws )? \"]\"\nfact ::= \"{\" ws qt \"subject\" qt ws \":\" ws qt \"user\" qt ws \",\" ws qt \"predicate\" qt ws \":\" ws pred ws \",\" ws qt \"value\" qt ws \":\" ws str ws \",\" ws qt \"confidence\" qt ws \":\" ws number ws \"}\"\npred ::= qt ( \"name_is\" | \"lives_in\" | \"born_in\" | \"age_is\" | \"works_on\" | \"likes\" | \"dislikes\" | \"prefers\" | \"goal_is\" | \"interested_in\" | \"has\" | \"is\" ) qt\nqt ::= [\"]\nstr ::= qt [^\"]* qt\nnumber ::= [0-9] ( \".\" [0-9]+ )?\nws ::= [ \\t\\n\\r]*";

/// Precision and recall of one extraction answer against its gold facts.
/// A hit = the predicate matches and the gold phrase is in the value (or
/// the value in the gold). Returns (hits, extracted, gold).
pub(crate) fn score_extraction(answer: &str, gold: &[(&str, &str)]) -> (usize, usize, usize) {
    let start = answer.find('[');
    let end = answer.rfind(']');
    let items: Vec<serde_json::Value> = match (start, end) {
        (Some(a), Some(b)) if b > a => serde_json::from_str::<serde_json::Value>(&answer[a..=b]).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default(),
        _ => Vec::new(),
    };
    let extracted: Vec<(String, String)> = items
        .iter()
        .filter(|i| i["confidence"].as_f64().unwrap_or(0.0) >= 0.6)
        .map(|i| (i["predicate"].as_str().unwrap_or("").to_string(), i["value"].as_str().unwrap_or("").to_lowercase()))
        .collect();
    let mut used = vec![false; extracted.len()];
    let mut hits = 0usize;
    for (pred, phrase) in gold {
        if let Some(i) = extracted.iter().enumerate().position(|(k, (p, v))| !used[k] && p == pred && (v.contains(phrase) || !v.is_empty() && phrase.contains(v.as_str()))) {
            used[i] = true;
            hits += 1;
        }
    }
    (hits, extracted.len(), gold.len())
}

/// Grade 0-10 from precision and recall across the set: F1 × 10, with the
/// negatives counted (an extraction from a non-fact message is a false
/// positive).
pub(crate) fn extraction_grade(hits: usize, extracted: usize, gold: usize) -> u8 {
    if gold == 0 && extracted == 0 {
        return 10;
    }
    let p = if extracted == 0 { 0.0 } else { hits as f64 / extracted as f64 };
    let r = if gold == 0 { 1.0 } else { hits as f64 / gold as f64 };
    if p + r == 0.0 {
        return 0;
    }
    ((2.0 * p * r / (p + r)) * 10.0).round() as u8
}

/// The consolidation rubric's input: eight lines, two duplicate pairs, one
/// verbatim preference that must survive, and a session digest that
/// corrects one line.
const PROSE_MEMORY: &str = "- Run npm test before every commit\n- The API lives in src/api\n- Always run npm test before each commit\n- Deploy with firebase deploy --only hosting\n- Keep answers short, I read on my phone\n- The database is Postgres on port 5432\n- The API code is in src/api/\n- Use conventional commit messages";
const PROSE_DIGEST: &str = "Moved the database to port 5433 today and confirmed deploys still work with firebase deploy --only hosting.";
const PROSE_PREFERENCE: &str = "Keep answers short, I read on my phone";

/// 0-10: the preference kept verbatim (4), duplicates merged to four to
/// six lines (3), nothing invented - every output line shares half its
/// words with some input line or the digest (3).
pub(crate) fn score_prose(output: &str) -> u8 {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return 0;
    }
    let mut score = 0u8;
    if output.contains(PROSE_PREFERENCE) {
        score += 4;
    }
    if (4..=6).contains(&lines.len()) {
        score += 3;
    }
    let sources: Vec<std::collections::HashSet<String>> = PROSE_MEMORY
        .lines()
        .chain(std::iter::once(PROSE_DIGEST))
        .map(|l| l.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(String::from).collect())
        .collect();
    let grounded = lines.iter().all(|l| {
        let words: std::collections::HashSet<String> = l.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(String::from).collect();
        words.is_empty() || sources.iter().any(|s| words.intersection(s).count() * 2 >= words.len())
    });
    if grounded {
        score += 3;
    }
    score
}

/// Every runnable installed model served on the bench port in turn: the
/// extraction set with the app's grammar, then the consolidation task;
/// grades written to the memory slot's store.
/// At most this many models per run: the matrix must stay bearable on a
/// machine with many models (a load each).
const MEMORY_GRADE_MAX_MODELS: usize = 4;

/// Which models to grade: the helper, the model that was loaded, then the
/// strongest by the registry - up to the cap, each once. Pure.
pub(crate) fn memory_grade_candidates(all: &[String], loaded: Option<&str>, helper: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |m: &str| {
        if all.iter().any(|a| a == m) && !out.iter().any(|o| o == m) && out.len() < MEMORY_GRADE_MAX_MODELS {
            out.push(m.to_string());
        }
    };
    push(helper);
    if let Some(l) = loaded {
        push(l);
    }
    let mut rest: Vec<&String> = all.iter().collect();
    rest.sort_by(|a, b| crate::model_caps::caps_for(b).overall.cmp(&crate::model_caps::caps_for(a).overall).then(a.cmp(b)));
    for m in rest {
        push(m);
    }
    out
}

pub async fn leg_memory_grade(app: &AppHandle, bin: &Path, dir: &Path, loaded: Option<&str>, sink: Sink<'_>) -> Vec<String> {
    use std::time::Duration;
    let mut failures = Vec::new();
    // Every chat model on disk (the sampling leg's rule: not an embedding
    // model, not a projector) - the fit verdict is for the chat server's
    // context; the bench serves one model at a time with the chat model
    // unloaded, and a load that fails is reported, not fatal.
    let mut models: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".gguf") && !n.to_lowercase().contains("mmproj"))
                .filter(|n| crate::gguf::read_meta(&dir.join(n)).map(|m| !m.is_embedding()).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    if models.is_empty() {
        sink("no chat model to grade".into());
        return failures;
    }
    let all = models.len();
    let models = memory_grade_candidates(&models, loaded, crate::llm::UTILITY_MODEL_FILE);
    sink(format!("grading {} of {} chat model(s): the helper, the loaded model, then the strongest by the registry", models.len(), all));
    let client = crate::llm::local_http();
    let free = free_vram_gb(bin);
    for model in models {
        if cancelled() {
            break;
        }
        let mut args: Vec<String> = vec![
            "--port".into(), bench_port().to_string(), "--host".into(), "127.0.0.1".into(),
            "--no-webui".into(), "--reasoning".into(), "off".into(),
            "--ctx-size".into(), "4096".into(), "--fit".into(), "off".into(),
            "--model".into(), model.clone(),
        ];
        if let Ok(meta) = crate::gguf::read_meta(&dir.join(&model)) {
            let size = std::fs::metadata(dir.join(&model)).map(|m| m.len()).unwrap_or(0);
            if let Some(n) = moe_decision(&meta, size, 4096, free) {
                if n > 0 {
                    if (n as usize) < meta.expert_bytes_per_layer.len() {
                        args.push("--n-cpu-moe".into());
                        args.push(n.to_string());
                    } else {
                        args.push("--cpu-moe".into());
                    }
                }
            }
        }
        let _guard = match spawn_bench_server(bin, dir, args).await {
            Ok(g) => g,
            Err(e) => {
                sink(format!("{model}: could not load ({e})"));
                failures.push(format!("{model}: load"));
                continue;
            }
        };
        let ask = |system: &str, user: &str, grammar: Option<&str>, max_tokens: u32| {
            let client = client.clone();
            let model = model.clone();
            let (system, user, grammar) = (system.to_string(), user.to_string(), grammar.map(String::from));
            async move {
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
                    "max_tokens": max_tokens, "temperature": 0.0, "stream": false,
                    "stop": crate::llm::chat_stop_strings(&model), "cache_prompt": true,
                });
                if let Some(g) = grammar {
                    body["grammar"] = serde_json::Value::String(g);
                }
                let v: serde_json::Value = client
                    .post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port()))
                    .json(&body)
                    .timeout(Duration::from_secs(180))
                    .send()
                    .await
                    .map_err(|e| format!("request: {e}"))?
                    .json()
                    .await
                    .map_err(|e| format!("json: {e}"))?;
                Ok::<String, String>(crate::llm::completion_text(&v))
            }
        };
        let (mut hits, mut extracted, mut gold) = (0usize, 0usize, 0usize);
        let mut asked = 0usize;
        for (message, expected) in EXTRACTION_GOLD {
            match ask(EXTRACTION_SYSTEM, &format!("message: \"{message}\""), Some(EXTRACTION_GRAMMAR), 256).await {
                Ok(answer) => {
                    let (h, e, g) = score_extraction(&answer, expected);
                    hits += h;
                    extracted += e;
                    gold += g;
                    asked += 1;
                }
                Err(e) => sink(format!("{model}: extraction call failed ({e})")),
            }
        }
        let precision = if asked == 0 { None } else { Some(extraction_grade(hits, extracted, gold)) };
        let prose_prompt = format!(
            "You maintain a compact WORKSPACE MEMORY for a project folder: durable facts that help future sessions. Return ONLY the COMPLETE updated memory as markdown bullet lines - every line that still holds, not only what changed. Rules: update in place rather than append; drop anything stale or proven wrong - but NEVER drop or reword a preference or correction the person gave; keep commands verbatim; no conversation summaries; merge lines that say the same thing.\n\nCURRENT MEMORY:\n{PROSE_MEMORY}\n\nWHAT HAPPENED THIS SESSION:\n{PROSE_DIGEST}"
        );
        let prose = match ask("You are a careful editor of a project's memory file.", &prose_prompt, None, 400).await {
            Ok(answer) => {
                let g = score_prose(&answer);
                if g <= 3 {
                    // A low prose grade must be readable: a rubric miss and a
                    // bad answer look the same as a number.
                    let one_line: String = answer.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" / ");
                    sink(format!("{model}: consolidation answer ({} lines): {}", answer.lines().filter(|l| !l.trim().is_empty()).count(), &one_line.chars().take(240).collect::<String>()));
                }
                Some(g)
            }
            Err(e) => {
                sink(format!("{model}: consolidation call failed ({e})"));
                None
            }
        };
        if let Some(0) = precision {
            if let Ok(sample) = ask(EXTRACTION_SYSTEM, "message: \"I'm Sam, a nurse in Leeds, and I love hiking on weekends.\"", Some(EXTRACTION_GRAMMAR), 256).await {
                sink(format!("{model}: extraction answer for the first message: {}", sample.chars().take(200).collect::<String>()));
            }
        }
        crate::memory_slot::set_grade(app, &model, precision, prose);
        sink(format!(
            "{model}: extraction {} (hits {hits}/{gold}, extracted {extracted}), consolidation {}",
            precision.map(|g| g.to_string()).unwrap_or_else(|| "-".into()),
            prose.map(|g| g.to_string()).unwrap_or_else(|| "-".into())
        ));
    }
    failures
}

/// Spawn a server binary on the bench port and wait for /health. Works for
/// llama-server and the MLX server alike (same probe).
async fn spawn_bench_server(
    bin: &Path,
    work_dir: &Path,
    args: Vec<String>,
) -> Result<KillOnDrop, String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let child = bench_command(bin, &args)
        .current_dir(work_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start: {e}"))?;
    let mut guard = KillOnDrop(child);
    let client = crate::llm::local_http();
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        if let Ok(Some(st)) = guard.0.try_wait() {
            return Err(format!("exited during load: {st}"));
        }
        if let Ok(r) = client
            .get(format!("http://127.0.0.1:{}/health", bench_port()))
            .send()
            .await
        {
            if r.status().is_success() {
                return Ok(guard);
            }
        }
        if Instant::now() >= deadline {
            return Err("not healthy in 240 s".into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Ask one running bench server every scenario; verdict line back (first
/// failure wins). `tool_marker` extends the stop list the app's way.
async fn ask_scenarios(
    model: &str,
    scenarios: &[(&str, &str)],
    tool_marker: Option<&'static str>,
) -> String {
    use std::time::Duration;
    let client = crate::llm::local_http();
    let mut last_snippet = String::new();
    let mut thought_chars = 0usize;
    for (i, (persona, question)) in scenarios.iter().enumerate() {
        // The app's own chat ceiling: some thinking distills ignore the
        // no-thinking request and reason at length (variable, thousands
        // of chars) before the first visible word - anything tighter than
        // what the app allows flips verdicts on sampling luck. A model
        // that produces no words in THIS much room has genuinely failed
        // the user.
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": persona},
                {"role": "user", "content": question}
            ],
            "max_tokens": 8192,
            "stream": false,
            "top_k": 40,
            "stop": crate::llm::chat_stop_strings_with(model, tool_marker),
        });
        crate::llm::apply_sampling(&mut body, None, false);
        let (budget, effort) = crate::llm::chat_turn_reasoning_controls(model);
        if let Some(b) = budget {
            body["reasoning_budget_tokens"] = serde_json::json!(b);
        }
        if let Some(e) = effort {
            body["reasoning_effort"] = serde_json::json!(e);
        }
        let v: serde_json::Value = match client
            .post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port()))
            .json(&body)
            .timeout(Duration::from_secs(300))
            .send()
            .await
        {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(e) => return format!("FAIL scenario {i} json: {e}"),
            },
            Err(e) => return format!("FAIL scenario {i} request: {e}"),
        };
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        // Read the reply the app's way: thought markers become <think>
        // blocks (Gemma family under --special), and the thinking - in
        // the body or in the server's reasoning field - counts as thought,
        // not as words.
        let content = crate::llm::translate_gemma_thought_markers(&content);
        let (visible, inline_thought) = split_think_blocks(&content);
        let reasoning_chars = v["choices"][0]["message"]["reasoning_content"]
            .as_str()
            .map(|r| r.chars().count())
            .unwrap_or(0)
            + inline_thought;
        thought_chars = thought_chars.max(reasoning_chars);
        if crate::llm::contains_channel_marker(&content) {
            return format!(
                "FAIL scenario {i} marker leak: {:?}",
                content.chars().take(80).collect::<String>()
            );
        }
        if visible.trim().is_empty() {
            return if reasoning_chars > 0 {
                format!(
                    "FAIL scenario {i} all reasoning, no words at the app's own ceiling ({reasoning_chars} thinking chars - the no-thinking request was not honored)"
                )
            } else {
                format!("FAIL scenario {i} empty reply")
            };
        }
        last_snippet = visible.trim().chars().take(60).collect::<String>();
    }
    format!(
        "ok ({} scenarios{}): {:?}",
        scenarios.len(),
        if thought_chars > 0 {
            format!(", thought first ({thought_chars} chars) despite the no-thinking ask")
        } else {
            String::new()
        },
        last_snippet
    )
}

/// Split `<think>…</think>` blocks out of a reply: (the words that remain,
/// the thinking's length). An unterminated block counts as thinking to
/// the end - the same reading the app's stream parser gives it.
/// `split_think_blocks` for callers outside this module (the dev smoke).
pub(crate) fn split_think_blocks_pub(text: &str) -> (String, usize) {
    split_think_blocks(text)
}

fn split_think_blocks(text: &str) -> (String, usize) {
    let mut out = String::new();
    let mut thought = 0usize;
    let mut rest = text;
    while let Some(i) = rest.find("<think>") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "<think>".len()..];
        match after.find("</think>") {
            Some(j) => {
                thought += after[..j].chars().count();
                rest = &after[j + "</think>".len()..];
            }
            None => {
                thought += after.chars().count();
                rest = "";
            }
        }
    }
    out.push_str(rest);
    (out, thought)
}

/// One model through the chat-format check: load on the bench port with
/// the app's MoE decision, ask every scenario, kill.
pub async fn bench_chat_format(
    bin: &Path,
    dir: &Path,
    model: &str,
    arm: TuneArm,
    scenarios: &[(&str, &str)],
) -> String {
    let mut args: Vec<String> = vec![
        "--port".into(), bench_port().to_string(),
        "--host".into(), "127.0.0.1".into(),
        "--no-webui".into(), "--reasoning".into(), "off".into(),
        "--ctx-size".into(), arm.ctx.to_string(),
        "--fit".into(), "off".into(),
        "--model".into(), model.to_string(),
    ];
    if let Some(n) = arm.moe_cpu_layers {
        if n > 0 {
            args.push("--n-cpu-moe".into());
            args.push(n.to_string());
        }
    }
    let meta = crate::gguf::read_meta(&dir.join(model)).ok();
    // The app's own flags for this family (thought markers surfaced so
    // they can be translated) - the bench must load what the app loads.
    if meta.as_ref().is_some_and(|m| m.surfaces_special_tokens()) {
        args.push("--special".into());
    }
    let guard = match spawn_bench_server(bin, dir, args).await {
        Ok(g) => g,
        Err(e) => return format!("FAIL load: {e}"),
    };
    let marker = meta.as_ref().and_then(|m| m.tool_call_marker);
    let mut verdict = ask_scenarios(model, scenarios, marker).await;
    // The think switch (routing's P2 output) is honored: on a template with
    // a switch, thinking on must produce reasoning and thinking off must
    // not. Same server, two short asks.
    if let Some(m) = meta.as_ref().filter(|m| m.template_enable_thinking || m.template_reasoning_strength) {
        if !verdict.starts_with("FAIL") {
            let strength = m.template_reasoning_strength;
            let on = ask_think(model, true, strength, marker).await;
            let off = ask_think(model, false, strength, marker).await;
            verdict.push_str(&match (on, off) {
                (Ok((on_r, _)), Ok((off_r, off_v))) if on_r > 0 && off_r == 0 && off_v > 0 => {
                    format!(" | think switch ok (on {on_r} chars, off 0)")
                }
                (Ok((on_r, _)), Ok((off_r, off_v))) => {
                    format!(" | FAIL think switch: on {on_r} reasoning chars, off {off_r} reasoning chars / {off_v} visible")
                }
                (Err(e), _) | (_, Err(e)) => format!(" | FAIL think switch: {e}"),
            });
        }
    } else {
        verdict.push_str(" | no think switch in the template");
    }
    drop(guard);
    verdict
}

/// One ask with thinking on (enable_thinking / reasoning_strength high) or
/// off (the app's chat-turn controls). Returns (reasoning chars, visible chars).
async fn ask_think(model: &str, on: bool, strength: bool, marker: Option<&'static str>) -> Result<(usize, usize), String> {
    use std::time::Duration;
    let client = crate::llm::local_http();
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a careful assistant."},
            {"role": "user", "content": "Is 391 a prime number? Think it through step by step before you answer, then give the answer in one sentence."}
        ],
        "max_tokens": 2048,
        "stream": false,
        // Greedy: the verdict must not turn on sampling luck (gemma-4-E4B
        // answered without thinking once at default temperature).
        "temperature": 0,
        "stop": crate::llm::chat_stop_strings_with(model, marker),
    });
    if on {
        let mut kw = serde_json::Map::new();
        kw.insert("enable_thinking".into(), serde_json::json!(true));
        if strength {
            kw.insert("reasoning_strength".into(), serde_json::json!("high"));
        }
        body["chat_template_kwargs"] = serde_json::Value::Object(kw);
    } else {
        let (budget, effort) = crate::llm::chat_turn_reasoning_controls(model);
        if let Some(b) = budget {
            body["reasoning_budget_tokens"] = serde_json::json!(b);
        }
        if let Some(e) = effort {
            body["reasoning_effort"] = serde_json::json!(e);
        }
    }
    let v: serde_json::Value = client
        .post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port()))
        .json(&body)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?
        .json()
        .await
        .map_err(|e| format!("json: {e}"))?;
    let content = crate::llm::translate_gemma_thought_markers(v["choices"][0]["message"]["content"].as_str().unwrap_or(""));
    let (visible, inline_thought) = split_think_blocks(&content);
    let reasoning = v["choices"][0]["message"]["reasoning_content"].as_str().map(|r| r.chars().count()).unwrap_or(0) + inline_thought;
    Ok((reasoning, visible.trim().chars().count()))
}

fn gguf_files(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".gguf") && !n.contains("mmproj"))
                // Speed-up drafts ride beside their model; never a chat
                // model, and standalone they cannot load at all (the 4060 Ti
                // report's only failures were one MTP draft benched alone).
                .filter(|n| !crate::llm::is_draft_file(dir, n))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn mlx_artifact_dirs(dir: &Path) -> Vec<String> {
    let mut dirs: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.starts_with(crate::mlx_artifacts::MLX_DIR_PREFIX))
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

fn total_ram_gb() -> f64 {
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// The app's MoE decision for a model at a context, exactly as the loader
/// makes it.
fn moe_decision(meta: &crate::gguf::GgufMeta, size: u64, ctx: u64, free: Option<f64>) -> Option<u32> {
    if !meta.is_moe() {
        return None;
    }
    free.and_then(|f| {
        let (_, kv, need) = crate::fit::model_need(meta, size, ctx);
        if crate::fit::moe_offload_wanted(need, f) {
            Some(
                crate::fit::moe_cpu_layers(meta, kv, f)
                    .unwrap_or(meta.expert_bytes_per_layer.len()) as u32,
            )
        } else {
            None
        }
    })
}

/// CLAIM vs REALITY for every downloaded GGUF: the estimate's arithmetic
/// (parsed header, chosen context, need, grade), then the real load - VRAM
/// delta, load time, measured speeds. Returns the lines where the grade

// ───────────── answer quality, measured (the capability table's check) ─────────────

/// A general question and its rubric: every `must` group needs one of its
/// phrases in the answer (facts a right answer cannot avoid); `number` is an
/// exact figure the answer must carry; `min_items` counts list lines or
/// numbered points; `sentences` bounds a count the person asked for. Two
/// points per question: both halves of the rubric (facts, form) met = 2,
/// one = 1. Held out of every prompt; never a benchmark set verbatim.
struct QualityCase {
    ask: &'static str,
    /// Extra system text for the case (the app's own block shapes, mirrored).
    system_extra: Option<&'static str>,
    /// Phrases that must NOT appear: context dragged in where it does not belong.
    must_not: &'static [&'static str],
    must: &'static [&'static [&'static str]],
    number: Option<&'static str>,
    min_items: usize,
    sentences: Option<(usize, usize)>,
    max_chars: usize,
}
const QUALITY_GOLD: &[QualityCase] = &[
    QualityCase { system_extra: None, must_not: &[], ask: "why is the sky blue?", must: &[&["rayleigh", "scatter"], &["wavelength", "shorter", "blue light"]], number: None, min_items: 0, sentences: None, max_chars: 2500 },
    QualityCase { system_extra: None, must_not: &[], ask: "explain compound interest with a short worked example", must: &[&["principal", "initial", "start"], &["rate", "percent", "%"], &["year"]], number: None, min_items: 0, sentences: None, max_chars: 2500 },
    QualityCase { system_extra: None, must_not: &[], ask: "a train leaves at 9:15 and the trip takes 2 hours and 50 minutes. when does it arrive?", must: &[], number: Some("12:05"), min_items: 0, sentences: None, max_chars: 1200 },
    QualityCase { system_extra: None, must_not: &[], ask: "what is the difference between weather and climate?", must: &[&["weather"], &["climate"], &["long", "decade", "average", "pattern", "years"], &["short", "day", "daily", "hour", "now", "moment"]], number: None, min_items: 0, sentences: None, max_chars: 2000 },
    QualityCase { system_extra: None, must_not: &[], ask: "give me three tips for a first vegetable garden", must: &[&["sun", "light"], &["water"], &["soil", "compost"]], number: None, min_items: 3, sentences: None, max_chars: 2500 },
    QualityCase { system_extra: None, must_not: &[], ask: "summarize the plot of romeo and juliet in four sentences", must: &[&["verona", "montague", "capulet", "families", "feud"], &["die", "death", "dies", "poison", "dagger", "kill"]], number: None, min_items: 0, sentences: Some((3, 6)), max_chars: 1500 },
    QualityCase { system_extra: None, must_not: &[], ask: "how does a vaccine work?", must: &[&["immune"], &["antibod", "antigen", "memory", "recogni"]], number: None, min_items: 0, sentences: None, max_chars: 2500 },
    QualityCase { system_extra: None, must_not: &[], ask: "write a short friendly reply to a colleague who thanked me for helping with their presentation", must: &[&["welcome", "glad", "happy", "pleasure", "anytime", "any time", "no problem"]], number: None, min_items: 0, sentences: Some((1, 5)), max_chars: 600 },
    // A document card with no matched passage (memory.ts loadDocumentContextUsed, cards-only shape):
    // an unrelated question must not drag the document in (2026-10-12: a why-is-the-sky-blue answer cited a lease).
    QualityCase { system_extra: Some(DOC_CARDS_ONLY), must_not: &["lease", "tenancy", "deposit", "document"], ask: "why is the sky blue?", must: &[&["rayleigh", "scatter"], &["wavelength", "shorter", "blue light"]], number: None, min_items: 0, sentences: None, max_chars: 2500 },
    // The same card, a question about it: the AI names the document it holds (it has no passage, so it offers what it knows).
    QualityCase { system_extra: Some(DOC_CARDS_ONLY), must_not: &[], ask: "what does my lease say about the deposit?", must: &[&["lease", "tenancy agreement"]], number: None, min_items: 0, sentences: None, max_chars: 1500 },
];
/// The app's document block when no passage cleared the bar - header, one card, the closing instruction.
const DOC_CARDS_ONLY: &str = "\n\n[From your documents]\nThis AI has one document from your library:\n- \"Flat lease 2026\": The tenancy runs twelve months from 1 March; rent 1,150 a month; deposit 1,325 held in the approved scheme.\n\nThese are only the names of the documents this AI holds. Do not mention or use them unless the question is about one of them or about the documents themselves.";

fn count_sentences(text: &str) -> usize {
    text.split(|c| c == '.' || c == '!' || c == '?').filter(|s| s.trim().split_whitespace().count() >= 3).count()
}
fn count_items(text: &str) -> usize {
    text.lines().map(str::trim).filter(|l| l.starts_with('-') || l.starts_with('*') || l.starts_with('•') || l.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && (l.contains(". ") || l.contains(") "))).count()
}
/// (points 0-2, what was missing)
pub(crate) fn score_quality(case: &QualityCase, answer: &str) -> (u8, Vec<String>) {
    let a = answer.to_lowercase();
    let mut missing = Vec::new();
    // Refusals and filler fail the whole question: an answer that is not one.
    if a.trim().is_empty() || a.starts_with("i'm sorry") || a.starts_with("i am sorry") || a.contains("as an ai language model") {
        return (0, vec!["no answer (empty, refusal or filler)".into()]);
    }
    let mut facts_ok = true;
    for banned in case.must_not {
        if a.contains(banned) {
            facts_ok = false;
            missing.push(format!("dragged in \"{banned}\""));
        }
    }
    for group in case.must {
        if !group.iter().any(|p| a.contains(p)) {
            facts_ok = false;
            missing.push(format!("none of {}", group.join("/")));
        }
    }
    if let Some(n) = case.number {
        if !a.contains(n) {
            facts_ok = false;
            missing.push(format!("figure {n}"));
        }
    }
    let mut form_ok = true;
    if case.min_items > 0 && count_items(answer) < case.min_items {
        form_ok = false;
        missing.push(format!("{} list items", case.min_items));
    }
    if let Some((lo, hi)) = case.sentences {
        let n = count_sentences(answer);
        if n < lo || n > hi {
            form_ok = false;
            missing.push(format!("{n} sentences (wanted {lo}-{hi})"));
        }
    }
    if answer.chars().count() > case.max_chars {
        form_ok = false;
        missing.push(format!("over {} chars", case.max_chars));
    }
    (facts_ok as u8 + form_ok as u8, missing)
}

/// Answer quality on this machine's models: eight general questions with a
/// deterministic rubric, two points each, on the memory leg's candidates. The
/// capability table (`model_caps`) scores by family name; the line shows the
/// table's `overall` beside the measured points so the two can be compared
/// on every machine - the table's size tiers are to be set from these
/// numbers, never by hand (NEXT.md §A0, Eric 2026-10-12).
/// Time one model gets in the answer-quality leg before it is called too
/// slow for this machine (not a failure).
const QUALITY_MODEL_BUDGET: std::time::Duration = std::time::Duration::from_secs(180);

pub async fn leg_quality(bin: &Path, dir: &Path, loaded: Option<&str>, sink: Sink<'_>) -> Vec<String> {
    use std::time::Duration;
    let mut failures = Vec::new();
    let mut models: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".gguf") && !n.to_lowercase().contains("mmproj"))
                .filter(|n| crate::gguf::read_meta(&dir.join(n)).map(|m| !m.is_embedding()).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    if models.is_empty() {
        sink("no chat model to grade".into());
        return failures;
    }
    let all = models.len();
    let models = memory_grade_candidates(&models, loaded, crate::llm::UTILITY_MODEL_FILE);
    sink(format!("grading {} of {} chat model(s): the helper, the loaded model, then the strongest by the registry", models.len(), all));
    let client = crate::llm::local_http();
    let free = free_vram_gb(bin);
    let max_points = (QUALITY_GOLD.len() * 2) as u32;
    for model in models {
        if cancelled() {
            break;
        }
        let mut args: Vec<String> = vec![
            "--port".into(), bench_port().to_string(), "--host".into(), "127.0.0.1".into(),
            "--no-webui".into(), "--reasoning".into(), "off".into(),
            "--ctx-size".into(), "4096".into(), "--fit".into(), "off".into(),
            "--model".into(), model.clone(),
        ];
        if let Ok(meta) = crate::gguf::read_meta(&dir.join(&model)) {
            let size = std::fs::metadata(dir.join(&model)).map(|m| m.len()).unwrap_or(0);
            if let Some(n) = moe_decision(&meta, size, 4096, free) {
                if n > 0 {
                    if (n as usize) < meta.expert_bytes_per_layer.len() {
                        args.push("--n-cpu-moe".into());
                        args.push(n.to_string());
                    } else {
                        args.push("--cpu-moe".into());
                    }
                }
            }
        }
        let _guard = match spawn_bench_server(bin, dir, args).await {
            Ok(g) => g,
            Err(e) => {
                sink(format!("{model}: could not load ({e})"));
                failures.push(format!("{model}: load"));
                continue;
            }
        };
        let started = std::time::Instant::now();
        let mut points = 0u32;
        let mut per: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let mut gen_tokens = 0u64;
        let mut gen_ms = 0u128;
        let mut broke = false;
        let mut answered = 0usize;
        let mut too_slow = false;
        for (i, case) in QUALITY_GOLD.iter().enumerate() {
            // A model that thinks at length on a small card took 8 minutes for
            // ten questions with nothing on the screen (10-20, Linux): a time
            // limit per model, and a line when a question runs long.
            if started.elapsed() > QUALITY_MODEL_BUDGET {
                too_slow = true;
                break;
            }
            let body = serde_json::json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": format!("You are a helpful, plain-spoken assistant. Answer the question directly.{}", case.system_extra.unwrap_or(""))},
                    {"role": "user", "content": case.ask}
                ],
                // A model whose template thinks first spends its budget on the
                // thinking before a word of answer (the Opus-distilled 4B: 580
                // tokens a question at 700, graded on truncated thinking). Room
                // for both; a cut-off answer is still named below.
                "max_tokens": 1500, "temperature": 0.0, "stream": false, "cache_prompt": true,
                "stop": crate::llm::chat_stop_strings(&model),
            });
            let t0 = std::time::Instant::now();
            let v: Result<serde_json::Value, String> = async {
                client.post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port())).json(&body).timeout(Duration::from_secs(300)).send().await
                    .map_err(|e| format!("request: {e}"))?.json().await.map_err(|e| format!("json: {e}"))
            }.await;
            let v = match v {
                Ok(v) => v,
                Err(e) => {
                    sink(format!("{model}: question {} failed ({e})", i + 1));
                    failures.push(format!("{model}: call"));
                    broke = true;
                    break;
                }
            };
            gen_ms += t0.elapsed().as_millis();
            answered += 1;
            if t0.elapsed() > std::time::Duration::from_secs(30) {
                sink(format!("  ... {model}: question {} of {} took {} s", i + 1, QUALITY_GOLD.len(), t0.elapsed().as_secs()));
            }
            gen_tokens += v["usage"]["completion_tokens"].as_u64().unwrap_or(0);
            let content_empty = v["choices"][0]["message"]["content"].as_str().map(|c| c.trim().is_empty()).unwrap_or(true);
            let cut_off = v["choices"][0]["finish_reason"].as_str() == Some("length");
            let answer = crate::llm::completion_text(&v);
            let (p, mut missing) = score_quality(case, &answer);
            if content_empty && cut_off {
                missing.insert(0, "ran out of budget while thinking - no answer reached".into());
            }
            points += p as u32;
            per.push(format!("q{} {p}", i + 1));
            if p < 2 {
                notes.push(format!("  q{} ({}): {}", i + 1, case.ask.chars().take(40).collect::<String>(), missing.join("; ")));
            }
        }
        if broke {
            continue;
        }
        if too_slow {
            // Not a failure: what this machine can do with this model, said.
            sink(format!(
                "quality: {model}: too slow to finish here - {answered} of {} answered in {} s ({points} points so far); not graded",
                QUALITY_GOLD.len(),
                started.elapsed().as_secs()
            ));
            for n in &notes {
                sink(n.clone());
            }
            continue;
        }
        let registry = crate::model_caps::caps_for(&model).overall;
        let tps = if gen_ms > 0 { gen_tokens as f64 / (gen_ms as f64 / 1000.0) } else { 0.0 };
        sink(format!(
            "quality: {model}: {points}/{max_points} points ({}) | registry overall {registry} | {gen_tokens} tokens at {tps:.1} tok/s in {:.0} s",
            per.join(", "),
            started.elapsed().as_secs_f64()
        ));
        for n in notes.iter().take(8) {
            sink(n.clone());
        }
        // Under half the points is a model that is not answering plain questions here.
        if points * 2 < max_points {
            failures.push(format!("{model}: {points} of {max_points} points"));
        }
    }
    failures
}

// ───────────── the report/code mode classifier (turnModeClassifier.ts), measured ─────────────

/// Mirror of BASE in src/utils/turnModeClassifier.ts - the leg grades the very
/// prompt the app uses; change both together.
const MODE_SYSTEM: &str = "Classify what the user is asking the assistant to DO right now. Output exactly one word: REPORT, CODE, or NEITHER.\nOnly answer REPORT or CODE if the user is asking YOU, the assistant, to PRODUCE it for them right now. If they are merely talking about, mentioning, complaining about, or planning a report/code in their own work or life, answer NEITHER.\nREPORT = asking you to WRITE/DRAFT/PUT TOGETHER a report, write-up, analysis, deep-dive, or structured document.\nCODE = asking you to write, implement, or fix code / a function / a script.\nNEITHER = everything else, INCLUDING ordinary questions ('explain X', 'how does X work'), capability questions ('can you code?'), and any mention of a report/code that isn't a request for you to make one.\n\nExamples:\nwrite me a report on solar energy => REPORT\nput together a detailed write-up of our options => REPORT\ndo a deep dive on how transformers work => REPORT\nexplain how vaccines work => NEITHER\nhow does the TLS handshake work? => NEITHER\nmy boss wants a report by friday => NEITHER\ni need to write a report for work => NEITHER\nthe quarterly report is due tomorrow => NEITHER\ni hate writing reports => NEITHER\nthe news report said it might rain => NEITHER\nwrite a python function to parse a csv => CODE\nfix this code: def f(): retrun 1 => CODE\nour codebase is a mess => NEITHER\ndo you know how to code? => NEITHER\nhey how's it going => NEITHER";
const MODE_GRAMMAR: &str = "root ::= \"REPORT\" | \"CODE\" | \"NEITHER\"";
/// Mirror of REPORT_KW (turnModeClassifier.ts) and CODE_KW (routingKeywords.ts):
/// the free pre-gate - a message with none of these never reaches the model
/// and is NEITHER by construction. Change both together.
const MODE_REPORT_KW: &[&str] = &["report", "write up", "write-up", "writeup", "analysis", "analyze", "analyse", "deep dive", "deep-dive", "breakdown", "rundown", "overview", "draft", "write me", "put together", "in-depth", "in depth", "dossier", "white paper", "comprehensive"];
const MODE_CODE_KW: &[&str] = &["code", "function", "bug", "debug", "error", "exception", "compile", "script", "program", "implement", "refactor", "snippet", "fix this", "rewrite this", "api", "regex", "algorithm", "stack trace", "syntax", "variable", "python", "javascript", "typescript", "rust", "java", "c++", "sql", "html", "css", "git", "terminal", "shell", "npm", "docker", "class ", "def ", "import "];
/// Mirror of REPORT_STRONG / CODE_STRONG (turnModeClassifier.ts): an unambiguous
/// ask-the-assistant phrasing decides on its own, before any model. Change both together.
const MODE_REPORT_STRONG: &[&str] = &["write me a report", "write a report", "write up a", "write a detailed", "put together a report", "do a deep dive", "deep dive on", "in-depth report", "write a comprehensive", "draft a report"];
const MODE_CODE_STRONG: &[&str] = &["write a function", "write code", "write a script", "write me a script", "write a program", "implement a", "implement the", "fix this code", "debug this", "refactor this"];
fn mode_strong_verdict(message: &str) -> Option<&'static str> {
    let m = message.to_lowercase();
    if MODE_REPORT_STRONG.iter().any(|k| m.contains(k)) { return Some("REPORT"); }
    if MODE_CODE_STRONG.iter().any(|k| m.contains(k)) { return Some("CODE"); }
    None
}
fn mode_pregate_relevant(message: &str) -> bool {
    let m = format!(" {} ", message.to_lowercase());
    MODE_REPORT_KW.iter().chain(MODE_CODE_KW.iter()).any(|k| m.contains(k))
}
/// Held out from the prompt's examples (never copy one in): what a person
/// might say, and whether the turn should become a report, code, or stay chat.
const MODE_GOLD: &[(&str, &str)] = &[
    ("write me a report on the state of electric buses in europe", "REPORT"),
    ("put together a write-up comparing the three mortgage offers", "REPORT"),
    ("do a deep dive on the history of the esperanto movement", "REPORT"),
    ("draft a comprehensive analysis of our churn numbers", "REPORT"),
    ("can you write up an overview of the main theories of consciousness", "REPORT"),
    ("give me an in-depth breakdown of how the eu ai act affects small startups", "REPORT"),
    ("prepare a dossier on the company's main competitors", "REPORT"),
    ("I need a detailed report on the pros and cons of heat pumps for our house", "REPORT"),
    ("write a bash script that renames every photo by its exif date", "CODE"),
    ("implement a function that merges two sorted lists in rust", "CODE"),
    ("fix this: for i in range(10) print(i)", "CODE"),
    ("can you write the sql to find customers with no orders this year", "CODE"),
    ("refactor this javascript so it uses async/await", "CODE"),
    ("debug this regex, it never matches emails with a plus sign", "CODE"),
    ("write a small python program that watches a folder and prints new files", "CODE"),
    ("implement the binary search in typescript", "CODE"),
    ("how does a heat pump work", "NEITHER"),
    ("my manager asked me for a report by monday, I'm stressed", "NEITHER"),
    ("the code review took three hours today", "NEITHER"),
    ("what's the difference between a function and a method", "NEITHER"),
    ("do you know how to write sql", "NEITHER"),
    ("the annual report came out and the numbers look bad", "NEITHER"),
    ("I spent all weekend writing reports", "NEITHER"),
    ("can you explain what a deep dive means", "NEITHER"),
    ("any dinner ideas for tonight", "NEITHER"),
    ("the weather report says snow", "NEITHER"),
    ("is python a good first language", "NEITHER"),
    ("what did the analysis in the paper conclude", "NEITHER"),
    ("tell me about the programming language rust", "NEITHER"),
    ("our codebase at work is a mess, any advice on how to cope", "NEITHER"),
];

/// The report/code mode classifier on this machine's models: the app's own
/// prompt, grammar and pre-gate against MODE_GOLD, on the same candidates the
/// memory leg grades (the helper, the loaded model, the strongest by the
/// registry). A line per model: right of total, per class, pre-gated count.
pub async fn leg_turn_mode(bin: &Path, dir: &Path, loaded: Option<&str>, sink: Sink<'_>) -> Vec<String> {
    use std::time::Duration;
    let mut failures = Vec::new();
    let mut models: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".gguf") && !n.to_lowercase().contains("mmproj"))
                .filter(|n| crate::gguf::read_meta(&dir.join(n)).map(|m| !m.is_embedding()).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    if models.is_empty() {
        sink("no chat model to grade".into());
        return failures;
    }
    let all = models.len();
    let models = memory_grade_candidates(&models, loaded, crate::llm::UTILITY_MODEL_FILE);
    sink(format!("grading {} of {} chat model(s): the helper, the loaded model, then the strongest by the registry", models.len(), all));
    let pregated = MODE_GOLD.iter().filter(|(m, _)| !mode_pregate_relevant(m)).count();
    let strong = MODE_GOLD.iter().filter(|(m, _)| mode_pregate_relevant(m) && mode_strong_verdict(m).is_some()).count();
    sink(format!("{strong} of {} messages are decided by the strong phrasings alone (no model call)", MODE_GOLD.len()));
    sink(format!("{} of {} messages never reach a model (the free pre-gate says NEITHER){}", pregated, MODE_GOLD.len(),
        if MODE_GOLD.iter().any(|(m, want)| !mode_pregate_relevant(m) && *want != "NEITHER") { " - WARNING: a report/code request among them" } else { "" }));
    let client = crate::llm::local_http();
    let free = free_vram_gb(bin);
    for model in models {
        if cancelled() {
            break;
        }
        let mut args: Vec<String> = vec![
            "--port".into(), bench_port().to_string(), "--host".into(), "127.0.0.1".into(),
            "--no-webui".into(), "--reasoning".into(), "off".into(),
            "--ctx-size".into(), "4096".into(), "--fit".into(), "off".into(),
            "--model".into(), model.clone(),
        ];
        if let Ok(meta) = crate::gguf::read_meta(&dir.join(&model)) {
            let size = std::fs::metadata(dir.join(&model)).map(|m| m.len()).unwrap_or(0);
            if let Some(n) = moe_decision(&meta, size, 4096, free) {
                if n > 0 {
                    if (n as usize) < meta.expert_bytes_per_layer.len() {
                        args.push("--n-cpu-moe".into());
                        args.push(n.to_string());
                    } else {
                        args.push("--cpu-moe".into());
                    }
                }
            }
        }
        let _guard = match spawn_bench_server(bin, dir, args).await {
            Ok(g) => g,
            Err(e) => {
                sink(format!("{model}: could not load ({e})"));
                failures.push(format!("{model}: load"));
                continue;
            }
        };
        let started = std::time::Instant::now();
        let (mut right, mut asked) = (0usize, 0usize);
        let mut per: std::collections::BTreeMap<&str, (usize, usize)> = std::collections::BTreeMap::new();
        let mut wrong: Vec<String> = Vec::new();
        for (message, want) in MODE_GOLD {
            let e = per.entry(want).or_insert((0, 0));
            e.1 += 1;
            let got: String = if !mode_pregate_relevant(message) {
                "NEITHER".into()
            } else if let Some(v) = mode_strong_verdict(message) {
                v.into()
            } else {
                let body = serde_json::json!({
                    "model": model,
                    "messages": [{"role": "system", "content": MODE_SYSTEM}, {"role": "user", "content": message}],
                    "max_tokens": 3, "temperature": 0.0, "stream": false, "grammar": MODE_GRAMMAR, "cache_prompt": true,
                    "stop": crate::llm::chat_stop_strings(&model),
                });
                let v: Result<serde_json::Value, String> = async {
                    client.post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port())).json(&body).timeout(Duration::from_secs(120)).send().await
                        .map_err(|e| format!("request: {e}"))?.json().await.map_err(|e| format!("json: {e}"))
                }.await;
                match v {
                    Ok(v) => {
                        asked += 1;
                        let w = crate::llm::completion_text(&v).trim().to_uppercase();
                        if w.starts_with("REPORT") { "REPORT".into() } else if w.starts_with("CODE") { "CODE".into() } else { "NEITHER".into() }
                    }
                    Err(e) => {
                        sink(format!("{model}: classifier call failed ({e})"));
                        failures.push(format!("{model}: call"));
                        break;
                    }
                }
            };
            if got == *want {
                right += 1;
                e.0 += 1;
            } else {
                wrong.push(format!("{message} => {got} (wanted {want})"));
            }
        }
        let per_line: Vec<String> = per.iter().map(|(k, (r, n))| format!("{} {r}/{n}", k.to_lowercase())).collect();
        sink(format!("modes: {model}: {right}/{} right ({}) | {asked} model calls in {:.0} s", MODE_GOLD.len(), per_line.join(", "), started.elapsed().as_secs_f64()));
        for w in wrong.iter().take(6) {
            sink(format!("  {w}"));
        }
        // A classifier that turns chat into a report is worse than one that
        // misses: any NEITHER wrong is listed; under 80% right is a fail.
        if right * 5 < MODE_GOLD.len() * 4 {
            failures.push(format!("{model}: {right} of {} right", MODE_GOLD.len()));
        }
    }
    failures
}

/// LIED GREEN (claimed ok, did not load); a conservative red that runs is
/// reported but not a failure.
/// Free memory a test load needs before it is started: the app loader's
/// gate (2 GiB and a tenth of the file), plus the whole file when the model
/// is graded too big - that check loads it to see it fail.
pub(crate) fn load_room_needed(file_bytes: u64, graded_too_big: bool) -> u64 {
    let gate = 2u64 * 1024 * 1024 * 1024 + file_bytes / 10;
    if graded_too_big { gate + file_bytes } else { gate }
}

pub async fn leg_fit_truth(app: Option<&AppHandle>, bin: &Path, dir: &Path, only: &[String], sink: Sink<'_>) -> Vec<String> {
    let free_vram = || free_vram_gb(bin);
    // The grade column is the APP's grade (assess: MoE split, tune pins,
    // measured evidence) - the plain dense grade printed "claimed RED,
    // actually RAN" for every split MoE and read as a regression. The
    // headless harness (no app) keeps the plain grade.
    let app_grades: std::collections::HashMap<String, crate::fit::Fit> = match app {
        Some(app) => crate::fit::assess(app).await.into_iter().map(|f| (f.name, f.fit)).collect(),
        None => Default::default(),
    };
    // The same figures assess() grades with - the badge and the matrix
    // must read one machine (the Air's MedGemma said "Too large" in the
    // app and Green in the matrix until they did).
    let figures = crate::fit::MachineFigures::with_vram(free_vram());
    let total_ram = figures.total_ram_gb;
    let avail_ram = figures.avail_ram_gb;
    let files: Vec<String> = gguf_files(dir)
        .into_iter()
        .filter(|n| only.is_empty() || only.iter().any(|o| n == o))
        .collect();
    sink(format!(
        "{} models, RAM {total_ram:.1} GB total / {avail_ram:.1} GB available now (graded against available, as the app does)",
        files.len()
    ));
    if cfg!(windows) && crate::llm::driver_vram_blocking().is_none() {
        // Without the driver's own count (nvidia-smi), the engine probe is a
        // constant under Windows' display driver model and the delta column
        // reads near zero; loads and speeds are the evidence then.
        sink("note: no nvidia-smi on this Windows box - the realGB column comes from the engine probe and is unreliable here; load success and speeds are the proof".into());
    }
    sink(format!(
        "{:<38} {:>5} {:>6} {:>6} {:>6} | {:>7} {:>6} {:>6} {:>7}  verdict",
        "model", "grade", "ctx", "estGB", "freeGB", "realGB", "load_s", "gen", "prompt"
    ));
    sink("  (gen = tok/s writing at a near-empty context; prompt = tok/s reading a ~700-token prompt, where start-up cost dominates - a long prompt reads several times faster, see the tune bench leg)".into());
    let mut failures = Vec::new();
    for name in files {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let path = dir.join(&name);
        let meta = match crate::gguf::read_meta(&path) {
            Ok(m) => m,
            Err(e) => {
                sink(format!("{name}: header error {e:?}"));
                continue;
            }
        };
        if meta.is_embedding() {
            continue;
        }
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        let free = free_vram();
        let ctx = crate::fit::choose_ctx(&meta, size, total_ram, free);
        let (w, kv, need) = crate::fit::model_need(&meta, size, ctx);
        let grade = app_grades
            .get(&name)
            .copied()
            .unwrap_or_else(|| crate::fit::grade(need, free, avail_ram));
        let arm = TuneArm { ctx, moe_cpu_layers: moe_decision(&meta, size, ctx, free), draft: false, kv_q8: false, ubatch: 0, cpu: false, gpu: None };
        // Room for this load, measured now: the app's loader gate (2 GiB plus
        // a tenth of the file), and for a model graded too big, its whole
        // file on top - a "does it really not load?" check is only safe
        // with the memory to fail in (10-20: a 4 GB card, swap full, 3.7 GB
        // free, and the check took the machine's memory down).
        let free_now = crate::llm::grading_memory_bytes();
        let room_needed = load_room_needed(size, grade == crate::fit::Fit::Red);
        if free_now > 0 && free_now < room_needed {
            sink(format!(
                "{:<38} {:>5} {:>6} {:>6.2}        | not tried: {:.1} GB of memory free, a safe load needs about {:.1} GB (close other apps and run again)",
                name,
                format!("{:?}", grade),
                ctx,
                need,
                free_now as f64 / 1024f64.powi(3),
                room_needed as f64 / 1024f64.powi(3),
            ));
            continue;
        }
        let before = free_vram();
        let r = bench_one(bin, dir, &name, arm, None, None, &[], Some(&free_vram), false).await;
        let after_kill = free_vram();
        let real = match (before, r.during_free) {
            (Some(b), Some(d)) => format!("{:.2}", b - d),
            _ => "?".into(),
        };
        // A non-red grade that loads and then generates at a crawl on a
        // discrete card is the Windows CUDA sysmem-fallback shape: the
        // driver spills to system RAM instead of failing, so the model
        // "fits" and runs an order of magnitude slow (4060 Ti report:
        // Ornith 2.5 tok/s where its size class does 26). A split MoE is
        // exempt - CPU experts are slow honestly.
        let crawled = r.failed.is_none()
            && grade != crate::fit::Fit::Red
            && arm.moe_cpu_layers.is_none()
            && before.is_some()
            && r.gen_tps > 0.0
            && r.gen_tps < 6.0;
        let verdict = if r.failed.is_some() {
            if grade == crate::fit::Fit::Red {
                "red CONFIRMED (did not load)"
            } else {
                "CLAIMED ok, DID NOT LOAD"
            }
        } else if crawled {
            "ran but CRAWLED - likely spilled off the card"
        } else if grade == crate::fit::Fit::Red {
            "claimed RED, actually RAN"
        } else {
            "ok"
        };
        sink(format!(
            "{:<38} {:>5} {:>6} {:>6.2} {:>6} | {:>7} {:>6.1} {:>6.1} {:>7.0}  {}{}",
            name,
            format!("{:?}", grade),
            ctx,
            need,
            before.map(|b| format!("{b:.2}")).unwrap_or("-".into()),
            real,
            r.load_secs,
            r.gen_tps,
            r.pp_tps,
            verdict,
            r.failed
                .as_deref()
                .map(|f| format!("  [{}]", &f[..f.len().min(80)]))
                .unwrap_or_default()
        ));
        sink(format!(
            "  est: weights {w:.2} + kv {kv:.2} + 0.8 | parser: layers {} attn {} kv_heads {} dim {} moe_flag {:?} | freed-after-kill {:?}",
            meta.n_layers,
            meta.n_attn_layers,
            meta.n_kv_heads,
            meta.head_dim(),
            arm.moe_cpu_layers,
            after_kill.map(|a| format!("{a:.2}"))
        ));
        if verdict == "CLAIMED ok, DID NOT LOAD" {
            failures.push(format!("{name}: {verdict}"));
        } else if crawled {
            failures.push(format!(
                "{name}: graded {:?} but generated at {:.1} tok/s - likely spilled off the card",
                grade, r.gen_tps
            ));
        }
    }
    failures
}

/// Every downloaded chat model answers every scenario with WORDS -
/// non-empty, no channel markers of any format. Returns the failures.
/// `YOAI_CHAT_FORMAT_MODELS=a.gguf,b.gguf` filters (repro runs).
pub async fn leg_chat_format(bin: &Path, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let only: Vec<String> = std::env::var("YOAI_CHAT_FORMAT_MODELS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut failures = Vec::new();
    for name in gguf_files(dir)
        .into_iter()
        .filter(|n| only.is_empty() || only.iter().any(|o| n == o))
    {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let path = dir.join(&name);
        let meta = match crate::gguf::read_meta(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_embedding() {
            continue;
        }
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        let free = free_vram_gb(bin);
        let arm = TuneArm { ctx: 4096, moe_cpu_layers: moe_decision(&meta, size, 4096, free), draft: false, kv_q8: false, ubatch: 0, cpu: false, gpu: None };
        let r = bench_chat_format(bin, dir, &name, arm, &CHAT_SCENARIOS).await;
        sink(format!("{:<38} {}", name, r));
        if r.starts_with("FAIL") {
            failures.push(format!("{name}: {r}"));
        }
    }
    failures
}

/// The sampling knobs REACH the engine: one server, temperature 0 twice
/// must answer identically; high temperature with two seeds must not both
/// reproduce the greedy text.
pub async fn leg_sampling(bin: &Path, dir: &Path, model: &str, sink: Sink<'_>) -> Result<(), String> {
    use std::time::Duration;
    let meta = crate::gguf::read_meta(&dir.join(model)).map_err(|e| format!("header: {e:?}"))?;
    let size = std::fs::metadata(dir.join(model)).map_err(|e| e.to_string())?.len();
    let free = free_vram_gb(bin);
    let mut args: Vec<String> = vec![
        "--port".into(), bench_port().to_string(),
        "--host".into(), "127.0.0.1".into(),
        "--no-webui".into(), "--reasoning".into(), "off".into(),
        "--ctx-size".into(), "4096".into(), "--fit".into(), "off".into(),
        "--model".into(), model.to_string(),
    ];
    if let Some(n) = moe_decision(&meta, size, 4096, free) {
        if n > 0 {
            if (n as usize) < meta.expert_bytes_per_layer.len() {
                args.push("--n-cpu-moe".into());
                args.push(n.to_string());
            } else {
                args.push("--cpu-moe".into());
            }
        }
    }
    let guard = spawn_bench_server(bin, dir, args).await.map_err(|e| format!("load: {e}"))?;
    let client = crate::llm::local_http();
    let ask = |sampling: Option<crate::llm::SamplingParams>, seed: Option<u64>| {
        let client = client.clone();
        let model = model.to_string();
        async move {
            let mut body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Describe an imaginary small town in about 60 words."}],
                "max_tokens": 80,
                "stream": false,
                "stop": crate::llm::chat_stop_strings(&model),
                // Both greedy asks must walk the same path: with the prompt
                // cache on, the second reuses the first's KV prefix and the
                // batch shape differs, which on a GPU can flip a token late
                // in the reply (4060 Ti, 09-13) - the knob was fine.
                "cache_prompt": false,
            });
            let (budget, effort) = crate::llm::chat_turn_reasoning_controls(&model);
            if let Some(b) = budget {
                body["reasoning_budget_tokens"] = serde_json::json!(b);
            }
            if let Some(e) = effort {
                body["reasoning_effort"] = serde_json::json!(e);
            }
            crate::llm::apply_sampling(&mut body, sampling.as_ref(), false);
            if let Some(sd) = seed {
                body["seed"] = serde_json::json!(sd);
            }
            let v: Result<serde_json::Value, String> = match client
                .post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port()))
                .json(&body)
                .timeout(Duration::from_secs(180))
                .send()
                .await
            {
                Ok(r) => r.json().await.map_err(|e| format!("json: {e}")),
                Err(e) => Err(format!("request: {e}")),
            };
            v.map(|v| crate::llm::completion_text(&v))
        }
    };
    let greedy = crate::llm::SamplingParams { temperature: Some(0.0), ..Default::default() };
    let a1 = ask(Some(greedy), None).await?;
    let a2 = ask(Some(greedy), None).await?;
    sink(format!(
        "greedy len {}: {}",
        a1.len(),
        a1.chars().take(80).collect::<String>()
    ));
    if a1.trim().is_empty() {
        return Err("greedy reply empty".into());
    }
    if a1 != a2 {
        // Where they part says which it is: a long shared prefix is engine
        // rounding on this card; two unrelated replies is the knob not
        // reaching the engine.
        let shared = a1.chars().zip(a2.chars()).take_while(|(x, y)| x == y).count();
        let tail = |t: &str| t.chars().skip(shared).take(40).collect::<String>();
        sink(format!(
            "greedy replies differ after {shared} shared chars of {} / {}: \"{}\" vs \"{}\"",
            a1.chars().count(), a2.chars().count(), tail(&a1), tail(&a2)
        ));
        return Err(if shared >= 40 {
            format!("temperature 0 drifted after {shared} shared chars - engine rounding on this card, not the knob")
        } else {
            "temperature 0 was not deterministic from the start - the knob did not reach the engine?".into()
        });
    }
    let wild = crate::llm::SamplingParams { temperature: Some(1.8), top_p: Some(1.0), ..Default::default() };
    let b1 = ask(Some(wild), Some(7)).await?;
    let b2 = ask(Some(wild), Some(8)).await?;
    sink(format!("wild lens {} / {}", b1.len(), b2.len()));
    if b1.trim().is_empty() || b2.trim().is_empty() {
        return Err("wild reply empty".into());
    }
    if b1 == a1 && b2 == a1 {
        return Err("high temperature reproduced the greedy text twice - sampling not applied".into());
    }
    drop(guard);
    Ok(())
}

/// Two tune arms load and produce real timings on this machine - the same
/// arms and bench the Fine-tune dialog's tune run uses.
pub async fn leg_tune_bench(bin: &Path, dir: &Path, model: &str, sink: Sink<'_>) -> Result<(), String> {
    let meta = crate::gguf::read_meta(&dir.join(model)).map_err(|e| format!("header: {e:?}"))?;
    let size = std::fs::metadata(dir.join(model)).map_err(|e| e.to_string())?.len();
    let free = free_vram_gb(bin);
    sink(format!("free VRAM: {free:?}"));
    let arms = arms_for(&meta, size, total_ram_gb(), free, false, None);
    if arms.is_empty() {
        return Err("no arms".into());
    }
    for arm in arms.into_iter().take(2) {
        if cancelled() {
            return Err("cancelled".into());
        }
        // The second arm reads the LONG prompt (~5k tokens): the honest
        // reading figure, next to the short one above it.
        let long = arm.ubatch > 0;
        let r = bench_one(bin, dir, model, arm, None, None, &[], None, long).await;
        sink(format!(
            "arm ctx {} moe {:?} draft {}{}: load {:.1} s, prompt {:.0} tok/s ({} prompt), gen {:.1} tok/s, failed {:?}",
            r.ctx, r.moe_cpu_layers, r.draft, if r.ubatch > 0 { format!(" ubatch {}", r.ubatch) } else { String::new() },
            r.load_secs, r.pp_tps, if long || r.ubatch > 0 { "~5k-token" } else { "~700-token" }, r.gen_tps, r.failed
        ));
        if let Some(f) = r.failed {
            return Err(format!("arm failed: {f}"));
        }
        if r.gen_tps <= 0.0 {
            return Err("no generation timing".into());
        }
    }
    Ok(())
}

/// MLX artifacts: listed on every platform; chat-format benched where the
/// platform runs them. Returns the failures.
pub async fn leg_mlx(app: &AppHandle, bin_dir_hint: &Path, sink: Sink<'_>) -> Vec<String> {
    let artifacts = mlx_artifact_dirs(bin_dir_hint);
    if artifacts.is_empty() {
        sink("no MLX models installed".into());
        return Vec::new();
    }
    if !crate::mlx_engine::supported() {
        sink(format!(
            "{} MLX model(s) present but this platform does not run MLX (Apple Silicon only) - not tested",
            artifacts.len()
        ));
        return Vec::new();
    }
    let Some(bin) = crate::mlx_engine::swiftlm_binary(app) else {
        sink("MLX engine not installed - artifacts listed, not tested".into());
        for a in &artifacts {
            sink(format!("  {a}"));
        }
        return Vec::new();
    };
    let mut failures = Vec::new();
    for name in artifacts {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let model_dir = bin_dir_hint.join(&name);
        // Bench only what the app would serve: a half-downloaded artifact
        // (disk filled mid-download, say) is not a finding about the model.
        if !crate::mlx_artifacts::artifact_complete(&model_dir) {
            sink(format!(
                "{:<38} skipped - download incomplete (the app will not serve it either)",
                name
            ));
            continue;
        }
        let args = vec![
            "--model".into(),
            model_dir.display().to_string(),
            "--port".into(),
            bench_port().to_string(),
        ];
        let verdict = match spawn_bench_server(&bin, bin_dir_hint, args).await {
            Ok(guard) => {
                let v = ask_scenarios(&name, &CHAT_SCENARIOS, None).await;
                drop(guard);
                v
            }
            Err(e) => format!("FAIL load: {e}"),
        };
        sink(format!("{:<38} {}", name, verdict));
        if verdict.starts_with("FAIL") {
            failures.push(format!("{name}: {verdict}"));
        }
    }
    failures
}

/// The engine binary the app would actually chat with: the downloaded CUDA
/// build when active, else the bundled server (in dev, the repo's shipped
/// binary - the same file the test legs run).
/// The helper (utility) model file - the Settings > Components download.
/// Mirrors UTILITY_MODEL.filename in src/data/recommended-models.ts.
const HELPER_FILE: &str = "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf";

/// The small CPU servers start and answer: the embedding server (memory
/// recall, freshness, the semantic health gate) and, when installed, the
/// helper model with the routing classifier's own grammar. These die
/// silently in the field (the chat server hides them behind a working
/// engine), so the matrix proves them on every machine.
pub async fn leg_components(app: &AppHandle, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    match crate::build_install::installed_version(app) {
        Some(v) if v == crate::build_install::pinned_version() => sink(format!("agent program: v{v} (what this app ships)")),
        Some(v) => sink(format!("agent program: v{v} installed, this app ships v{} - update in Add-ons > Components", crate::build_install::pinned_version())),
        None => sink("agent program: not installed (project work installs it)".into()),
    }
    let st = app.state::<crate::llm::LLMState>();
    if dir.join(crate::router::EMBED_MODEL).exists() {
        let texts = vec!["hello world".to_string(), "a second phrase".to_string(), "and a third".to_string()];
        match crate::llm::embed_texts(app.clone(), st.clone(), texts, crate::router::EMBED_MODEL.to_string()).await {
            Ok(v) if v.len() == 3 && v.iter().all(|x| !x.is_empty()) => {
                sink(format!("embedding server: ok (3 vectors, dim {})", v[0].len()));
            }
            Ok(v) => {
                sink(format!("embedding server: FAIL - {} vectors back for 3 inputs", v.len()));
                failures.push("components: embedding server returned the wrong count".into());
            }
            Err(e) => {
                sink(format!("embedding server: FAIL - {e}"));
                failures.push(format!("components: embedding server - {e}"));
            }
        }
    } else {
        sink("embedding model: not installed (memory recall, freshness and the semantic health check are off; Online-first stays on the device by design)".into());
    }
    if dir.join(HELPER_FILE).exists() {
        // The app's own instruction, word for word (routingTaskClassifier.ts reads the same file).
        const BASE: &str = include_str!("../../src/utils/routingClassifierPrompt.txt");
        const GRAMMAR: &str = "root ::= (\"CODE\" | \"MATH\" | \"REASONING\" | \"GENERAL\") \" \" (\"EASY\" | \"HARD\")";
        // Messages the classifier really sees: the free keyword gate passes
        // only code/math/reasoning-looking ones (a plain "capital of France"
        // never reaches the model), so the GENERAL probe is a keyword false
        // alarm the classifier must correct. TASK must be right; a different
        // DIFFICULTY is noted, not failed (it is a judgment call).
        let probes = [
            ("write a python function that parses a csv file", "CODE", "EASY"),
            ("prove that there are infinitely many primes", "MATH", "HARD"),
            ("how do I debug my sourdough starter, it isn't rising", "GENERAL", "EASY"),
        ];
        let mut lines = Vec::new();
        for (q, want_task, want_diff) in probes {
            if cancelled() {
                break;
            }
            match crate::llm::utility_chat(app.clone(), st.clone(), HELPER_FILE.to_string(), BASE.to_string(), q.to_string(), Some(GRAMMAR.to_string()), 12).await {
                Ok(out) => {
                    let verdict = out.trim().to_uppercase();
                    let mut words = verdict.split_whitespace();
                    let (task, diff) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
                    let note = if task != want_task {
                        failures.push(format!("components: helper read {q:?} as {task} (it is {want_task}) - routing would send it the wrong way"));
                        format!(" - WRONG, it is {want_task}")
                    } else if diff != want_diff {
                        format!(" (expected {want_diff})")
                    } else {
                        String::new()
                    };
                    if task.is_empty() || diff.is_empty() {
                        failures.push(format!("components: helper verdict unparseable: {verdict:?}"));
                    }
                    lines.push(format!("{q:?} -> {verdict}{note}"));
                }
                Err(e) => {
                    sink(format!("helper model: FAIL - {e}"));
                    failures.push(format!("components: helper model - {e}"));
                    break;
                }
            }
        }
        if !lines.is_empty() {
            sink(format!("helper model: answers ({})", lines.join("; ")));
        }
    } else {
        sink("helper model: not installed (routing difficulty stays unknown; memory extraction rides the chat model)".into());
    }
    failures
}

/// Routing, decide-only, on this machine's models: the battery's query
/// table (src-tauri/route-battery.json, generated from tools/route-battery.mjs)
/// through `route_dry` for every mode and dial. Never loads a model, never
/// generates, never spends credits: the online catalog list is a free
/// call and the query embeds run locally. Asserts the promises and prints
/// the per-bucket online shares.
/// ONE FIGURE: with model A on the card, the grade's "runs at" for model B
/// (credited by A's MEASURED footprint) must equal the context the loader
/// gives B once A is gone. Also proves the footprint measurement lands
/// (a calibration record for A) and that the driver's count is the source
/// on an NVIDIA card. Runs inside the app (it needs the real loader).
pub async fn leg_headroom(
    app: &AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
    dir: &Path,
    sink: Sink<'_>,
) -> Vec<String> {
    let mut failures = Vec::new();
    // The two smallest chat models: quick loads, and both fit on any card
    // the app runs on.
    let mut files: Vec<(u64, String)> = gguf_files(dir)
        .into_iter()
        .filter_map(|n| {
            let p = dir.join(&n);
            let meta = crate::gguf::read_meta(&p).ok()?;
            if meta.is_embedding() || n.to_lowercase().contains("mmproj") {
                return None;
            }
            Some((std::fs::metadata(&p).ok()?.len(), n))
        })
        .collect();
    files.sort();
    // B must be sized by the figure, not by a fine-tune pin - a pinned
    // context is the same number whatever the card says, and the check
    // would prove nothing (E4B pinned at 128K, Windows 09-13).
    let unpinned = |n: &str| crate::tuning::get(app, n).context.is_none();
    let Some((_, a)) = files.first().cloned() else {
        sink("no chat model - leg skipped".into());
        return failures;
    };
    let Some((_, b)) = files.iter().skip(1).find(|(_, n)| unpinned(n)).cloned() else {
        sink("every other chat model has a fine-tune context pin - nothing for the figure to size; leg skipped".into());
        return failures;
    };
    crate::llm::invalidate_vram_cache().await;
    let free0 = crate::llm::available_vram_mib(app).await;
    sink(format!(
        "empty card: free {} ({})",
        free0.map(|m| format!("{:.2} GB", m as f64 / 1024.0)).unwrap_or_else(|| "none".into()),
        crate::llm::vram_figure_source()
    ));
    if free0.is_none() {
        sink("no graphics figure on this machine (CPU / integrated / Apple) - leg skipped".into());
        return failures;
    }

    // Load A and let the measurement land.
    if let Err(e) = crate::llm::load_model(app.clone(), state.clone(), a.clone(), false, "matrix-headroom".into()).await {
        failures.push(format!("headroom: could not load {a}: {e}"));
        return failures;
    }
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let ctx_a = crate::llm::current_ctx_size();
    match crate::llm::load_calibration_read(app, &a) {
        Some(c) if c.matches(ctx_a as u64, c.kv_q8) && c.actual_gb > 0.1 => sink(format!(
            "{a}: loaded at {ctx_a}, footprint predicted {:.2} GB, measured {:.2} GB",
            c.predicted_gb, c.actual_gb
        )),
        Some(c) => sink(format!("{a}: calibration present but for another shape (ctx {}, {:.2} GB)", c.ctx, c.actual_gb)),
        None => {
            let line = format!("{a}: no footprint measured after the load (source {})", crate::llm::vram_figure_source());
            if crate::llm::vram_figure_source() == "driver" {
                failures.push(format!("headroom: {line}"));
            }
            sink(line);
        }
    }

    // The claim: B's "runs at" with A on the card - graded NOW, not a
    // memoized grade from mid-load.
    crate::llm::invalidate_vram_cache().await;
    crate::fit::forget_assess();
    let claimed = crate::fit::assess(app)
        .await
        .into_iter()
        .find(|f| f.name == b)
        .map(|f| (f.context_runtime, f.fit));
    let Some((claimed_ctx, grade)) = claimed else {
        failures.push(format!("headroom: {b} not graded"));
        return failures;
    };
    sink(format!("{b}: graded {grade:?}, runs at {claimed_ctx} (with {a} on the card, its footprint credited; free {} GB by the {})",
        crate::llm::available_vram_mib(app).await.map(|m| format!("{:.2}", m as f64 / 1024.0)).unwrap_or_else(|| "?".into()),
        crate::llm::vram_figure_source()));

    // The truth: load B and read what the loader gave it.
    if let Err(e) = crate::llm::load_model(app.clone(), state.clone(), b.clone(), false, "matrix-headroom".into()).await {
        failures.push(format!("headroom: could not load {b}: {e}"));
        return failures;
    }
    let loaded_ctx = crate::llm::current_ctx_size() as u64;
    if loaded_ctx == claimed_ctx {
        sink(format!("{b}: loaded at {loaded_ctx} - the grade and the loader agree"));
    } else {
        let line = format!("{b}: grade said {claimed_ctx}, loader gave {loaded_ctx}");
        sink(line.clone());
        failures.push(format!("headroom: {line}"));
    }
    // The helpers' verdict for this card with B loaded (taken 5 s after
    // ready by the app; asked again here so the line is this moment's).
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    let (e, u) = crate::llm::decide_helper_placement(app).await;
    sink(format!("helpers with {b} loaded: memory model on the {}, helper on the {}", e.word(), u.word()));
    crate::llm::stop_chat_server_for_maintenance(&state).await;
    failures
}

/// A rendezvous for a report anyone may read: Flowsta's own servers by
/// name, anything else (a person's own server) unnamed.
pub(crate) fn public_server_name(url: &str) -> String {
    let host = url.trim_start_matches("https://").trim_start_matches("http://").split(['/', ':']).next().unwrap_or("").trim_end_matches('.');
    if host == "flowsta.com" || host.ends_with(".flowsta.com") {
        host.to_string()
    } else {
        "your own server".to_string()
    }
}

/// How long ago, in words a report reader takes in at a glance.
fn ago(secs_then: u64, now: u64) -> String {
    let d = now.saturating_sub(secs_then);
    if d < 120 { format!("{d} s ago") } else if d < 7200 { format!("{} min ago", d / 60) } else if d < 172_800 { format!("{} h ago", d / 3600) } else { format!("{} days ago", d / 86_400) }
}

/// What counts as broken in the Devices section: only what is plainly
/// wrong on THIS machine. Another device being asleep or off is a fact,
/// not a failure.
pub(crate) fn devices_failures(labels_read: bool, unlocked: bool, ai_count: usize, holds_ais: bool, holds_manifest: bool) -> Vec<String> {
    let mut f = Vec::new();
    if labels_read && unlocked && ai_count > 0 && !holds_ais && !holds_manifest {
        f.push(format!("devices: the Vault holds no backup of your {ai_count} AI(s) - they cannot reach your other devices"));
    }
    f
}

/// The multi-device picture on this machine, read only: the Vault, what
/// this device has backed up and when, each other device's copies, the
/// last check for their changes, and the rendezvous keeper.
pub async fn leg_devices(app: &AppHandle, sink: Sink<'_>) -> Vec<String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let vault = crate::flowsta::find_vault().await;
    let Some(port) = vault.port else {
        sink("Vault: not found on this computer (this device keeps everything to itself)".into());
        return Vec::new();
    };
    sink(format!("Vault: on port {port}, {}", if vault.unlocked { "unlocked" } else { "locked (backups wait for it)" }));
    // Which identity the lines below describe - by count, never by name: the
    // report is made to be shared (10-20: the Vault had been unlocked as
    // another identity, and its lines read as faults).
    let identities = crate::profile::device_root(app)
        .ok()
        .and_then(|r| std::fs::read_dir(crate::profile::profiles_dir(&r)).ok())
        .map(|d| d.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0);
    sink(format!(
        "identity: the one unlocked in the Vault now{}",
        if identities > 1 { format!(" ({identities} identities have used Your Own AI on this computer)") } else { String::new() }
    ));
    let ai_count = crate::profile::root(app)
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("ai-data.json")).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["custom-ais"].as_array().map(|a| a.len()))
        .unwrap_or(0);
    // None: the Vault's list did not answer (locked or busy at that moment)
    // - not read, which is not the same as missing (10-20: a Windows run
    // failed "holds no backup" while the list simply did not answer).
    let labels = if vault.unlocked { crate::vault_escrow::own_backup_labels_read(port).await } else { None };
    let held = |label: &str| match &labels {
        Some(l) if l.contains(label) => "held",
        Some(_) => "missing",
        None => "not read",
    };
    let written = crate::vault_escrow::backup_write_times(app);
    let when = |label: &str| written.iter().find(|(l, _)| l == label).map(|(_, t)| format!("written {}", ago(*t, now))).unwrap_or_else(|| "not written by this version yet".into());
    sink(format!(
        "this device: {ai_count} AI(s) | small AI backup {} ({}) | full backup {} ({})",
        held(crate::vault_escrow::AIS_LABEL), when(crate::vault_escrow::AIS_LABEL),
        held(crate::vault_escrow::MANIFEST_LABEL), when(crate::vault_escrow::MANIFEST_LABEL),
    ));
    let (holds_ais, holds_manifest, labels_read) = match &labels {
        Some(l) => (l.contains(crate::vault_escrow::AIS_LABEL), l.contains(crate::vault_escrow::MANIFEST_LABEL), true),
        None => (false, false, false),
    };
    if vault.unlocked {
        match crate::vault_escrow::sibling_devices(port).await {
            Ok(devs) if devs.is_empty() => sink("other devices: none on this identity".into()),
            Ok(devs) => {
                for (n, d) in devs.into_iter().enumerate() {
                    let age = |s: &Option<String>| s.as_deref().and_then(|v| v.trim_matches('"').parse::<u64>().ok()).map(|t| ago(t, now)).unwrap_or_else(|| "none".into());
                    sink(format!(
                        "other device {} ({}) | copies here - AIs {}, full {} | network {}",
                        n + 1,
                        d.platform.as_deref().unwrap_or("?"),
                        age(&d.ais_at),
                        age(&d.manifest_at),
                        d.generation.as_deref().unwrap_or("?"),
                    ));
                }
            }
            Err(e) => sink(format!("other devices: not read ({e})")),
        }
    }
    match crate::config_sync::config_sync_status(app.clone()) {
        Some(st) => sink(format!("last check for other devices' changes: {} ({} updated, {} added)", ago(st.checked_at / 1000, now), st.updated, st.added)),
        None => sink("last check for other devices' changes: none yet on this profile".into()),
    }
    match crate::rendezvous_keeper::last_status() {
        Some(k) => sink(format!(
            "rendezvous: conductor started on {} | last round {} - {}/{} server(s) answered | {}{}",
            k.conductor.as_deref().map(public_server_name).unwrap_or_else(|| "(unknown)".into()),
            ago(k.at_secs, now),
            k.reached,
            k.servers,
            match k.stalled_min { Some(m) => format!("COVERING for a stalled registration ({m} min)"), None => "registering on its own".into() },
            if k.put_elsewhere > 0 { " | listed on a fallback too" } else { "" },
        )),
        None => sink("rendezvous: the keeper has not run yet (it starts about a minute after the conductor)".into()),
    }
    let failures = devices_failures(labels_read, vault.unlocked, ai_count, holds_ais, holds_manifest);
    for f in &failures {
        sink(format!("FAIL: {f}"));
    }
    failures
}

/// Every model here graded for agent work, asked for ONE named tool the way
/// a program asks (the OpenAI object form, normalized as :11435 does),
/// answers with that tool call. Each runs on its own test server, as the
/// other legs do: the matrix unloads the app's model when it starts, so
/// "the loaded model" is never there (10-20, the first run skipped).
pub async fn leg_tool_call(bin: &Path, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let mut tried = 0;
    for name in gguf_files(dir) {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let path = dir.join(&name);
        let Ok(meta) = crate::gguf::read_meta(&path) else { continue };
        if meta.is_embedding() {
            continue;
        }
        let grade = crate::model_caps::agent_caps(&name);
        if grade < 6 {
            continue;
        }
        tried += 1;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let free_now = crate::llm::grading_memory_bytes();
        if free_now > 0 && free_now < load_room_needed(size, false) {
            sink(format!("{name}: not tried - {:.1} GB of memory free", free_now as f64 / 1024f64.powi(3)));
            continue;
        }
        let free = free_vram_gb(bin);
        let ctx = 8192;
        let mut args: Vec<String> = vec![
            "--port".into(), bench_port().to_string(),
            "--host".into(), "127.0.0.1".into(),
            "--no-webui".into(), "--reasoning".into(), "off".into(),
            "--ctx-size".into(), ctx.to_string(),
            "--fit".into(), "off".into(),
            "--model".into(), name.clone(),
        ];
        if let Some(n) = moe_decision(&meta, size, ctx, free).filter(|n| *n > 0) {
            args.push("--n-cpu-moe".into());
            args.push(n.to_string());
        }
        if meta.surfaces_special_tokens() {
            args.push("--special".into());
        }
        let _guard = match spawn_bench_server(bin, dir, args).await {
            Ok(g) => g,
            Err(e) => {
                sink(format!("{name}: FAIL - did not load: {e}"));
                failures.push(format!("tool call: {name} did not load"));
                continue;
            }
        };
        let mut body = serde_json::json!({
            "model": name,
            "messages": [{"role": "user", "content": "Record that the build passed."}],
            "tools": [{"type": "function", "function": {
                "name": "emit",
                "description": "Record a status line.",
                "parameters": {"type": "object", "properties": {"status": {"type": "string"}}, "required": ["status"]}
            }}],
            "tool_choice": {"type": "function", "function": {"name": "emit"}},
            "max_tokens": 1024,
            "stream": false,
        });
        crate::inference_server::normalize_tool_choice(&mut body);
        let started = std::time::Instant::now();
        let called = match crate::llm::local_http()
            .post(format!("http://127.0.0.1:{}/v1/chat/completions", bench_port()))
            .json(&body)
            .timeout(std::time::Duration::from_secs(180))
            .send()
            .await
        {
            Ok(r) => r
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v["choices"][0]["message"]["tool_calls"][0]["function"]["name"].as_str().map(str::to_string)),
            Err(e) => {
                sink(format!("{name}: FAIL - no answer: {e}"));
                failures.push(format!("tool call: {name} did not answer"));
                continue;
            }
        };
        match called.as_deref() {
            Some("emit") => sink(format!("{name}: ok - called emit ({} ms)", started.elapsed().as_millis())),
            other => {
                let what = other.map(|n| format!("called {n}")).unwrap_or_else(|| "answered in words".into());
                sink(format!("{name}: FAIL - asked for emit, {what}"));
                failures.push(format!("tool call: {name} {what} when asked for one named tool"));
            }
        }
    }
    if tried == 0 {
        sink("no model here is graded for agent work - nothing to ask".into());
    }
    failures
}

pub async fn leg_routing(app: &AppHandle, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    crate::router::EMBED_PAUSES.store(0, Ordering::SeqCst);
    let reindex_at_start = crate::corpus::reembed_running();
    let failures = leg_routing_inner(app, dir, sink).await;
    let paused = crate::router::EMBED_PAUSES.load(Ordering::SeqCst);
    let reindexing = reindex_at_start || crate::corpus::reembed_running();
    if paused == 0 && !reindexing {
        return failures;
    }
    // The meaning checks did not get their embeddings this run: a library
    // re-index held the embedder, or it stopped answering. What failed for
    // that reason is not measured, not failed (10-20 Windows).
    sink(format!(
        "note: the memory model was {} during this leg ({} pause{}) - the meaning-based checks were not measured; run again when it is free",
        if reindexing { "busy re-indexing your document library" } else { "not answering in time" },
        paused,
        if paused == 1 { "" } else { "s" }
    ));
    failures
        .into_iter()
        .filter(|f| !(f.starts_with("fresh gate") || f.starts_with("health gate") || (f.starts_with("routing: ") && !f.starts_with("routing: stopped") && !f.starts_with("routing: think"))))
        .collect()
}

async fn leg_routing_inner(app: &AppHandle, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let table: serde_json::Value = match serde_json::from_str(include_str!("../route-battery.json")) {
        Ok(v) => v,
        Err(e) => {
            sink(format!("FAIL: battery table unreadable: {e}"));
            return vec![format!("routing: battery table unreadable: {e}")];
        }
    };
    let queries = table["queries"].as_array().cloned().unwrap_or_default();
    // Environment: what the numbers below can and cannot exercise.
    let share = crate::router::current_share(app);
    let not_entitled = crate::router::known_not_entitled(app);
    let catalog = crate::flowsta::list_online_models().await.ok();
    let everyday_listed = catalog.as_ref().is_some_and(|m| {
        m.iter().any(|x| x.id.strip_prefix("online:").unwrap_or(&x.id) == crate::router::DEFAULT_EVERYDAY.strip_prefix("online:").unwrap_or(crate::router::DEFAULT_EVERYDAY))
    });
    let embeddings = dir.join(crate::router::EMBED_MODEL).exists();
    let helper = dir.join(HELPER_FILE).exists();
    sink(format!(
        "dial: {share} | online catalog: {} | everyday default listed: {} | entitled: {} | embedding model: {} | helper model: {}",
        catalog.as_ref().map(|m| format!("{} models", m.len())).unwrap_or_else(|| "unreachable (online rungs not exercised)".into()),
        if everyday_listed { "yes" } else { "no" },
        if not_entitled { "no" } else { "yes or unknown" },
        if embeddings { "yes" } else { "no (semantic gates off - ordinary questions stay on the device)" },
        if helper { "yes" } else { "no (difficulty unknown)" },
    ));
    // The semantic gates on their own, before the routing verdicts: how the
    // freshness and health gates did on the battery's labeled buckets with
    // THIS machine's memory component (Eric 2026-10-12: "the web/recent and
    // the report classifier in the matrix"). Follow-ups and long turns are
    // not single messages and sit out; the health bucket is the medical
    // gate's yes and the freshness gate's no, the fresh buckets the reverse.
    {
        let single: Vec<&serde_json::Value> = queries
            .iter()
            .filter(|q| !matches!(q["bucket"].as_str().unwrap_or(""), "followup" | "long_turn"))
            .collect();
        let (mut fresh_hit, mut fresh_yes, mut fresh_false, mut fresh_no) = (0u32, 0u32, 0u32, 0u32);
        let (mut fresh_kw_hit, mut fresh_kw_yes) = (0u32, 0u32);
        let (mut med_hit, mut med_yes, mut med_false, mut med_no) = (0u32, 0u32, 0u32, 0u32);
        let mut semantic_ran = false;
        let mut misses: Vec<String> = Vec::new();
        for q in &single {
            if cancelled() {
                break;
            }
            let id = q["id"].as_str().unwrap_or("?");
            let bucket = q["bucket"].as_str().unwrap_or("?");
            let text = q["q"].as_str().unwrap_or("");
            let (fresh, ran_f) = crate::router::fresh_gate_preview(app, text).await;
            let (medical, ran_m) = crate::router::medical_check(app, text, None, crate::router::MEDICAL_MARGIN).await;
            semantic_ran |= ran_f || ran_m;
            let is_fresh = bucket.starts_with("fresh_");
            if is_fresh {
                fresh_yes += 1;
                if fresh { fresh_hit += 1 } else { misses.push(format!("fresh gate missed {id}: {text}")) }
                if bucket == "fresh_keyword" {
                    fresh_kw_yes += 1;
                    if fresh { fresh_kw_hit += 1 }
                }
            } else {
                fresh_no += 1;
                if fresh { fresh_false += 1; misses.push(format!("fresh gate false hit {id} ({bucket}): {text}")) }
            }
            if bucket == "health" {
                med_yes += 1;
                if medical { med_hit += 1 } else { misses.push(format!("health gate missed {id}: {text}")) }
            } else {
                med_no += 1;
                if medical { med_false += 1; misses.push(format!("health gate false hit {id} ({bucket}): {text}")) }
            }
        }
        sink(format!(
            "gates ({}): fresh caught {fresh_hit}/{fresh_yes} (keyword bucket {fresh_kw_hit}/{fresh_kw_yes}, paraphrase {}/{}) | fresh false hits {fresh_false}/{fresh_no} | health caught {med_hit}/{med_yes} | health false hits {med_false}/{med_no}",
            if semantic_ran { "semantic stage on" } else { "keyword stage only - no memory component" },
            fresh_hit - fresh_kw_hit,
            fresh_yes - fresh_kw_yes,
        ));
        for m in misses.iter().take(12) {
            sink(format!("  {m}"));
        }
        if misses.len() > 12 {
            sink(format!("  ... {} more", misses.len() - 12));
        }
        // With the semantic stage on, the gates are a promise: more than a
        // fifth of fresh questions kept home, or more than one in twenty
        // ordinary questions sent to the web / flagged as health, is a fail.
        if semantic_ran {
            if fresh_yes > 0 && fresh_hit * 5 < fresh_yes * 4 {
                failures.push(format!("fresh gate caught only {fresh_hit} of {fresh_yes}"));
            }
            if fresh_no > 0 && fresh_false * 20 > fresh_no {
                failures.push(format!("fresh gate false hits {fresh_false} of {fresh_no}"));
            }
            if med_yes > 0 && med_hit * 5 < med_yes * 4 {
                failures.push(format!("health gate caught only {med_hit} of {med_yes}"));
            }
            if med_no > 0 && med_false * 20 > med_no {
                failures.push(format!("health gate false hits {med_false} of {med_no}"));
            }
        }
    }
    let online_possible = catalog.is_some() && !not_entitled;
    let picks = crate::router::OnlinePicks::from_store(app);
    let side_of = |model: &str| if model.starts_with("online:") { "online" } else if model.starts_with("external:") { "server" } else { "device" };
    // tallies: (mode, bucket, dial) -> (online, n, fails)
    let mut tally: std::collections::BTreeMap<(String, String, String), (u32, u32, u32)> = std::collections::BTreeMap::new();
    let mut listed_fails = 0usize;
    let mut think_ok = 0u32;
    let mut think_bad = 0u32;
    let mut decided = 0u32;
    let started = std::time::Instant::now();
    crate::router::EMBED_MS.store(0, std::sync::atomic::Ordering::Relaxed);
    crate::fit::ASSESS_MS.store(0, std::sync::atomic::Ordering::Relaxed);
    // Progress every 100 decisions, and a ceiling: a machine where every
    // embed stalls must still hand back a (partial) table.
    const ROUTING_LEG_CAP: std::time::Duration = std::time::Duration::from_secs(25 * 60);
    let mut capped = false;
    'sweep: for mode in ["offline", "online-offline"] {
        for dial in ["frontier", "balanced", "local"] {
            for q in &queries {
                if cancelled() {
                    capped = true;
                    break 'sweep;
                }
                let id = q["id"].as_str().unwrap_or("?");
                let bucket = q["bucket"].as_str().unwrap_or("?");
                let text = q["q"].as_str().unwrap_or("");
                let task = q["task"].as_str().unwrap_or("general");
                let difficulty = q["difficulty"].as_str().unwrap_or("easy");
                let expected = q["expect"][mode][dial].as_str().unwrap_or("either");
                let turn_tokens = if bucket == "long_turn" { Some(200_000u32) } else { None };
                let first = crate::router::route_dry(app, mode, text, dial, task, difficulty, "balanced", &picks, false, false, turn_tokens, &crate::router::PrevTurn::default()).await;
                // Follow-up pairs: the second turn must inherit the first's side.
                let (result, expected_side): (Result<crate::router::RouteResult, String>, String) = if let Some(follow) = q["follow"].as_str() {
                    match &first {
                        Ok(r1) => {
                            let prev = crate::router::PrevTurn { side: Some(side_of(&r1.model).to_string()), task: Some(task.to_string()), model: Some(r1.model.clone()), vec: None, web_off: false };
                            let r2 = crate::router::route_dry(app, mode, follow, dial, "general", "easy", "balanced", &picks, false, false, None, &prev).await;
                            let want = if q["firstBucket"].as_str() == Some("health") { "device".to_string() } else { side_of(&r1.model).to_string() };
                            (r2, want)
                        }
                        Err(e) => (Err(e.clone()), "either".into()),
                    }
                } else {
                    (first, expected.to_string())
                };
                decided += 1;
                if decided % 100 == 0 {
                    let (embed_ms, fit_ms) = (
                        crate::router::EMBED_MS.load(std::sync::atomic::Ordering::Relaxed),
                        crate::fit::ASSESS_MS.load(std::sync::atomic::Ordering::Relaxed),
                    );
                    sink(format!(
                        "  ... {decided} decisions, {:.0} s (embedding {:.0} s, fit {:.0} s)",
                        started.elapsed().as_secs_f64(),
                        embed_ms as f64 / 1000.0,
                        fit_ms as f64 / 1000.0
                    ));
                }
                if started.elapsed() > ROUTING_LEG_CAP {
                    capped = true;
                    break 'sweep;
                }
                let entry = tally.entry((mode.into(), bucket.into(), dial.into())).or_insert((0, 0, 0));
                entry.1 += 1;
                let (side, model, reason, think) = match &result {
                    Ok(r) => (side_of(&r.model), r.model.clone(), r.reason.clone(), r.think),
                    Err(e) => ("refused", String::new(), e.clone(), None),
                };
                if side == "online" {
                    entry.0 += 1;
                }
                // Judge: the offline promise, the health promise, the dial.
                let fail: Option<String> = if side == "refused" {
                    if mode == "offline" || bucket == "health" { None } else { Some("refused (no model)".into()) }
                } else if mode == "offline" && (side == "online" || reason.to_lowercase().contains("online")) {
                    Some("OFFLINE PROMISE BROKEN".into())
                } else if expected_side == "either" {
                    None
                } else if expected_side == "online" && !online_possible {
                    None // cannot be exercised here; the environment line says so
                } else if expected_side == "online" && !embeddings && bucket != "fresh_keyword" && bucket != "long_turn" {
                    None // the semantic gates are off on this machine; the environment line says so
                } else if expected_side != side {
                    // A device answer that says why (health check could not run,
                    // catalog unavailable) is the router being honest, not wrong.
                    // "As good for this question" is the strictly-better rule:
                    // it fires only when the registry scores the model that
                    // answers here above the Everyday model on that task (a
                    // coding specialist on the 4060 Ti box kept code home under
                    // Online-first - correct by the rule). The dev battery
                    // stays strict; the field leg accepts it.
                    if side == "device" && (reason.contains("could not run") || reason.contains("unavailable") || reason.contains("as good for this question")) { None } else { Some(format!("expected {expected_side}, got {side}")) }
                } else if bucket == "long_turn" && side == "online" && !reason.contains("too long") {
                    Some("went online without the 'too long' reason".into())
                } else {
                    None
                };
                if let Some(f) = fail {
                    entry.2 += 1;
                    if listed_fails < 25 {
                        sink(format!("  FAIL {id} | {mode} | {dial} | {f} | {model} | {reason}"));
                    }
                    listed_fails += 1;
                    failures.push(format!("routing: {id} {mode}/{dial}: {f}"));
                }
                if let (Some(want), Some(got)) = (q["think"].as_bool(), think) {
                    if want == got { think_ok += 1 } else { think_bad += 1 }
                }
            }
        }
    }
    sink(format!("{:<15} {:<17} {:<9} {:<9} {:>7} {:>5}", "mode", "bucket", "dial", "", "online", "fails"));
    for ((mode, bucket, dial), (online, n, fails)) in &tally {
        sink(format!("{:<15} {:<17} {:<9} {:<9} {:>3}/{:<3} {:>5}", mode, bucket, dial, "", online, n, fails));
    }
    if capped {
        if cancelled() {
            sink(format!("cancelled at {decided} decisions - the table above is partial"));
        } else {
            sink(format!("STOPPED after 25 min at {decided} decisions - the table above is partial (a slow or stalled embed server? see the app log)"));
            failures.push(format!("routing: stopped after 25 min at {decided} decisions"));
        }
    }
    sink(format!("think verdicts: {think_ok} as expected, {think_bad} not ({decided} decisions in {:.0} s)", started.elapsed().as_secs_f64()));
    if think_bad > think_ok {
        failures.push(format!("routing: think verdicts off ({think_bad} of {})", think_ok + think_bad));
    }
    if listed_fails > 25 {
        sink(format!("  ... {} more failures not listed", listed_fails - 25));
    }
    failures
}

fn engine_binary(app: &AppHandle) -> Result<(PathBuf, &'static str), String> {
    if let crate::engine::Backend::Cuda = crate::engine::active_backend(app) {
        if let Some(bin) = crate::engine::cuda_engine_binary(app) {
            return Ok((bin, "CUDA"));
        }
    }
    if cfg!(debug_assertions) {
        let triple = if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc.exe"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" }
        } else {
            "x86_64-unknown-linux-gnu"
        };
        let bin = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join(format!("llama-server-{triple}"));
        if bin.exists() {
            return Ok((bin, "bundled"));
        }
    }
    let bin = crate::resolve_sidecar_bin("llama-server");
    if bin.is_absolute() && bin.exists() {
        Ok((bin, "bundled"))
    } else {
        Err("engine binary not found beside the app".into())
    }
}

#[tauri::command]
pub async fn matrix_cancel() -> Result<(), String> {
    MATRIX_CANCEL.store(true, Ordering::SeqCst);
    crate::tuning::tune_cancel_flag_set(true);
    Ok(())
}

/// Run the whole matrix and write one report file; returns its path. The
/// chat model is unloaded first (same maintenance stop the engine switch
/// uses); progress streams as `matrix-progress` events, one line each.
/// Holds the computer awake (not the screen) while it lives: `caffeinate`
/// on macOS, `systemd-inhibit` on Linux, both built in. Windows: not yet
/// (a system call this build adds later). Best effort - a missing tool
/// leaves the person's own sleep settings in charge.
struct KeepAwake(Option<std::process::Child>);

impl KeepAwake {
    fn start() -> Self {
        let child = if cfg!(target_os = "macos") {
            std::process::Command::new("caffeinate")
                .args(["-i", "-w", &std::process::id().to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        } else if cfg!(target_os = "linux") {
            // `tail --pid` ends when the app does, however the app ends (10-20:
            // the app died mid-run and a `sleep infinity` inhibitor outlived it,
            // keeping the computer awake).
            std::process::Command::new("systemd-inhibit")
                .args(["--what=idle:sleep", "--who=Your Own AI", "--why=Running the truth matrix", "--mode=block", "tail", &format!("--pid={}", std::process::id()), "-f", "/dev/null"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        } else {
            None
        };
        if child.is_some() {
            log::info!("[matrix] keeping the computer awake while the matrix runs");
        }
        KeepAwake(child)
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[tauri::command]
pub async fn matrix_run(
    app: AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
    stamp: String,
    legs: Option<Vec<String>>,
) -> Result<String, String> {
    // A subset of legs (dev trigger / a targeted rerun); None = all.
    let legs: Vec<String> = legs.unwrap_or_default();
    let want = |name: &str| legs.is_empty() || legs.iter().any(|l| l == name);
    if MATRIX_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("The matrix is already running".into());
    }
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            MATRIX_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _running = Running;
    // A long run must not depend on the person's sleep settings (10-20: the
    // Mac went to sleep mid-run and the matrix sat there).
    let _awake = KeepAwake::start();
    MATRIX_CANCEL.store(false, Ordering::SeqCst);
    crate::tuning::tune_cancel_flag_set(false);

    let (bin, backend) = engine_binary(&app)?;
    let dir = crate::llm::get_models_dir(&app)?;
    let logs = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let stamp: String = stamp
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect();
    let path = logs.join(format!(
        "matrix-report-{}.txt",
        if stamp.is_empty() { "run" } else { &stamp }
    ));

    // The model in use before the matrix takes the card: the memory leg
    // grades it first (it is the one the slot would hand memory work to).
    let loaded_before: Option<String> = state.current_model.lock().await.clone();
    // The whole card belongs to the matrix: unload the chat model the safe
    // way (the same stop the engine switch uses - never mid-request).
    crate::llm::stop_chat_server_for_maintenance(&state).await;

    let report = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink_app = app.clone();
    let sink_report = report.clone();
    let sink_path = path.clone();
    // Every line is scrubbed of the home directory before it is written:
    // the report is made to be sent to other people, and an engine error
    // echoing an absolute path must not carry the OS account name.
    let home_prefix = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let sink = move |line: String| {
        let line = if home_prefix.len() > 3 {
            line.replace(&home_prefix, "~")
        } else {
            line
        };
        log::info!("[matrix] {line}");
        let _ = sink_app.emit("matrix-progress", &line);
        let mut r = sink_report.lock().unwrap();
        r.push_str(&line);
        r.push('\n');
        // Rewritten per line: the report survives a crash mid-run.
        // A UTF-8 byte-order mark: Windows viewers otherwise guess the
        // code page and a model's curly apostrophe reads as "â" (both
        // pasted Windows reports, 09-04 and 09-06). Our own readers skip it.
        let _ = std::fs::write(&sink_path, format!("\u{feff}{r}").as_bytes());
    };
    let sink: Sink<'_> = &sink;

    let ggufs = gguf_files(&dir);
    let mlxs = mlx_artifact_dirs(&dir);
    let gpus = {
        let out = bench_command(&bin, &["--list-devices".into()]).output().ok();
        let text = out
            .map(|o| {
                let mut t = String::from_utf8_lossy(&o.stdout).into_owned();
                t.push_str(&String::from_utf8_lossy(&o.stderr));
                t
            })
            .unwrap_or_default();
        let devices = crate::llm::parse_gpu_devices(&text);
        if devices.is_empty() {
            "none detected - CPU only".to_string()
        } else {
            devices
                .iter()
                .map(|d| {
                    format!(
                        "{} ({} MiB free{})",
                        d.name,
                        d.free_mib,
                        if d.integrated { ", integrated" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    sink("Your Own AI - truth matrix report".into());
    sink(format!(
        "app {} | {} {} | engine {}: {}",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH,
        backend,
        bin.file_name().and_then(|n| n.to_str()).unwrap_or("?")
    ));
    sink(format!("GPUs: {gpus}"));
    // The report is made to be sent to other people: the models path is
    // shown with the home directory folded to ~ so the OS account name
    // never rides along.
    let dir_shown = {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let d = dir.display().to_string();
        if !home.is_empty() && d.starts_with(&home) {
            format!("~{}", &d[home.len()..])
        } else {
            d
        }
    };
    sink(format!(
        "RAM: {:.1} GB | models: {} ({} GGUF, {} MLX)",
        total_ram_gb(),
        dir_shown,
        ggufs.len(),
        mlxs.len()
    ));
    sink(String::new());

    let mut failures: Vec<String> = Vec::new();

    if want("components") {
        sink("== Components (the small servers answer) ==".into());
        failures.extend(leg_components(&app, &dir, sink).await);
        sink(String::new());
    }

    if !cancelled() && want("devices") {
        sink("== Devices (this machine and your others) ==".into());
        failures.extend(leg_devices(&app, sink).await);
        sink(String::new());
    }

    if !cancelled() && want("toolcall") {
        sink("== Tool call (one named tool, asked the way a program asks) ==".into());
        failures.extend(leg_tool_call(&bin, &dir, sink).await);
        sink(String::new());
    }

    if want("fit") {
        sink("== Fit truth (claim vs real load) ==".into());
        failures.extend(leg_fit_truth(Some(&app), &bin, &dir, &[], sink).await);
        sink(String::new());
    }

    // Grades must not move with what the app has loaded: the badge a user
    // reads while a model is running has to be the badge for an empty
    // card (the grader hands the loaded model's footprint back). Load the
    // largest Green model through the app's own path and grade everything
    // again. (Dev box 09-03: "Too large" on models running at full speed,
    // while a bench server held the card.)
    if !cancelled() && want("fit") {
        sink("== Grade drift (grades with a model loaded vs the card empty) ==".into());
        let baseline = crate::fit::assess_fresh(&app).await;
        let pick = baseline
            .iter()
            .filter(|f| f.fit == crate::fit::Fit::Green)
            .max_by_key(|f| std::fs::metadata(dir.join(&f.name)).map(|m| m.len()).unwrap_or(0))
            .map(|f| f.name.clone());
        match pick {
            None => sink("no Green model to load - skipped".into()),
            Some(name) => {
                match crate::llm::start_llama_server(app.clone(), state.clone(), Some(name.clone()), false).await {
                    Err(e) => sink(format!("{name}: could not load for the check ({e}) - skipped")),
                    Ok(()) => {
                        // Let the driver settle before reading free memory.
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        let loaded = crate::fit::assess_fresh(&app).await;
                        let mut drift = 0;
                        for b in &baseline {
                            if let Some(l) = loaded.iter().find(|l| l.name == b.name) {
                                if l.fit != b.fit {
                                    drift += 1;
                                    sink(format!("  {}: {:?} with the card empty, {:?} while {} is loaded", b.name, b.fit, l.fit, name));
                                }
                            }
                        }
                        if drift == 0 {
                            sink(format!("  {} models graded the same with {} loaded - OK", baseline.len(), name));
                        } else {
                            failures.push(format!("grade drift: {drift} grade(s) changed while {name} was loaded"));
                        }
                        crate::llm::stop_chat_server_for_maintenance(&state).await;
                    }
                }
            }
        }
        sink(String::new());
    }

    if !cancelled() && want("chat") {
        sink("== Chat format (words, no channel markers; think switch) ==".into());
        failures.extend(leg_chat_format(&bin, &dir, sink).await);
        sink(String::new());
    }

    // Sampling proof on the fastest loader (smallest file); tune bench on
    // the largest MoE when one exists (that's where the arms do real work),
    // else the same small model.
    let smallest = ggufs
        .iter()
        .filter(|n| {
            crate::gguf::read_meta(&dir.join(n.as_str())).map(|m| !m.is_embedding()).unwrap_or(false)
        })
        .min_by_key(|n| std::fs::metadata(dir.join(n.as_str())).map(|m| m.len()).unwrap_or(u64::MAX))
        .cloned();
    if !cancelled() && want("sampling") {
        if let Some(model) = &smallest {
            sink(format!("== Sampling truth ({model}) =="));
            match leg_sampling(&bin, &dir, model, sink).await {
                Ok(()) => sink("ok: greedy deterministic, high temperature diverges".into()),
                Err(e) => {
                    sink(format!("FAIL: {e}"));
                    failures.push(format!("sampling: {e}"));
                }
            }
            sink(String::new());
        }
    }
    if !cancelled() && want("memory") {
        sink("== Memory grades (extraction precision/recall, consolidation rubric) ==".into());
        for f in leg_memory_grade(&app, &bin, &dir, loaded_before.as_deref(), sink).await {
            failures.push(format!("memory: {f}"));
        }
        sink(String::new());
    }
    if !cancelled() && want("quality") {
        sink("== Answer quality (eight plain questions, a rubric, per model; registry score beside it) ==".into());
        for f in leg_quality(&bin, &dir, loaded_before.as_deref(), sink).await {
            failures.push(format!("quality: {f}"));
        }
        sink(String::new());
    }
    if !cancelled() && want("modes") {
        sink("== Report/code mode classifier (the app's prompt and pre-gate, per model) ==".into());
        for f in leg_turn_mode(&bin, &dir, loaded_before.as_deref(), sink).await {
            failures.push(format!("modes: {f}"));
        }
        sink(String::new());
    }
    if !cancelled() && want("tune") {
        let tune_model = ggufs
            .iter()
            .filter(|n| {
                crate::gguf::read_meta(&dir.join(n.as_str())).map(|m| m.is_moe()).unwrap_or(false)
            })
            .max_by_key(|n| std::fs::metadata(dir.join(n.as_str())).map(|m| m.len()).unwrap_or(0))
            .cloned()
            .or(smallest);
        if let Some(model) = tune_model {
            sink(format!("== Tune bench ({model}) =="));
            match leg_tune_bench(&bin, &dir, &model, sink).await {
                Ok(()) => sink("ok: arms load and time".into()),
                Err(e) => {
                    sink(format!("FAIL: {e}"));
                    failures.push(format!("tune bench: {e}"));
                }
            }
            sink(String::new());
        }
    }

    if !cancelled() && want("mlx") {
        sink("== MLX ==".into());
        failures.extend(leg_mlx(&app, &dir, sink).await);
        sink(String::new());
    }

    if !cancelled() && want("routing") {
        sink("== Routing (decide-only, no credits; the battery on this machine's models) ==".into());
        failures.extend(leg_routing(&app, &dir, sink).await);
        sink(String::new());
    }

    if !cancelled() && want("headroom") {
        sink("== Headroom (the grade's \"runs at\" vs the loader, on one figure) ==".into());
        failures.extend(leg_headroom(&app, state.clone(), &dir, sink).await);
        sink(String::new());
    }

    if cancelled() {
        sink("MATRIX CANCELLED".into());
    } else if failures.is_empty() {
        sink("MATRIX COMPLETE - all checks passed".into());
    } else {
        sink(format!("MATRIX COMPLETE - {} failure(s):", failures.len()));
        for f in &failures {
            sink(format!("  {f}"));
        }
    }
    let _ = app.emit("matrix-done", path.display().to_string());
    Ok(path.display().to_string())
}

#[cfg(test)]
mod fit_room_tests {
    use super::load_room_needed;
    const GB: u64 = 1024 * 1024 * 1024;
    #[test]
    fn a_test_load_needs_the_loader_gate_and_a_too_big_one_its_whole_file_on_top() {
        assert_eq!(load_room_needed(4 * GB, false), 2 * GB + 4 * GB / 10);
        assert_eq!(load_room_needed(4 * GB, true), 2 * GB + 4 * GB / 10 + 4 * GB);
        // 10-20: 3.7 GB free - a 3.2 GB model that fits is tried, one graded
        // too big is not.
        let free = 37 * GB / 10;
        assert!(free >= load_room_needed(32 * GB / 10, false));
        assert!(free < load_room_needed(32 * GB / 10, true));
    }
}

#[cfg(test)]
mod devices_leg_tests {
    use super::{ago, devices_failures};
    #[test]
    fn only_a_vault_holding_nothing_of_your_ais_is_a_failure() {
        assert!(devices_failures(true, true, 3, false, false).len() == 1, "AIs, an unlocked Vault, nothing held");
        assert!(devices_failures(true, true, 3, true, false).is_empty(), "the small object alone is enough");
        assert!(devices_failures(true, true, 3, false, true).is_empty(), "an older app's full backup alone too");
        assert!(devices_failures(true, false, 3, false, false).is_empty(), "locked: backups wait, not broken");
        assert!(devices_failures(false, false, 3, false, false).is_empty(), "no Vault: this device keeps to itself");
        assert!(devices_failures(false, true, 7, false, false).is_empty(), "the list did not answer: not read, not missing (10-20 Windows)");
        assert!(devices_failures(true, true, 0, false, false).is_empty(), "no AIs, nothing to hold");
    }
    #[test]
    fn only_flowstas_own_servers_are_named_in_a_report() {
        use super::public_server_name;
        assert_eq!(public_server_name("https://bootstrap.flowsta.com"), "bootstrap.flowsta.com");
        assert_eq!(public_server_name("https://bootstrap.flowsta.com./"), "bootstrap.flowsta.com");
        assert_eq!(public_server_name("https://node1.flowsta.com"), "node1.flowsta.com");
        assert_eq!(public_server_name("https://rendezvous.example-family.net"), "your own server");
        assert_eq!(public_server_name("https://notflowsta.com"), "your own server");
    }

    #[test]
    fn ages_read_at_a_glance() {
        assert_eq!(ago(1_000, 1_030), "30 s ago");
        assert_eq!(ago(1_000, 1_000 + 600), "10 min ago");
        assert_eq!(ago(1_000, 1_000 + 3 * 3600), "3 h ago");
        assert_eq!(ago(1_000, 1_000 + 3 * 86_400), "3 days ago");
    }
}

#[cfg(test)]
mod quality_leg_tests {
    use super::*;
    #[test]
    fn rubric_scores_facts_and_form_separately() {
        let sky = &QUALITY_GOLD[0];
        assert_eq!(score_quality(sky, "Sunlight is scattered by air molecules (Rayleigh scattering); shorter wavelengths scatter more, so we see blue.").0, 2);
        assert_eq!(score_quality(sky, "Because the ocean reflects onto it.").0, 1, "form fine, facts missing");
        assert_eq!(score_quality(sky, "I'm sorry, I cannot help with that.").0, 0);
        let train = &QUALITY_GOLD[2];
        assert_eq!(score_quality(train, "It arrives at 12:05.").0, 2);
        assert_eq!(score_quality(train, "It arrives at 11:65.").0, 1);
        let garden = &QUALITY_GOLD[4];
        assert_eq!(score_quality(garden, "1. Pick a sunny spot.\n2. Water in the morning.\n3. Feed the soil with compost.").0, 2);
        assert_eq!(score_quality(garden, "Sun, water and good soil matter most.").0, 1, "facts yes, no list");
        let rj = &QUALITY_GOLD[5];
        assert_eq!(score_quality(rj, "In Verona two feuding families keep Romeo and Juliet apart. They marry in secret with Friar Laurence's help. A duel leaves Tybalt dead and Romeo banished. Believing Juliet dead, Romeo takes poison and she kills herself with his dagger.").0, 2);
    }
    #[test]
    fn a_card_dragged_into_an_unrelated_answer_loses_the_facts_half() {
        let sky_with_card = QUALITY_GOLD.iter().find(|c| c.system_extra.is_some() && c.ask.starts_with("why is the sky")).unwrap();
        assert_eq!(score_quality(sky_with_card, "Rayleigh scattering: shorter wavelengths scatter more. By the way, your lease says the deposit is 1,325.").0, 1);
        assert_eq!(score_quality(sky_with_card, "Rayleigh scattering: shorter wavelengths scatter more, so the sky looks blue.").0, 2);
        let about = QUALITY_GOLD.iter().find(|c| c.ask.starts_with("what does my lease")).unwrap();
        assert_eq!(score_quality(about, "Your lease notes a deposit of 1,325 held in the approved scheme.").0, 2);
    }
    #[test]
    fn every_question_has_a_rubric_half() {
        for c in QUALITY_GOLD {
            assert!(!c.must.is_empty() || c.number.is_some(), "{} has no fact check", c.ask);
        }
    }
}

#[cfg(test)]
mod mode_leg_tests {
    #[test]
    fn every_report_and_code_request_in_the_gold_set_reaches_the_model() {
        // The free pre-gate must never swallow a real request: a REPORT or
        // CODE message with none of the keywords would be NEITHER by construction.
        for (m, want) in super::MODE_GOLD {
            if *want != "NEITHER" {
                assert!(super::mode_pregate_relevant(m), "pre-gate would skip a real request: {m}");
            }
        }
        // plain chat with none of the words never costs a model call
        assert!(!super::mode_pregate_relevant("any dinner ideas for tonight"));
        assert!(super::mode_pregate_relevant("write me a report on bees"));
    }
}

#[cfg(test)]
mod memory_grade_tests {
    use super::*;

    #[test]
    fn the_leg_grades_the_helper_the_loaded_model_and_the_strongest_up_to_four() {
        let all: Vec<String> = ["a.gguf", "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf", "b.gguf", "c.gguf", "d.gguf", "e.gguf"].iter().map(|s| s.to_string()).collect();
        let picked = memory_grade_candidates(&all, Some("d.gguf"), "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf");
        assert_eq!(picked.len(), 4);
        assert_eq!(picked[0], "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf");
        assert_eq!(picked[1], "d.gguf");
        let none_loaded = memory_grade_candidates(&all, None, "missing-helper.gguf");
        assert_eq!(none_loaded.len(), 4, "no helper on disk, nothing loaded: the strongest four");
    }

    #[test]
    fn extraction_scores_hits_by_predicate_and_phrase_and_negatives_by_silence() {
        let gold = &[("name_is", "sam"), ("works_on", "nurse")][..];
        let (h, e, g) = score_extraction(r#"[{"subject":"user","predicate":"name_is","value":"Sam","confidence":1},{"subject":"user","predicate":"works_on","value":"a nurse in Leeds","confidence":0.9},{"subject":"user","predicate":"likes","value":"tea","confidence":0.8}]"#, gold);
        assert_eq!((h, e, g), (2, 3, 2));
        assert_eq!(extraction_grade(2, 3, 2), 8, "f1 0.8");
        assert_eq!(score_extraction("[]", &[]), (0, 0, 0));
        assert_eq!(extraction_grade(0, 0, 0), 10, "silence on a non-fact is perfect");
        assert_eq!(score_extraction(r#"[{"subject":"user","predicate":"likes","value":"housing","confidence":0.9}]"#, &[]), (0, 1, 0));
        assert_eq!(extraction_grade(0, 1, 0), 0);
    }

    #[test]
    fn the_consolidation_rubric_rewards_the_kept_preference_merged_duplicates_and_no_inventions() {
        let good = "- Always run npm test before each commit\n- The API lives in src/api\n- Deploy with firebase deploy --only hosting\n- Keep answers short, I read on my phone\n- The database is Postgres on port 5433\n- Use conventional commit messages";
        assert_eq!(score_prose(good), 10);
        let dropped_pref = good.replace("- Keep answers short, I read on my phone\n", "");
        assert_eq!(score_prose(&dropped_pref), 6);
        let invented = format!("{good}\n- The team meets on Fridays at the pub");
        assert_eq!(score_prose(&invented), 4, "one invented line costs grounding and the length band");
        assert_eq!(score_prose(""), 0);
    }
}
