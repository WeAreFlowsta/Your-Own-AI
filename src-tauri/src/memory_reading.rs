//! The reading job: keeps memory's index (`memory_index`) level with the
//! person's conversations, in the background.
//!
//! One standing task. Each pass it makes sure the text cache holds every
//! AI's conversations, compares what the cache holds with what the index
//! has read, and reads the difference: the most recently active
//! conversations first, a few passages at a time, each batch stored with
//! its resume point, so a stop loses at most the batch in hand. It runs on
//! a calm machine; while the person is working on mains power it takes one
//! batch every ten seconds; "Read now" runs it at once.
//!
//! A turn recorded here wakes it; a conversation deleted here leaves the
//! index at once. Everything else (an import, a message written on another
//! device, a restore) is found by the comparison.

use crate::memory_index::{self as index, Cache, ConvMeta, ConvState, Hit, Made, Opened, ReadMessage, Store};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The job's name in the machine's list of held work (the Activity tray).
const JOB: &str = "reading your conversations";
/// Texts per embedding request, with a breath between requests: more per
/// request starves a routing question on a processor-run embedder.
const BATCH: usize = 4;
const BREATH: Duration = Duration::from_millis(50);
/// The pace while the person is at the computer.
const SLOW_PACE: Duration = Duration::from_secs(10);
/// Reading stops when the profile's disk has less than this free.
const DISK_FLOOR_MB: u64 = 200;

struct Open {
    root: PathBuf,
    store: Store,
    cache: Cache,
    opened: Opened,
    /// Every item's vector with the stamp of the words it was made from,
    /// loaded on first use.
    items: Option<std::collections::HashMap<String, (i64, index::Quant)>>,
}

static INDEX: Mutex<Option<Open>> = Mutex::new(None);
static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static READ_NOW: AtomicBool = AtomicBool::new(false);
static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static LEVEL_AT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// What the reading line shows.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Messages that can be searched, and messages there are.
    pub read: i64,
    pub total: i64,
    pub passages: usize,
    /// Reading has more to do and is not running now.
    pub waiting: bool,
    /// Why it waits, in words the app can show.
    pub why: Option<String>,
    /// "Read now" is on.
    pub read_now: bool,
    /// More disk room needed before reading can go on, in megabytes.
    pub needs_mb: Option<u64>,
    /// The index could not be read and is being built again.
    pub rebuilding: bool,
    /// When the reader last began bringing the text of every conversation
    /// level and finished it, in milliseconds since 1970 (0: not yet). What
    /// was recorded before then is counted in `total`.
    pub level_at: i64,
    /// Seconds one message has been taking, for the estimate.
    pub secs_per_message: Option<f32>,
    /// Passages embedded from a shortened text, so far.
    pub shortened: usize,
}

fn data_key(app: &AppHandle) -> Result<[u8; 32], String> {
    app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()
}

fn made() -> Made {
    Made { model: crate::embedding_model::FILE.to_string(), mask: crate::secret_mask::MASK_VERSION.to_string() }
}

/// Run `f` on the open index of this profile, opening it (and loading its
/// vectors) the first time and when the profile changed.
fn with_index<R>(app: &AppHandle, f: impl FnOnce(&mut Open, &[u8; 32]) -> Result<R, String>) -> Result<R, String> {
    let key = data_key(app)?;
    let root = crate::profile::root(app)?;
    let mut guard = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_none_or(|o| o.root != root) {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let (store, opened) = Store::open(&root.join("memory-index.sqlite"), &made())?;
        let t0 = Instant::now();
        let cache = match store.load(&key) {
            Ok(c) => c,
            // Written under another key (another identity's file in this
            // place): nothing of it can be used.
            Err(e) => {
                log::warn!("[memory index] the stored vectors could not be opened ({e}); read again from the records");
                drop(store);
                for suffix in ["", "-wal", "-shm"] {
                    let _ = std::fs::remove_file(PathBuf::from(format!("{}{suffix}", root.join("memory-index.sqlite").display())));
                }
                let (store, _) = Store::open(&root.join("memory-index.sqlite"), &made())?;
                *guard = Some(Open { root: root.clone(), store, cache: Cache::default(), opened: Opened::Rebuilt, items: None });
                return f(guard.as_mut().unwrap(), &key);
            }
        };
        log::info!("[memory index] open: {} passage(s) in memory ({} KB) in {} ms", cache.passages(), cache.bytes_held() / 1024, t0.elapsed().as_millis());
        *guard = Some(Open { root, store, cache, opened, items: None });
    }
    f(guard.as_mut().unwrap(), &key)
}

/// A turn was just recorded: the reading job takes it on its next pass,
/// which starts now.
pub(crate) fn note_recorded() {
    WAKE.notify_one();
}

/// A conversation was deleted: its passages leave the index now.
pub(crate) fn forget_conversation(app: &AppHandle, hash: &str) {
    let Ok(k) = crate::transcript_search::row_key(app, hash) else { return };
    let _ = with_index(app, |open, _| {
        open.cache.forget(&k);
        open.store.forget(&k).map(|_| ())
    });
}

/// A message a search found, with where it is.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct Found {
    pub hits: Vec<Hit>,
    /// Messages read into the index, and messages there are: a search
    /// always says what share it could look at.
    pub read: i64,
    pub total: i64,
}

/// Search the index for a question that is already embedded.
pub(crate) fn search(app: &AppHandle, question: &[f32], at_most: usize, keep: &dyn Fn(&index::Candidate) -> bool) -> Result<Found, String> {
    let hits = with_index(app, |open, _| Ok(open.cache.search(question, at_most, keep)))?;
    let (read, total) = STATUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| (s.read, s.total)).unwrap_or((0, 0));
    Ok(Found { hits, read, total })
}

/// A found message with what names it outside this device's index: the
/// conversation's start, the message's sequence and time.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct FoundMessage {
    pub conversation: String,
    pub started_at: i64,
    pub title: Option<String>,
    pub source: Option<String>,
    pub seq: u32,
    pub at: i64,
    pub role: String,
    pub score: f32,
    pub flags: u8,
}

/// The hits of a search with their places, in rank order. A hit whose
/// conversation left the text cache since is dropped.
pub(crate) fn places(app: &AppHandle, hits: &[Hit]) -> Result<Vec<FoundMessage>, String> {
    let cached = crate::transcript_search::cached_conversations(app)?;
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        let Some(c) = cached.iter().find(|c| c.k == h.k) else { continue };
        let entry = crate::transcript_search::cached_entries(app, &h.k, i64::from(h.seq))?.into_iter().next().filter(|e| e.seq == h.seq);
        let Some(e) = entry else { continue };
        out.push(FoundMessage { conversation: c.hash.clone(), started_at: c.started_at, title: c.title.clone(), source: c.source.clone(), seq: h.seq, at: e.at, role: e.role, score: h.score, flags: h.flags });
    }
    Ok(out)
}

/// The hits of a search as whole exchanges: each found message with the
/// message before it and the one after, every text with its secrets
/// hidden. Also answers how many secrets were hidden.
pub(crate) fn exchanges(app: &AppHandle, hits: &[Hit]) -> Result<(Vec<crate::memory_block::Exchange>, u32), String> {
    use crate::memory_block::{Exchange, Said};
    let cached = crate::transcript_search::cached_conversations(app)?;
    let mut read: std::collections::HashMap<String, Vec<crate::transcript_search::CachedEntry>> = std::collections::HashMap::new();
    let mut hidden = 0u32;
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        let Some(c) = cached.iter().find(|c| c.k == h.k) else { continue };
        if !read.contains_key(&h.k) {
            let said: Vec<_> = crate::transcript_search::cached_entries(app, &h.k, 0)?.into_iter().filter(|e| e.role == "user" || e.role == "assistant").collect();
            read.insert(h.k.clone(), said);
        }
        let entries = &read[&h.k];
        let Some(x) = entries.iter().position(|e| e.seq == h.seq) else { continue };
        let mut said = |e: &crate::transcript_search::CachedEntry| {
            let m = crate::secret_mask::hand_on(&e.content);
            hidden += m.hidden;
            Said { by_person: e.role == "user", text: m.text, seq: e.seq, at: e.at }
        };
        let before = (x > 0).then(|| said(&entries[x - 1]));
        let hit = said(&entries[x]);
        let after = entries.get(x + 1).map(&mut said);
        out.push(Exchange {
            conversation: c.started_at,
            title: c.title.clone().unwrap_or_default(),
            ai_name: Some(c.ai_name.clone()).filter(|n| !n.is_empty()),
            project: None,
            before,
            hit,
            after,
            score: h.score,
        });
    }
    Ok((out, hidden))
}

/// What a reply was given, read again from its record: each pointer
/// looked up among the conversations on this device, the block rebuilt and
/// compared with its recorded fingerprint. A conversation is named by its
/// start (and, where two share one, by the message's own time), which is
/// the same on every device and after a restore.
#[tauri::command]
pub fn memory_given_texts(app: AppHandle, given: crate::memory_block::GivenRecord) -> Result<crate::memory_block::Panel, String> {
    use crate::memory_block::{Now, Ptr};
    let cached = crate::transcript_search::cached_conversations(&app)?;
    let deleted: std::collections::HashSet<i64> = crate::conversation_cache::deleted_ledger(&app).into_iter().map(|d| d.started_at).collect();
    let read: std::cell::RefCell<std::collections::HashMap<String, Vec<crate::transcript_search::CachedEntry>>> = Default::default();
    let at = |p: &Ptr| -> Now {
        let mut gone = if deleted.contains(&p.c) { Now::Deleted } else { Now::NotHere };
        for c in cached.iter().filter(|c| c.started_at == p.c) {
            let mut read = read.borrow_mut();
            let entries = read.entry(c.k.clone()).or_insert_with(|| crate::transcript_search::cached_entries(&app, &c.k, 0).unwrap_or_default());
            let Some(e) = entries.iter().find(|e| e.seq == p.s) else { continue };
            let here = Now::Here {
                by_person: e.role == "user",
                text: crate::secret_mask::hand_on(&e.content).text,
                title: c.title.clone().unwrap_or_default(),
                ai_name: Some(c.ai_name.clone()).filter(|n| !n.is_empty()),
                project: None,
                conversation_id: c.hash.clone(),
            };
            // The message with this very time is the one; another
            // conversation with the same start stands in only if none has.
            if e.at == p.t {
                return here;
            }
            gone = here;
        }
        gone
    };
    let mut panel = crate::memory_block::rebuild(&given, &at, crate::memory_block::Wording::shipped());
    // What was left out as replaced: is it still, and since when.
    if !panel.left_out.is_empty() {
        let items = data_key(&app).map(|key| crate::memory_items::items(&crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE))).unwrap_or_default();
        for l in panel.left_out.iter_mut() {
            let replaced = items.iter().find(|i| i.row.id == l.item).filter(|i| i.meta.state == crate::memory_items::ItemState::Replaced);
            l.still_replaced = replaced.is_some();
            l.replaced_on = replaced.map(|i| crate::memory_block::day(i.meta.state_at * 1000));
        }
    }
    Ok(panel)
}

fn item_table<'a>(open: &'a mut Open, key: &[u8; 32]) -> Result<&'a mut std::collections::HashMap<String, (i64, index::Quant)>, String> {
    if open.items.is_none() {
        open.items = Some(open.store.item_vecs(key)?.into_iter().map(|(id, at, q)| (id, (at, q))).collect());
    }
    Ok(open.items.as_mut().expect("just loaded"))
}

/// Every item's vector on this device: the item's id, the stamp of the
/// words the vector was made from, the vector.
pub(crate) fn item_vectors(app: &AppHandle) -> std::collections::HashMap<String, (i64, index::Quant)> {
    with_index(app, |open, key| item_table(open, key).map(|t| t.clone())).unwrap_or_default()
}

/// Keep an item's vector, with the stamp of the words it was made from.
pub(crate) fn put_item_vector(app: &AppHandle, id: &str, stamp: i64, vector: &[f32]) -> Result<(), String> {
    let q = index::quantize(vector);
    with_index(app, |open, key| {
        open.store.item_vec_put(key, id, stamp, &q)?;
        item_table(open, key)?.insert(id.to_string(), (stamp, q));
        Ok(())
    })
}

/// When each found message's conversation started, and how alike the
/// message is to each of `to`: one row per hit, in the hits' order.
pub(crate) fn hits_beside(app: &AppHandle, hits: &[Hit], to: &[Vec<f32>]) -> Vec<(Option<i64>, Vec<Option<f32>>)> {
    with_index(app, |open, _| Ok(hits.iter().map(|h| (open.cache.started_at(&h.k), to.iter().map(|v| open.cache.message_likeness(&h.k, h.seq, v)).collect())).collect())).unwrap_or_default()
}

/// The messages of one exchange, read from this device's copy of the
/// conversation that started at `started_at`: the message at `seq` and
/// the other half of its exchange (the reply to a person's message, or
/// the message an AI's reply answered), with secrets hidden. Empty when
/// the conversation is not here.
pub(crate) fn exchange_at(app: &AppHandle, started_at: i64, seq: u32) -> Vec<crate::memory_block::Said> {
    let Ok(cached) = crate::transcript_search::cached_conversations(app) else { return Vec::new() };
    for c in cached.iter().filter(|c| c.started_at == started_at) {
        let said: Vec<_> = crate::transcript_search::cached_entries(app, &c.k, 0).unwrap_or_default().into_iter().filter(|e| e.role == "user" || e.role == "assistant").collect();
        let Some(x) = said.iter().position(|e| e.seq == seq) else { continue };
        let (from, to) = if said[x].role == "user" {
            (x, if said.get(x + 1).is_some_and(|n| n.role == "assistant") { x + 1 } else { x })
        } else {
            (if x > 0 && said[x - 1].role == "user" { x - 1 } else { x }, x)
        };
        return said[from..=to]
            .iter()
            .map(|e| crate::memory_block::Said { by_person: e.role == "user", text: crate::secret_mask::hand_on(&e.content).text, seq: e.seq, at: e.at })
            .collect();
    }
    Vec::new()
}

/// How many passages the index holds in memory.
pub(crate) fn passages(app: &AppHandle) -> usize {
    with_index(app, |open, _| Ok(open.cache.passages())).unwrap_or(0)
}

/// Is the index being built again after it could not be read?
pub(crate) fn rebuilding(app: &AppHandle) -> bool {
    with_index(app, |open, _| Ok(open.opened == Opened::Rebuilt && open.cache.passages() == 0)).unwrap_or(false)
}

#[tauri::command]
pub fn memory_reading_status() -> Status {
    let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
    s.read_now = READ_NOW.load(Ordering::SeqCst);
    s
}

/// "Read now": read at once, on battery too, until done or switched off.
#[tauri::command]
pub fn memory_read_now(on: bool) {
    READ_NOW.store(on, Ordering::SeqCst);
    WAKE.notify_one();
}

fn publish(app: &AppHandle, s: Status) {
    let mut held = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    if held.as_ref() != Some(&s) {
        let _ = app.emit("memory-reading-progress", &s);
        *held = Some(s);
    }
}

/// Start the standing job.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // The conductor and the first machine reading come up first.
        tokio::time::sleep(Duration::from_secs(45)).await;
        loop {
            match pass(&app).await {
                Ok(true) => {}
                Ok(false) => {
                    // Held (the machine is busy, the stores are closed):
                    // look again soon.
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    continue;
                }
                Err(e) => log::warn!("[memory index] reading stopped: {e}"),
            }
            crate::machine::done(JOB);
            // Everything is read: wait for a recorded turn, or look again
            // in a while for what arrived another way.
            let _ = tokio::time::timeout(Duration::from_secs(10 * 60), WAKE.notified()).await;
        }
    });
}

/// May the next batch run now? `Some(pace)`: yes, after `pace`. `None`:
/// no, with the reason published.
fn gate() -> Result<Duration, &'static str> {
    if READ_NOW.load(Ordering::SeqCst) {
        return Ok(BREATH);
    }
    if crate::machine::would_run(crate::machine::Work::Heavy) {
        crate::machine::may_run(crate::machine::Work::Heavy, JOB);
        return Ok(BREATH);
    }
    if crate::machine::may_run(crate::machine::Work::HeavySlowly, JOB) {
        return Ok(SLOW_PACE);
    }
    Err(crate::machine::held_reason(JOB).unwrap_or("the computer is busy"))
}

/// What the person is told while reading waits.
pub(crate) fn why_in_words(reason: &str) -> String {
    match reason {
        "on battery" => "The computer is on battery.".to_string(),
        "memory is short" => "The computer is short of memory.".to_string(),
        "other programs are busy" | "the computer is in use" => "The computer is busy.".to_string(),
        "checking the computer" => "Checking the computer.".to_string(),
        other => format!("Waiting: {other}."),
    }
}

/// One pass: bring the text cache level, then read what the index lacks.
/// `Ok(true)` when everything is read; `Ok(false)` when it had to stop.
async fn pass(app: &AppHandle) -> Result<bool, String> {
    if data_key(app).is_err() {
        return Ok(false);
    }
    let root = crate::profile::root(app)?;
    let rebuilding = with_index(app, |open, _| Ok(open.opened == Opened::Rebuilt))?;

    // The words first: every AI's conversations in the text cache. A chain
    // read, so it asks the machine like the embedding does.
    let Some(hc) = app.try_state::<std::sync::Arc<crate::commands_holochain::HolochainState>>() else { return Ok(false) };
    let manager = hc.get()?;
    let agents = manager.ai_agents().await;
    let began = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let mut level = true;
    for agent in &agents {
        if gate().is_err() {
            level = false;
            break;
        }
        // Another read is running (the person opened search): next pass.
        if !matches!(crate::transcript_search::read_agent(app, agent).await, Ok(true)) {
            level = false;
        }
    }
    if level {
        LEVEL_AT.store(began, Ordering::SeqCst);
    }

    let mut shortened = 0usize;
    let mut pace_secs: Option<f32> = None;
    loop {
        let cached = crate::transcript_search::cached_conversations(app)?;
        let listed: Vec<(String, i64, i64)> = cached.iter().filter(|c| index::source_is_indexed(c.source.as_deref())).map(|c| (c.k.clone(), c.last_seq + 1, c.touched)).collect();
        let (known, passages) = with_index(app, |open, key| {
            // A conversation the cache no longer holds was deleted (here
            // while the app was closed, or on another device).
            let known = open.store.convs(key)?;
            let mut kept = Vec::with_capacity(known.len());
            for c in known {
                if listed.iter().any(|l| l.0 == c.k) {
                    kept.push(c);
                } else {
                    open.cache.forget(&c.k);
                    open.store.forget(&c.k)?;
                }
            }
            Ok((kept, open.cache.passages()))
        })?;
        let todo = index::to_read(&listed, &known);
        let (read, total) = index::share_read(&listed, &known);
        let status = |waiting: bool, why: Option<String>, needs_mb: Option<u64>| Status { read, total, passages, waiting, why, read_now: READ_NOW.load(Ordering::SeqCst), needs_mb, rebuilding: rebuilding && read < total, secs_per_message: pace_secs, shortened, level_at: LEVEL_AT.load(Ordering::SeqCst) };
        let Some((k, from)) = todo.into_iter().next() else {
            READ_NOW.store(false, Ordering::SeqCst);
            publish(app, status(false, None, None));
            return Ok(true);
        };
        if let Some((free, _)) = crate::llm::disk_space_for(&root) {
            let free_mb = free / (1024 * 1024);
            if free_mb < DISK_FLOOR_MB {
                publish(app, status(true, Some("There is not enough room on the disk.".to_string()), Some(DISK_FLOOR_MB - free_mb)));
                return Ok(false);
            }
        }
        let pace = match gate() {
            Ok(p) => p,
            Err(reason) => {
                publish(app, status(true, Some(why_in_words(reason)), None));
                return Ok(false);
            }
        };
        publish(app, status(false, None, None));

        let Some(conv) = cached.iter().find(|c| c.k == k) else { continue };
        let meta = ConvMeta {
            started_at: conv.started_at,
            ai: crate::commands_holochain::founding_key_for(app, &conv.agent_key),
            source: conv.source.clone(),
            project: None,
            private: false,
        };
        let entries = crate::transcript_search::cached_entries(app, &k, from)?;
        // One batch: the next messages of this conversation, up to BATCH
        // passages (a long message is taken whole).
        let mut texts: Vec<String> = Vec::new();
        let mut plan: Vec<(u32, u8, u8, usize)> = Vec::new();
        let mut through = from - 1;
        for e in &entries {
            let role = match e.role.as_str() {
                "user" => index::ROLE_PERSON,
                "assistant" => index::ROLE_AI,
                _ => {
                    through = i64::from(e.seq);
                    continue;
                }
            };
            // What is embedded is what may be found: the masked text.
            let masked = crate::secret_mask::hand_on(&e.content);
            let flags = if masked.hidden > 0 { index::FLAG_SECRET } else { 0 };
            let passages = index::cut(&masked.text);
            if !texts.is_empty() && texts.len() + passages.len() > BATCH {
                break;
            }
            plan.push((e.seq, role, flags, passages.len()));
            texts.extend(passages.into_iter().map(|p| format!("{}{p}", crate::embedding_model::DOC_PREFIX)));
            through = i64::from(e.seq);
        }
        let t0 = Instant::now();
        let mut messages: Vec<ReadMessage> = Vec::new();
        if !texts.is_empty() {
            let before = crate::llm::EMBEDS_SHORTENED.load(Ordering::Relaxed);
            let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
            for chunk in texts.chunks(BATCH) {
                vectors.extend(crate::llm::embed_texts(app.clone(), app.state(), chunk.to_vec(), crate::embedding_model::FILE.to_string()).await.map_err(|e| format!("the memory model: {e}"))?);
                tokio::time::sleep(BREATH).await;
            }
            // Which of these were embedded from a shortened text: rare, so
            // found by asking again one at a time only when the count moved.
            let mut short = vec![false; texts.len()];
            if crate::llm::EMBEDS_SHORTENED.load(Ordering::Relaxed) != before {
                for (i, t) in texts.iter().enumerate() {
                    let b = crate::llm::EMBEDS_SHORTENED.load(Ordering::Relaxed);
                    if crate::llm::embed_texts(app.clone(), app.state(), vec![t.clone()], crate::embedding_model::FILE.to_string()).await.is_ok() {
                        short[i] = crate::llm::EMBEDS_SHORTENED.load(Ordering::Relaxed) != b;
                    }
                }
            }
            if vectors.len() != texts.len() {
                return Err("the memory model answered with fewer vectors than texts".into());
            }
            let mut at = 0;
            for (seq, role, mut flags, n) in plan {
                if short[at..at + n].iter().any(|s| *s) {
                    flags |= index::FLAG_SHORTENED;
                    shortened += short[at..at + n].iter().filter(|s| **s).count();
                }
                messages.push(ReadMessage { seq, role, flags, passages: vectors[at..at + n].iter().map(|v| index::quantize(v)).collect() });
                at += n;
            }
        }
        // Nothing at or after the resume point although the list said so:
        // the conversation counts as read, or the pass would never end.
        let read_through = if entries.is_empty() { conv.last_seq } else { through };
        let state = ConvState { k: k.clone(), read_through, entries: conv.last_seq + 1, touched: conv.touched, meta: meta.clone() };
        with_index(app, |open, key| {
            open.store.put_batch(key, &state, &messages)?;
            for m in &messages {
                for q in &m.passages {
                    open.cache.append(&k, &meta, m.seq, m.role, m.flags, q);
                }
            }
            Ok(())
        })?;
        if !messages.is_empty() {
            let per = t0.elapsed().as_secs_f32() / messages.len() as f32;
            pace_secs = Some(pace_secs.map_or(per, |p| p * 0.8 + per * 0.2));
        }
        tokio::time::sleep(pace).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_reason_is_said_in_plain_words() {
        assert_eq!(why_in_words("on battery"), "The computer is on battery.");
        assert_eq!(why_in_words("the computer is in use"), "The computer is busy.");
        assert_eq!(why_in_words("other programs are busy"), "The computer is busy.");
        assert_eq!(why_in_words("memory is short"), "The computer is short of memory.");
    }

    #[test]
    fn the_status_is_named_for_the_page() {
        let s = serde_json::to_value(Status { read: 62, total: 100, read_now: true, needs_mb: Some(40), ..Default::default() }).unwrap();
        assert_eq!(s["read"], 62);
        assert_eq!(s["readNow"], true);
        assert_eq!(s["needsMb"], 40);
        assert_eq!(s["secsPerMessage"], serde_json::Value::Null);
    }
}
