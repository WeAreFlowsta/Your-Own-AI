//! Dev builds only: write a file of conversations into the open profile as
//! ordinary records, with the dates the file gives.
//!
//! This is how a made-up history (`e2e/fixtures/memory/history.json`) gets
//! into a scratch profile for tests and measurements: the records are the
//! same shape the chat page writes, so every background job sees them as
//! conversations the person had. Nothing here runs in a release build; the
//! route that calls it answers 404 there.
//!
//! The file:
//!
//! ```json
//! { "conversations": [ { "id": "conv-b01", "ai": "Juno", "title": "...",
//!     "started_at": "2026-01-12T09:14:00Z",
//!     "messages": [ { "role": "user", "text": "...", "at": "2026-01-12T09:14:00Z" } ] } ] }
//! ```
//!
//! `ai` is a name the caller maps to one of the profile's AIs. Every
//! conversation needs its own start time: a start time names a
//! conversation in several places, and a second one with the same start
//! for the same AI is refused, never written beside the first.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize, Debug)]
pub(crate) struct Fixture {
    pub conversations: Vec<FixtureConversation>,
}

/// A moment in the file: a date and time as text, or microseconds since
/// 1970 as a number (what a record itself holds, kept exact).
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub(crate) enum When {
    Micros(i64),
    Text(String),
}

impl When {
    fn micros(&self) -> Option<i64> {
        match self {
            When::Micros(m) => Some(*m),
            When::Text(t) => crate::conversation_import::iso_to_micros(t),
        }
    }
}

#[derive(Deserialize, Debug)]
pub(crate) struct FixtureConversation {
    pub id: String,
    pub ai: String,
    pub title: String,
    pub started_at: When,
    /// What drove the conversation, when it was not the chat page (the
    /// name another program gave over the local port, for one). A history
    /// taken from real use carries these; a made-up one leaves it out.
    #[serde(default)]
    pub source: Option<String>,
    /// The model named on the records, when the file knows it.
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<FixtureMessage>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct FixtureMessage {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub at: Option<When>,
    /// The message's sequence number, when the file carries the one it had
    /// (two devices writing while apart can both take the same number).
    /// Left out: its place in the list.
    #[serde(default)]
    pub sequence: Option<u32>,
}

/// One of the profile's AIs, as the loader needs it.
#[derive(Clone, Debug)]
pub(crate) struct FixtureAi {
    /// The AI's agent key on this device, hex.
    pub agent: String,
    /// Its founding key, hex (the agent key itself for an AI made here).
    pub founding: String,
    pub name: String,
}

/// A conversation ready to write: who writes it, and the plaintext of its
/// record and of each message.
#[derive(Debug)]
pub(crate) struct Planned {
    pub fixture_id: String,
    pub ai: FixtureAi,
    pub started_at: i64,
    pub meta: serde_json::Value,
    pub messages: Vec<serde_json::Value>,
}

#[derive(Debug, Default, serde::Serialize, PartialEq)]
pub(crate) struct Loaded {
    pub conversations: usize,
    pub messages: usize,
    /// Already in the profile (same AI, same start time): left alone.
    pub skipped: usize,
    /// Fixture id -> the conversation's id in the profile.
    pub ids: HashMap<String, String>,
}

/// Turn the file into records to write. `ais` maps the file's AI names to
/// the profile's AIs; `existing` holds the start times each AI (by agent
/// key) already has. Refuses a file that would be ambiguous or wrong
/// before anything is written.
pub(crate) fn plan(fixture: &Fixture, ais: &HashMap<String, FixtureAi>, model: &str, existing: &HashMap<String, HashSet<i64>>) -> Result<(Vec<Planned>, usize), String> {
    let mut out = Vec::new();
    let mut skipped = 0usize;
    let mut seen: HashSet<(String, i64)> = HashSet::new();
    for c in &fixture.conversations {
        let ai = ais.get(&c.ai).ok_or_else(|| format!("{}: no AI is mapped to the name {:?}", c.id, c.ai))?;
        let started_at = c.started_at.micros().ok_or_else(|| format!("{}: started_at {:?} is not a date and time", c.id, c.started_at))?;
        if !seen.insert((ai.agent.clone(), started_at)) {
            return Err(format!("{}: a second conversation for {} starts at {}; every conversation needs its own start time", c.id, c.ai, started_at));
        }
        if c.messages.is_empty() {
            return Err(format!("{}: no messages", c.id));
        }
        if existing.get(&ai.agent).is_some_and(|s| s.contains(&started_at)) {
            skipped += 1;
            continue;
        }
        let meta = serde_json::json!({
            "ai_personality_id": ai.agent,
            "ai_personality_name": ai.name,
            "model_used": c.model.as_deref().unwrap_or(model),
            "started_at": started_at,
            "title": c.title,
            "source": c.source,
        });
        let mut messages = Vec::with_capacity(c.messages.len());
        let mut last = started_at;
        for (place, m) in c.messages.iter().enumerate() {
            let sequence = m.sequence.unwrap_or(place as u32);
            if m.role != "user" && m.role != "assistant" {
                return Err(format!("{} message {}: role {:?} is neither user nor assistant", c.id, sequence, m.role));
            }
            let timestamp = match &m.at {
                Some(at) => at.micros().ok_or_else(|| format!("{} message {}: at {:?} is not a date and time", c.id, sequence, at))?,
                // No time given: one second after the message before it.
                None => last + 1_000_000,
            };
            last = timestamp.max(last);
            messages.push(serde_json::json!({
                "role": m.role,
                "content": m.text,
                "sequence": sequence,
                "timestamp": timestamp,
                "model": c.model.as_deref().unwrap_or(model),
                "thinking": serde_json::Value::Null,
                "tokens": serde_json::Value::Null,
            }));
        }
        out.push(Planned { fixture_id: c.id.clone(), ai: ai.clone(), started_at, meta, messages });
    }
    Ok((out, skipped))
}

/// Write the planned conversations, one record after another, on the line
/// that takes each AI's writes.
pub(crate) async fn write(manager: &crate::holochain::HolochainManager, key: &[u8; 32], planned: &[Planned], skipped: usize) -> Result<Loaded, String> {
    let mut loaded = Loaded { skipped, ..Default::default() };
    for p in planned {
        let id = crate::conversation_import::start_imported(manager, &p.ai.agent, &p.ai.founding, key, &p.meta).await.map_err(|e| format!("{}: {e}", p.fixture_id))?;
        for (i, m) in p.messages.iter().enumerate() {
            crate::conversation_import::record_imported(manager, &p.ai.agent, key, &id, m).await.map_err(|e| format!("{} message {i}: {e}", p.fixture_id))?;
            loaded.messages += 1;
        }
        loaded.conversations += 1;
        loaded.ids.insert(p.fixture_id.clone(), id);
    }
    Ok(loaded)
}

/// Load a fixture file into the open profile. `names` maps the file's AI
/// names to the profile's AIs (by id or by name); a name with no entry is
/// looked up among the profile's AIs by its own name, then falls to `all`
/// when that is given. Lists, search and last-active times are told about
/// each record the way a live turn tells them.
pub(crate) async fn load_into_profile(app: &tauri::AppHandle, path: &std::path::Path, profile_ais: &[serde_json::Value], names: &HashMap<String, String>, all: Option<&str>, model: &str) -> Result<Loaded, String> {
    use tauri::Manager;
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let fixture: Fixture = serde_json::from_slice(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let hc = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>();
    let manager = hc.get()?;
    let key = manager.data_key()?;

    let find = |wanted: &str| -> Option<FixtureAi> {
        profile_ais.iter().find_map(|a| {
            let agent = a["agentPubKey"].as_str().or_else(|| a["id"].as_str()).filter(|k| !k.is_empty())?;
            let name = a["name"].as_str().unwrap_or("");
            (a["id"].as_str() == Some(wanted) || agent == wanted || name.eq_ignore_ascii_case(wanted)).then(|| FixtureAi {
                agent: agent.to_string(),
                founding: crate::commands_holochain::founding_key_for(app, agent),
                name: name.to_string(),
            })
        })
    };
    let mut ais: HashMap<String, FixtureAi> = HashMap::new();
    for c in &fixture.conversations {
        if ais.contains_key(&c.ai) {
            continue;
        }
        let wanted = names.get(&c.ai).map(String::as_str).unwrap_or(&c.ai);
        let found = find(wanted).or_else(|| if names.contains_key(&c.ai) { None } else { all.and_then(find) });
        match found {
            Some(ai) => {
                ais.insert(c.ai.clone(), ai);
            }
            None => return Err(format!("no AI in this profile for the name {:?} (map it with map=Name:ai, or give all=ai)", c.ai)),
        }
    }
    // What each AI already has, from its list cache (kept current by every
    // write, and readable on either line).
    let mut existing: HashMap<String, HashSet<i64>> = HashMap::new();
    for ai in ais.values() {
        let have = crate::conversation_cache::read_cache(app, &ai.agent).unwrap_or_default();
        existing.entry(ai.agent.clone()).or_default().extend(have.iter().map(|c| c.started_at));
    }
    let (planned, skipped) = plan(&fixture, &ais, model, &existing)?;
    let loaded = write(&manager, &key, &planned, skipped).await?;
    for p in &planned {
        let Some(id) = loaded.ids.get(&p.fixture_id) else { continue };
        let last = p.messages.last().and_then(|m| m["timestamp"].as_i64()).unwrap_or(p.started_at);
        crate::conversation_cache::append_to_cache(
            app,
            &p.ai.agent,
            crate::commands_holochain::ConversationInfo {
                hash: id.clone(),
                ai_personality_id: p.ai.agent.clone(),
                ai_personality_name: p.ai.name.clone(),
                model_used: model.to_string(),
                started_at: p.started_at,
                last_active_at: Some(last),
                title: p.meta["title"].as_str().map(str::to_string),
                source: None,
                agent_key: p.ai.agent.clone(),
            },
        );
        for m in &p.messages {
            crate::transcript_search::note_recorded(app, id, m["sequence"].as_u64().unwrap_or(0) as u32, m["role"].as_str().unwrap_or(""), m["content"].as_str().unwrap_or(""), m["timestamp"].as_i64().unwrap_or(0));
        }
    }
    log::info!("[fixture] {} conversation(s), {} message(s) written; {} already there", loaded.conversations, loaded.messages, loaded.skipped);
    Ok(loaded)
}

/// The stores the app's memory reads when it builds a prompt: the facts
/// about the person, and each AI's index of earlier turns. A file of these
/// puts a profile's memory in a known state before it is measured.
#[derive(Deserialize)]
pub(crate) struct MemoryStores {
    #[serde(default)]
    pub facts: Option<Vec<crate::memory::MemoryFact>>,
    #[serde(default)]
    pub recall: Vec<RecallStore>,
}

#[derive(Deserialize)]
pub(crate) struct RecallStore {
    /// The AI's name in the file (mapped like a conversation's).
    pub ai: String,
    pub entries: Vec<crate::transcript_memory::TranscriptEmbedding>,
}

/// The store id of the profile AI a file's name stands for.
fn store_id_for(profile_ais: &[serde_json::Value], names: &HashMap<String, String>, all: Option<&str>, name: &str) -> Result<String, String> {
    let find = |wanted: &str| {
        profile_ais.iter().find_map(|a| {
            let id = a["id"].as_str().filter(|i| !i.is_empty())?;
            (id == wanted || a["agentPubKey"].as_str() == Some(wanted) || a["name"].as_str().unwrap_or("").eq_ignore_ascii_case(wanted)).then(|| id.to_string())
        })
    };
    let wanted = names.get(name).map(String::as_str).unwrap_or(name);
    find(wanted)
        .or_else(|| if names.contains_key(name) { None } else { all.and_then(find) })
        .ok_or_else(|| format!("no AI in this profile for the name {name:?} (map it with map=Name:ai, or give all=ai)"))
}

fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    use tauri::Manager;
    app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()
}

/// Write a file of memory stores into the open profile, replacing what is
/// there. Answers with how many facts and how many index entries per AI.
pub(crate) fn seed_stores(app: &tauri::AppHandle, path: &std::path::Path, profile_ais: &[serde_json::Value], names: &HashMap<String, String>, all: Option<&str>) -> Result<serde_json::Value, String> {
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stores: MemoryStores = serde_json::from_slice(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let key = data_key(app)?;
    let mut targets = Vec::new();
    for r in &stores.recall {
        targets.push(store_id_for(profile_ais, names, all, &r.ai)?);
    }
    if let Some(facts) = &stores.facts {
        crate::memory::write_facts(app, &key, facts)?;
    }
    let mut recall = serde_json::Map::new();
    for (r, id) in stores.recall.iter().zip(&targets) {
        crate::transcript_memory::write_for(app, &key, id, &r.entries)?;
        recall.insert(r.ai.clone(), serde_json::json!(r.entries.len()));
    }
    Ok(serde_json::json!({ "facts": stores.facts.as_ref().map(Vec::len), "recall": recall }))
}

const STORES_KEPT: &str = "memory-stores-kept";

fn is_store_file(name: &str) -> bool {
    name == "memory-facts.enc" || (name.starts_with("transcript-emb-") && name.ends_with(".enc"))
}

fn store_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(is_store_file)).collect())
        .unwrap_or_default()
}

/// Keep a copy of the profile's memory stores as they are now.
pub(crate) fn keep_stores(root: &std::path::Path) -> Result<usize, String> {
    let kept = root.join(STORES_KEPT);
    let _ = std::fs::remove_dir_all(&kept);
    std::fs::create_dir_all(&kept).map_err(|e| e.to_string())?;
    let files = store_files(root);
    for f in &files {
        std::fs::copy(f, kept.join(f.file_name().unwrap())).map_err(|e| e.to_string())?;
    }
    Ok(files.len())
}

/// Put the memory stores back as they were kept: stores written since are
/// replaced, and a store that did not exist then is removed.
pub(crate) fn put_stores_back(root: &std::path::Path) -> Result<usize, String> {
    let kept = root.join(STORES_KEPT);
    if !kept.is_dir() {
        return Err("no copy of the memory stores was kept in this profile".into());
    }
    for f in store_files(root) {
        std::fs::remove_file(&f).map_err(|e| e.to_string())?;
    }
    let files = store_files(&kept);
    for f in &files {
        std::fs::copy(f, root.join(f.file_name().unwrap())).map_err(|e| e.to_string())?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn kept_memory_stores_come_back_and_later_ones_go() {
        let dir = std::env::temp_dir().join(format!("memory-stores-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("memory-facts.enc"), "facts v1").unwrap();
        std::fs::write(dir.join("transcript-emb-aaa.enc"), "index v1").unwrap();
        std::fs::write(dir.join("ai-data.json"), "not a store").unwrap();
        assert!(super::put_stores_back(&dir).is_err(), "nothing kept yet");
        assert_eq!(super::keep_stores(&dir).unwrap(), 2);
        std::fs::write(dir.join("memory-facts.enc"), "facts v2").unwrap();
        std::fs::write(dir.join("transcript-emb-bbb.enc"), "a later index").unwrap();
        std::fs::write(dir.join("ai-data.json"), "changed").unwrap();
        assert_eq!(super::put_stores_back(&dir).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(dir.join("memory-facts.enc")).unwrap(), "facts v1");
        assert_eq!(std::fs::read_to_string(dir.join("transcript-emb-aaa.enc")).unwrap(), "index v1");
        assert!(!dir.join("transcript-emb-bbb.enc").exists());
        assert_eq!(std::fs::read_to_string(dir.join("ai-data.json")).unwrap(), "changed", "other files are left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stores_file_names_ais_the_way_a_history_does() {
        let ais = vec![serde_json::json!({ "id": "ai-1", "name": "Juno", "agentPubKey": "uhCAk1" }), serde_json::json!({ "id": "ai-2", "name": "Atlas" })];
        let none = HashMap::new();
        assert_eq!(super::store_id_for(&ais, &none, None, "juno").unwrap(), "ai-1");
        assert_eq!(super::store_id_for(&ais, &none, None, "uhCAk1").unwrap(), "ai-1");
        assert_eq!(super::store_id_for(&ais, &none, Some("Atlas"), "Pilot").unwrap(), "ai-2");
        let map: HashMap<String, String> = [("Pilot".to_string(), "Juno".to_string())].into();
        assert_eq!(super::store_id_for(&ais, &map, None, "Pilot").unwrap(), "ai-1");
        assert!(super::store_id_for(&ais, &none, None, "Pilot").is_err());
        let wrong: HashMap<String, String> = [("Pilot".to_string(), "Nobody".to_string())].into();
        assert!(super::store_id_for(&ais, &wrong, Some("Atlas"), "Pilot").is_err(), "a wrong map is an error, never the fallback");
    }

    use super::*;

    fn ais() -> HashMap<String, FixtureAi> {
        let ai = |agent: &str, name: &str| FixtureAi { agent: agent.into(), founding: agent.into(), name: name.into() };
        HashMap::from([("Juno".to_string(), ai("agent-juno", "Juno")), ("Atlas".to_string(), ai("agent-atlas", "Atlas"))])
    }

    fn the_bakery() -> Fixture {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../e2e/fixtures/memory/history.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn small(json: serde_json::Value) -> Fixture {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn the_made_up_history_plans_whole() {
        let (planned, skipped) = plan(&the_bakery(), &ais(), "a-model.gguf", &HashMap::new()).unwrap();
        assert_eq!((planned.len(), skipped), (14, 0));
        assert_eq!(planned.iter().map(|p| p.messages.len()).sum::<usize>(), 66);
        let starts: HashSet<i64> = planned.iter().map(|p| p.started_at).collect();
        assert_eq!(starts.len(), 14, "every conversation has its own start time");
    }

    #[test]
    fn a_planned_conversation_is_an_ordinary_one_with_the_files_dates() {
        let (planned, _) = plan(&the_bakery(), &ais(), "a-model.gguf", &HashMap::new()).unwrap();
        let p = planned.iter().find(|p| p.fixture_id == "conv-b06").unwrap();
        // 2026-03-03T11:20:00Z
        assert_eq!(p.started_at, 1_772_536_800_000_000);
        assert_eq!(p.meta["started_at"], 1_772_536_800_000_000i64);
        assert_eq!(p.meta["title"], "Delivery threshold");
        assert_eq!(p.meta["ai_personality_id"], "agent-juno");
        assert_eq!(p.meta["ai_personality_name"], "Juno");
        // Not marked as an import: the app treats those differently.
        assert!(p.meta["source"].is_null());
        assert_eq!(p.meta["model_used"], "a-model.gguf");
        assert_eq!(p.messages.len(), 4);
        for (i, m) in p.messages.iter().enumerate() {
            assert_eq!(m["sequence"], i as u32);
            assert_eq!(m["model"], "a-model.gguf");
            assert_eq!(m["timestamp"], 1_772_536_800_000_000i64 + 90_000_000 * i as i64);
        }
        assert_eq!(p.messages[0]["role"], "user");
        assert!(p.messages[0]["content"].as_str().unwrap().contains("$60"));
        assert_eq!(p.messages[1]["role"], "assistant");
    }

    #[test]
    fn a_conversation_already_in_the_profile_is_left_alone() {
        let all = plan(&the_bakery(), &ais(), "m", &HashMap::new()).unwrap().0;
        let juno_first = all.iter().find(|p| p.ai.agent == "agent-juno").unwrap().started_at;
        let existing = HashMap::from([("agent-juno".to_string(), HashSet::from([juno_first])), ("agent-atlas".to_string(), HashSet::from([juno_first]))]);
        let (planned, skipped) = plan(&the_bakery(), &ais(), "m", &existing).unwrap();
        assert_eq!((planned.len(), skipped), (13, 1), "only the AI that has it skips it");
        // A second load of everything writes nothing.
        let mut have: HashMap<String, HashSet<i64>> = HashMap::new();
        for p in &all {
            have.entry(p.ai.agent.clone()).or_default().insert(p.started_at);
        }
        let (planned, skipped) = plan(&the_bakery(), &ais(), "m", &have).unwrap();
        assert_eq!((planned.len(), skipped), (0, 14));
    }

    #[test]
    fn a_file_that_would_be_ambiguous_is_refused_before_anything_is_written() {
        let conv = |id: &str, ai: &str, start: &str| serde_json::json!({"id": id, "ai": ai, "title": "t", "started_at": start, "messages": [{"role": "user", "text": "hi", "at": start}]});
        let two_same = small(serde_json::json!({"conversations": [conv("a", "Juno", "2026-01-01T00:00:00Z"), conv("b", "Juno", "2026-01-01T00:00:00Z")]}));
        assert!(plan(&two_same, &ais(), "m", &HashMap::new()).unwrap_err().contains("its own start time"));
        // The same start for two different AIs is two conversations.
        let two_ais = small(serde_json::json!({"conversations": [conv("a", "Juno", "2026-01-01T00:00:00Z"), conv("b", "Atlas", "2026-01-01T00:00:00Z")]}));
        assert_eq!(plan(&two_ais, &ais(), "m", &HashMap::new()).unwrap().0.len(), 2);
        let unknown = small(serde_json::json!({"conversations": [conv("a", "Nobody", "2026-01-01T00:00:00Z")]}));
        assert!(plan(&unknown, &ais(), "m", &HashMap::new()).unwrap_err().contains("no AI is mapped"));
        let no_date = small(serde_json::json!({"conversations": [conv("a", "Juno", "soon")]}));
        assert!(plan(&no_date, &ais(), "m", &HashMap::new()).unwrap_err().contains("not a date"));
        let bad_role = small(serde_json::json!({"conversations": [{"id": "a", "ai": "Juno", "title": "t", "started_at": "2026-01-01T00:00:00Z", "messages": [{"role": "system", "text": "x"}]}]}));
        assert!(plan(&bad_role, &ais(), "m", &HashMap::new()).unwrap_err().contains("neither user nor assistant"));
        let empty = small(serde_json::json!({"conversations": [{"id": "a", "ai": "Juno", "title": "t", "started_at": "2026-01-01T00:00:00Z", "messages": []}]}));
        assert!(plan(&empty, &ais(), "m", &HashMap::new()).unwrap_err().contains("no messages"));
    }

    #[test]
    fn a_message_with_no_time_follows_the_one_before_it() {
        let f = small(serde_json::json!({"conversations": [{"id": "a", "ai": "Juno", "title": "t", "started_at": "2026-01-01T00:00:00Z", "messages": [{"role": "user", "text": "x"}, {"role": "assistant", "text": "y"}]}]}));
        let (planned, _) = plan(&f, &ais(), "m", &HashMap::new()).unwrap();
        let t: Vec<i64> = planned[0].messages.iter().map(|m| m["timestamp"].as_i64().unwrap()).collect();
        assert_eq!(t[1] - t[0], 1_000_000);
        assert!(t[0] > planned[0].started_at, "no two messages of a conversation share a time, nor the start");
    }

    #[test]
    fn a_history_from_real_use_keeps_its_source_model_sequence_and_times() {
        // Two messages that took the same sequence number on two devices,
        // the later one listed first, and a conversation another program
        // drove over the local port.
        let f = small(serde_json::json!({"conversations": [{"id": "a", "ai": "Juno", "title": "t", "started_at": "2026-01-01T00:00:00Z", "source": "some-program/1.0", "model": "their-model.gguf",
            "messages": [{"role": "user", "text": "x", "at": 1_767_225_900_000_123i64, "sequence": 4}, {"role": "user", "text": "y", "at": "2026-01-01T00:03:00Z", "sequence": 4}]}]}));
        let (planned, _) = plan(&f, &ais(), "m", &HashMap::new()).unwrap();
        let p = &planned[0];
        assert_eq!(p.meta["source"], "some-program/1.0");
        assert_eq!(p.meta["model_used"], "their-model.gguf");
        assert_eq!(p.messages.iter().map(|m| m["sequence"].as_u64().unwrap()).collect::<Vec<_>>(), vec![4, 4]);
        assert_eq!(p.messages[0]["model"], "their-model.gguf");
        assert_eq!(p.messages[0]["timestamp"], 1_767_225_900_000_123i64, "a time given as a number is kept to the microsecond");
        assert!(p.messages[1]["timestamp"].as_i64() < p.messages[0]["timestamp"].as_i64(), "times are kept as given, never reordered");
    }

    /// The real sidecars: the made-up history is written through two AIs on
    /// a scratch conductor and read back as ordinary conversations with the
    /// file's dates; a second load writes nothing.
    /// Run with the app closed: `cargo test --lib -- --ignored live_fixture --test-threads 1 --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_fixture_the_made_up_history_loads_as_ordinary_conversations() {
        use crate::conductor::{start_for_test, supervisor};
        use crate::holochain::HolochainManager;
        use crate::transcript_crypto::RecoveryMaterial;
        use holochain_types::prelude::{ActionHash, ExternIO, Record};
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let resource_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let data_key = [11u8; 32];
        let material = RecoveryMaterial { network_seed: format!("live-fixture-{}", std::process::id()), data_key_hex: hex::encode(data_key), version: 1 };
        start_for_test(profile.clone(), None).await.expect("0.6 starts");
        let manager = HolochainManager::new(resource_dir, material);
        let juno = manager.provision_agent("live-fixture-juno").await.expect("an AI");
        let atlas = manager.provision_agent("live-fixture-atlas").await.expect("a second AI");
        let ai = |agent: &String, name: &str| FixtureAi { agent: agent.clone(), founding: agent.clone(), name: name.into() };
        let ais = HashMap::from([("Juno".to_string(), ai(&juno, "Juno")), ("Atlas".to_string(), ai(&atlas, "Atlas"))]);

        let fixture = the_bakery();
        let (planned, skipped) = plan(&fixture, &ais, "a-model.gguf", &HashMap::new()).unwrap();
        let loaded = write(&manager, &data_key, &planned, skipped).await.expect("the history is written");
        assert_eq!((loaded.conversations, loaded.messages, loaded.skipped), (14, 66, 0));

        // Read back through each AI's own cell.
        let mut have: HashMap<String, HashSet<i64>> = HashMap::new();
        let mut titles = HashSet::new();
        for (agent, expect) in [(&juno, 12usize), (&atlas, 2usize)] {
            let out = manager.call_zome(agent, "transcript", "get_all_conversations", ExternIO::encode(()).unwrap()).await.unwrap();
            let records: Vec<Record> = ExternIO::decode(&out).unwrap();
            assert_eq!(records.len(), expect, "conversations listed for this AI");
            for r in &records {
                let (plain, _) = crate::vault_escrow::open_record(&data_key, r).expect("a conversation this key opens");
                assert!(plain["source"].is_null(), "an ordinary conversation, not an import");
                titles.insert(plain["title"].as_str().unwrap().to_string());
                have.entry(agent.clone()).or_default().insert(plain["started_at"].as_i64().unwrap());
            }
        }
        assert_eq!(titles.len(), 14);
        let id = loaded.ids.get("conv-b06").expect("its id");
        let hash = ActionHash::from_raw_39(hex::decode(id).unwrap());
        let out = manager.call_zome(&juno, "transcript", "get_conversation_entries", ExternIO::encode(hash).unwrap()).await.unwrap();
        let entries: Vec<Record> = ExternIO::decode(&out).unwrap();
        let mut plains: Vec<serde_json::Value> = entries.iter().filter_map(|r| crate::vault_escrow::open_record(&data_key, r)).map(|(p, _)| p).collect();
        plains.sort_by_key(|p| p["sequence"].as_u64());
        assert_eq!(plains.len(), 4);
        assert_eq!(plains[0]["timestamp"], 1_772_536_800_000_000i64);
        assert!(plains[1]["content"].as_str().unwrap().contains("free delivery starts at $60"));

        // A second load of the same file writes nothing.
        let (planned, skipped) = plan(&fixture, &ais, "a-model.gguf", &have).unwrap();
        assert_eq!((planned.len(), skipped), (0, 14));
        supervisor().stop();
    }
}
