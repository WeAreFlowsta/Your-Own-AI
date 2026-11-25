//! Memory's search index: one index for the person, over both speakers of
//! every conversation of every AI. It holds vectors and pointers, never
//! text. A found message's words are read from the text cache
//! (`transcript_search`) at search time and pass through the secret mask.
//!
//! The index is a cache: it can be deleted at any time and is built again
//! from the records, newest conversations first, in the background. A
//! search uses whatever is read so far and always says what share that is.
//!
//! On disk (`memory-index.sqlite` in the profile): one row per passage,
//! its vector as one signed byte per number with a scale, encrypted with
//! the data key. Conversations are keyed by the same keyed hash the text
//! cache uses. In the clear: counts, sequence numbers, last-active times.
//! In memory: every vector, as bytes, appended to as messages arrive.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How a message is cut for embedding: a passage starts every `STRIDE`
/// characters and is `LEN` long, so neighbors share 100 characters.
pub(crate) const CONV_PASSAGE_STRIDE: usize = 900;
pub(crate) const CONV_PASSAGE_LEN: usize = 1000;
/// How many messages a search returns.
pub(crate) const FOUND_AT_MOST: usize = 12;

const FORMAT: &str = "1";
const QUANT: &str = "i8-maxabs-1";
const CUT: &str = "stride900-len1000-1";

/// The passage holds a hidden secret (it was embedded with it masked).
pub(crate) const FLAG_SECRET: u8 = 1;
/// The reply was itself given a search block: a report of a search is not
/// evidence (what it quoted is still in the index).
pub(crate) const FLAG_GIVEN_A_BLOCK: u8 = 2;
/// A reply the person stopped.
pub(crate) const FLAG_STOPPED: u8 = 4;
/// Embedded from a shortened text (the embedder refused the whole passage).
pub(crate) const FLAG_SHORTENED: u8 = 8;
/// The turn was about the person's health: never given to an AI that
/// answers off this computer.
pub(crate) const FLAG_HEALTH: u8 = 16;

pub(crate) const ROLE_PERSON: u8 = 0;
pub(crate) const ROLE_AI: u8 = 1;

/// The passages of one message, as the measured cut makes them.
pub(crate) fn cut(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec!["(empty)".to_string()];
    }
    (0..chars.len()).step_by(CONV_PASSAGE_STRIDE).map(|i| chars[i..(i + CONV_PASSAGE_LEN).min(chars.len())].iter().collect()).collect()
}

/// A vector stored as one signed byte per number.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Quant {
    pub bytes: Vec<i8>,
    /// What one step of a byte is worth.
    pub scale: f32,
}

/// `v` scaled to length 1 (a zero vector stays zero).
pub(crate) fn unit(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>().sqrt();
    if n == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| (f64::from(*x) / n) as f32).collect()
}

/// One byte per number: the unit vector's largest value becomes 127.
pub(crate) fn quantize(v: &[f32]) -> Quant {
    let u = unit(v);
    let m = u.iter().fold(0f32, |m, x| m.max(x.abs()));
    if m == 0.0 {
        return Quant { bytes: vec![0; u.len()], scale: 0.0 };
    }
    let s = 127.0 / m;
    Quant { bytes: u.iter().map(|x| (x * s).round() as i8).collect(), scale: 1.0 / s }
}

fn score(unit_query: &[f32], bytes: &[i8], scale: f32) -> f32 {
    unit_query.iter().zip(bytes).map(|(q, b)| q * f32::from(*b)).sum::<f32>() * scale
}

/// A stored vector as numbers again (of a length close to 1).
pub(crate) fn numbers(q: &Quant) -> Vec<f32> {
    q.bytes.iter().map(|b| f32::from(*b) * q.scale).collect()
}

/// How alike a vector of any length and a stored one are: the cosine,
/// within what one byte per number can say. Vectors of different sizes
/// (made by different models) are not alike at all.
pub(crate) fn likeness(v: &[f32], q: &Quant) -> f32 {
    if v.is_empty() || v.len() != q.bytes.len() {
        return 0.0;
    }
    score(&unit(v), &q.bytes, q.scale)
}

/// What the index knows of a conversation beyond its key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct ConvMeta {
    pub started_at: i64,
    /// The founding key of the AI the conversation is with.
    #[serde(default)]
    pub ai: String,
    /// None for conversations held in the app; `import:<from>`,
    /// `copy:<device>`, or the name of a program that used the local port.
    #[serde(default)]
    pub source: Option<String>,
    /// The project it was held in, by the project's identity.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub private: bool,
}

/// Is a conversation from this source read into the index at all? The old
/// project memory text's revisions are not conversations.
pub(crate) fn source_is_indexed(source: Option<&str>) -> bool {
    source != Some("workspace-memory")
}

/// Is a conversation from this source searched without the person saying
/// so? Conversations held in the app, imports, and copies held for another
/// device are; what other programs sent through the local port is not.
pub(crate) fn source_is_searched_by_default(source: Option<&str>) -> bool {
    match source {
        None => true,
        Some(s) => s.starts_with("import:") || s.starts_with("copy:"),
    }
}

/// One passage in memory.
#[derive(Clone, Copy, Debug)]
struct Row {
    conv: u32,
    seq: u32,
    role: u8,
    flags: u8,
    scale: f32,
}

/// Every vector, in memory. Appended to; never dropped on a write.
#[derive(Default)]
pub(crate) struct Cache {
    dims: usize,
    rows: Vec<Row>,
    bytes: Vec<i8>,
    keys: Vec<String>,
    metas: Vec<ConvMeta>,
    by_key: HashMap<String, u32>,
}

/// A message a search found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Hit {
    /// The conversation's key (the text cache's key for it).
    pub k: String,
    pub seq: u32,
    pub role: u8,
    pub score: f32,
    pub flags: u8,
}

/// A passage a filter is asked about.
pub(crate) struct Candidate<'a> {
    pub k: &'a str,
    pub meta: &'a ConvMeta,
    pub seq: u32,
    pub flags: u8,
}

impl Cache {
    pub(crate) fn passages(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn bytes_held(&self) -> usize {
        self.bytes.len() + self.rows.len() * std::mem::size_of::<Row>()
    }

    fn conv(&mut self, k: &str, meta: &ConvMeta) -> u32 {
        if let Some(i) = self.by_key.get(k) {
            self.metas[*i as usize] = meta.clone();
            return *i;
        }
        let i = self.keys.len() as u32;
        self.keys.push(k.to_string());
        self.metas.push(meta.clone());
        self.by_key.insert(k.to_string(), i);
        i
    }

    /// Add one passage. A vector of another size than the index's belongs
    /// to another embedding model and is refused.
    pub(crate) fn append(&mut self, k: &str, meta: &ConvMeta, seq: u32, role: u8, flags: u8, q: &Quant) -> bool {
        if self.dims == 0 {
            self.dims = q.bytes.len();
        }
        if q.bytes.len() != self.dims || self.dims == 0 {
            return false;
        }
        let conv = self.conv(k, meta);
        self.rows.push(Row { conv, seq, role, flags, scale: q.scale });
        self.bytes.extend_from_slice(&q.bytes);
        true
    }

    /// Drop a conversation's passages (it was deleted).
    pub(crate) fn forget(&mut self, k: &str) -> usize {
        let Some(conv) = self.by_key.get(k).copied() else { return 0 };
        let dims = self.dims;
        let before = self.rows.len();
        let mut kept_rows = Vec::with_capacity(before);
        let mut kept_bytes = Vec::with_capacity(self.bytes.len());
        for (i, r) in self.rows.iter().enumerate() {
            if r.conv != conv {
                kept_rows.push(*r);
                kept_bytes.extend_from_slice(&self.bytes[i * dims..(i + 1) * dims]);
            }
        }
        self.rows = kept_rows;
        self.bytes = kept_bytes;
        // The key stays in the table (indexes into it are held by rows); a
        // conversation with no rows is simply never found.
        before - self.rows.len()
    }

    /// When a conversation started, by its key.
    pub(crate) fn started_at(&self, k: &str) -> Option<i64> {
        self.by_key.get(k).map(|i| self.metas[*i as usize].started_at)
    }

    /// How alike one message and a vector are: its best passage's
    /// likeness. `None` when the message has no passage here.
    pub(crate) fn message_likeness(&self, k: &str, seq: u32, to: &[f32]) -> Option<f32> {
        let conv = *self.by_key.get(k)?;
        if self.dims == 0 || to.len() != self.dims {
            return None;
        }
        let q = unit(to);
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.conv == conv && r.seq == seq)
            .map(|(i, r)| score(&q, &self.bytes[i * self.dims..(i + 1) * self.dims], r.scale))
            .fold(None, |best: Option<f32>, s| Some(best.map_or(s, |b| b.max(s))))
    }

    /// The messages closest to a question: a message's score is its best
    /// passage's; the first `at_most` messages are returned. `keep` says
    /// which passages may be found at all. Two messages with the very same
    /// best passage (a history imported twice) are one result.
    pub(crate) fn search(&self, question: &[f32], at_most: usize, keep: &dyn Fn(&Candidate) -> bool) -> Vec<Hit> {
        if self.dims == 0 || question.len() != self.dims {
            return Vec::new();
        }
        let q = unit(question);
        let mut best: HashMap<(u32, u32), (f32, usize)> = HashMap::new();
        for (i, r) in self.rows.iter().enumerate() {
            let c = Candidate { k: &self.keys[r.conv as usize], meta: &self.metas[r.conv as usize], seq: r.seq, flags: r.flags };
            if !keep(&c) {
                continue;
            }
            let s = score(&q, &self.bytes[i * self.dims..(i + 1) * self.dims], r.scale);
            let e = best.entry((r.conv, r.seq)).or_insert((f32::MIN, i));
            if s > e.0 {
                *e = (s, i);
            }
        }
        let mut ranked: Vec<((u32, u32), (f32, usize))> = best.into_iter().collect();
        // By score; ties by the older conversation, then the earlier message.
        ranked.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        let mut out: Vec<Hit> = Vec::new();
        let mut seen: Vec<usize> = Vec::new();
        for ((conv, seq), (s, i)) in ranked {
            if out.len() >= at_most {
                break;
            }
            let same = |a: usize, b: usize| self.rows[a].scale == self.rows[b].scale && self.bytes[a * self.dims..(a + 1) * self.dims] == self.bytes[b * self.dims..(b + 1) * self.dims];
            if seen.iter().any(|j| same(*j, i)) {
                continue;
            }
            seen.push(i);
            let r = self.rows[i];
            out.push(Hit { k: self.keys[conv as usize].clone(), seq, role: r.role, score: s, flags: r.flags });
        }
        out
    }
}

/// The filter every search starts from: sources searched by default, and
/// never a reply that was itself a search report.
pub(crate) fn searched_by_default(c: &Candidate) -> bool {
    source_is_searched_by_default(c.meta.source.as_deref()) && c.flags & FLAG_GIVEN_A_BLOCK == 0 && !c.meta.private
}

// ── the store ───────────────────────────────────────────────────────────

/// What must match for a store on disk to be used as it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Made {
    /// The embedding model's file name.
    pub model: String,
    /// The secret mask's version.
    pub mask: String,
}

pub(crate) struct Store {
    conn: Connection,
}

/// How a store came to be open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Opened {
    /// As it was left.
    AsLeft,
    /// New, or made again because the embedding model, the storage form or
    /// the mask changed.
    Empty,
    /// The file could not be read: it was replaced by an empty one.
    Rebuilt,
}

/// One conversation's reading state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ConvState {
    pub k: String,
    /// The highest sequence whose passages are all stored; -1 before any.
    pub read_through: i64,
    pub entries: i64,
    pub touched: i64,
    pub meta: ConvMeta,
}

/// One message's passages, ready to store.
pub(crate) struct ReadMessage {
    pub seq: u32,
    pub role: u8,
    pub flags: u8,
    pub passages: Vec<Quant>,
}

fn enc(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, plain)?;
    let mut out = Vec::with_capacity(24 + cipher.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&cipher);
    Ok(out)
}

fn dec(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 24 {
        return Err("memory index: blob too short".into());
    }
    crate::transcript_crypto::decrypt(key, &blob[..24], &blob[24..])
}

fn vec_blob(q: &Quant) -> Vec<u8> {
    let mut out: Vec<u8> = q.bytes.iter().map(|b| *b as u8).collect();
    out.extend_from_slice(&q.scale.to_le_bytes());
    out
}

fn blob_vec(plain: &[u8]) -> Option<Quant> {
    let n = plain.len().checked_sub(4)?;
    let scale = f32::from_le_bytes(plain[n..].try_into().ok()?);
    Some(Quant { bytes: plain[..n].iter().map(|b| *b as i8).collect(), scale })
}

const SCHEMA: &str = "
  PRAGMA journal_mode = WAL;
  CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v BLOB NOT NULL);
  CREATE TABLE IF NOT EXISTS convs (
    k TEXT PRIMARY KEY,
    read_through INTEGER NOT NULL,
    entries INTEGER NOT NULL,
    touched INTEGER NOT NULL,
    meta_enc BLOB NOT NULL
  );
  CREATE TABLE IF NOT EXISTS passages (
    k TEXT NOT NULL, seq INTEGER NOT NULL, idx INTEGER NOT NULL,
    role INTEGER NOT NULL,
    flags INTEGER NOT NULL,
    vec_enc BLOB NOT NULL,
    PRIMARY KEY (k, seq, idx)
  );
  CREATE TABLE IF NOT EXISTS item_vecs (id TEXT PRIMARY KEY, updated_at INTEGER NOT NULL, vec_enc BLOB NOT NULL);
";

impl Store {
    /// Open the store at `path`, made for `made`. A store made for anything
    /// else is emptied (the reading job fills it again); a file that cannot
    /// be read is replaced.
    pub(crate) fn open(path: &Path, made: &Made) -> Result<(Store, Opened), String> {
        let existed = path.exists();
        match Self::open_checked(path, made) {
            Ok((store, as_left)) => Ok((store, if as_left && existed { Opened::AsLeft } else { Opened::Empty })),
            Err(e) if existed => {
                log::warn!("[memory index] the store could not be read ({e}); it is rebuilt from the records");
                for suffix in ["", "-wal", "-shm"] {
                    let _ = std::fs::remove_file(PathBuf::from(format!("{}{suffix}", path.display())));
                }
                Self::open_checked(path, made).map(|(s, _)| (s, Opened::Rebuilt))
            }
            Err(e) => Err(e),
        }
    }

    fn open_checked(path: &Path, made: &Made) -> Result<(Store, bool), String> {
        let conn = Connection::open(path).map_err(|e| format!("memory index: {e}"))?;
        let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
        conn.execute_batch(SCHEMA).map_err(|e| format!("memory index schema: {e}"))?;
        // A damaged file can open and still fail on its first real read.
        conn.query_row("SELECT count(*) FROM passages", [], |r| r.get::<_, i64>(0)).map_err(|e| format!("memory index: {e}"))?;
        let want = [("format", FORMAT), ("model", made.model.as_str()), ("quant", QUANT), ("cut", CUT), ("mask", made.mask.as_str())];
        let have = |k: &str| conn.query_row("SELECT v FROM meta WHERE k = ?1", params![k], |r| r.get::<_, Vec<u8>>(0)).ok();
        let fresh = have("format").is_none();
        let as_left = !fresh && want.iter().all(|(k, v)| have(k).as_deref() == Some(v.as_bytes()));
        if !as_left {
            if !fresh {
                log::info!("[memory index] made for another model, storage form or mask: emptied, read again");
            }
            conn.execute_batch("DELETE FROM passages; DELETE FROM convs; DELETE FROM item_vecs; DELETE FROM meta;").map_err(|e| e.to_string())?;
            for (k, v) in want {
                conn.execute("INSERT INTO meta (k, v) VALUES (?1, ?2)", params![k, v.as_bytes()]).map_err(|e| e.to_string())?;
            }
        }
        Ok((Store { conn }, as_left))
    }

    /// Store a batch of one conversation's messages and move its resume
    /// point, in one transaction: a stop loses at most the batch in hand.
    pub(crate) fn put_batch(&mut self, key: &[u8; 32], state: &ConvState, messages: &[ReadMessage]) -> Result<(), String> {
        let meta_enc = enc(key, &serde_json::to_vec(&state.meta).map_err(|e| e.to_string())?)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        for m in messages {
            tx.execute("DELETE FROM passages WHERE k = ?1 AND seq = ?2", params![state.k, m.seq]).map_err(|e| e.to_string())?;
            for (idx, q) in m.passages.iter().enumerate() {
                tx.execute(
                    "INSERT INTO passages (k, seq, idx, role, flags, vec_enc) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![state.k, m.seq, idx as i64, m.role, m.flags, enc(key, &vec_blob(q))?],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.execute(
            "INSERT INTO convs (k, read_through, entries, touched, meta_enc) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(k) DO UPDATE SET read_through = ?2, entries = ?3, touched = ?4, meta_enc = ?5",
            params![state.k, state.read_through, state.entries, state.touched, meta_enc],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Remove a conversation: its passages and its reading state.
    pub(crate) fn forget(&mut self, k: &str) -> Result<usize, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let n = tx.execute("DELETE FROM passages WHERE k = ?1", params![k]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM convs WHERE k = ?1", params![k]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// Every conversation's reading state.
    pub(crate) fn convs(&self, key: &[u8; 32]) -> Result<Vec<ConvState>, String> {
        let mut st = self.conn.prepare("SELECT k, read_through, entries, touched, meta_enc FROM convs").map_err(|e| e.to_string())?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Vec<u8>>(4)?)))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let (k, read_through, entries, touched, meta_enc) = row.map_err(|e| e.to_string())?;
            let meta = serde_json::from_slice(&dec(key, &meta_enc)?).map_err(|e| e.to_string())?;
            out.push(ConvState { k, read_through, entries, touched, meta });
        }
        Ok(out)
    }

    /// Store an item's vector, with a stamp of the words it was made from
    /// (other words are embedded again).
    pub(crate) fn item_vec_put(&self, key: &[u8; 32], id: &str, stamp: i64, q: &Quant) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO item_vecs (id, updated_at, vec_enc) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET updated_at = ?2, vec_enc = ?3",
                params![id, stamp, enc(key, &vec_blob(q))?],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Every item's vector: its id, the stamp of the words it was made
    /// from, the vector. One that cannot be opened is left out (it is made again).
    pub(crate) fn item_vecs(&self, key: &[u8; 32]) -> Result<Vec<(String, i64, Quant)>, String> {
        let mut st = self.conn.prepare("SELECT id, updated_at, vec_enc FROM item_vecs").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?))).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let (id, updated_at, vec_enc) = row.map_err(|e| e.to_string())?;
            if let Some(q) = dec(key, &vec_enc).ok().and_then(|p| blob_vec(&p)) {
                out.push((id, updated_at, q));
            }
        }
        Ok(out)
    }

    pub(crate) fn passage_count(&self) -> Result<usize, String> {
        self.conn.query_row("SELECT count(*) FROM passages", [], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(|e| e.to_string())
    }

    /// Every vector, into memory.
    pub(crate) fn load(&self, key: &[u8; 32]) -> Result<Cache, String> {
        let metas: HashMap<String, ConvMeta> = self.convs(key)?.into_iter().map(|c| (c.k, c.meta)).collect();
        let mut cache = Cache::default();
        let mut st = self.conn.prepare("SELECT k, seq, role, flags, vec_enc FROM passages ORDER BY k, seq, idx").map_err(|e| e.to_string())?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, u8>(2)?, r.get::<_, u8>(3)?, r.get::<_, Vec<u8>>(4)?)))
            .map_err(|e| e.to_string())?;
        let none = ConvMeta::default();
        for row in rows {
            let (k, seq, role, flags, vec_enc) = row.map_err(|e| e.to_string())?;
            let q = blob_vec(&dec(key, &vec_enc)?).ok_or("memory index: a vector is cut short")?;
            cache.append(&k, metas.get(&k).unwrap_or(&none), seq as u32, role, flags, &q);
        }
        Ok(cache)
    }
}

/// The order conversations are read in: the most recently active first.
/// `listed` is what the records say now (key, entries, last active);
/// `known` what the index holds. Only conversations with something unread
/// are returned, each with the sequence to start from.
pub(crate) fn to_read(listed: &[(String, i64, i64)], known: &[ConvState]) -> Vec<(String, i64)> {
    let have: HashMap<&str, &ConvState> = known.iter().map(|c| (c.k.as_str(), c)).collect();
    let mut todo: Vec<(&str, i64, i64)> = listed
        .iter()
        .filter_map(|(k, entries, touched)| match have.get(k.as_str()) {
            Some(c) if c.read_through + 1 >= *entries => None,
            Some(c) => Some((k.as_str(), c.read_through + 1, *touched)),
            None if *entries > 0 => Some((k.as_str(), 0, *touched)),
            None => None,
        })
        .collect();
    todo.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)));
    todo.into_iter().map(|(k, from, _)| (k.to_string(), from)).collect()
}

/// What share of the listed messages can be searched, as (read, total).
pub(crate) fn share_read(listed: &[(String, i64, i64)], known: &[ConvState]) -> (i64, i64) {
    let have: HashMap<&str, i64> = known.iter().map(|c| (c.k.as_str(), c.read_through + 1)).collect();
    let total: i64 = listed.iter().map(|l| l.1).sum();
    let read: i64 = listed.iter().map(|(k, entries, _)| have.get(k.as_str()).copied().unwrap_or(0).min(*entries)).sum();
    (read, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    fn made() -> Made {
        Made { model: "embed-a.gguf".into(), mask: "mask-1".into() }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("memory-index-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("memory-index.sqlite")
    }

    /// A made-up vector of 8 numbers that points mostly along `axis`.
    fn v(axis: usize, lean: f32) -> Vec<f32> {
        let mut x = vec![0.05f32; 8];
        x[axis] = 1.0;
        x[(axis + 1) % 8] = lean;
        x
    }

    fn meta(started_at: i64) -> ConvMeta {
        ConvMeta { started_at, ai: "founding-a".into(), ..Default::default() }
    }

    fn message(seq: u32, role: u8, vectors: &[Vec<f32>]) -> ReadMessage {
        ReadMessage { seq, role, flags: 0, passages: vectors.iter().map(|x| quantize(x)).collect() }
    }

    fn state(k: &str, read_through: i64, entries: i64, touched: i64) -> ConvState {
        ConvState { k: k.into(), read_through, entries, touched, meta: meta(touched) }
    }

    fn all(_: &Candidate) -> bool {
        true
    }

    #[test]
    fn cut_matches_the_measured_cut() {
        let text: String = (0..1900).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let passages = cut(&text);
        assert_eq!(passages.len(), 3);
        assert_eq!(passages[0], text[0..1000]);
        assert_eq!(passages[1], text[900..1900]);
        assert_eq!(passages[2], text[1800..1900]);
        assert_eq!(cut(&text[..900]).len(), 1);
        assert_eq!(cut(&text[..901]).len(), 2);
        assert_eq!(cut(&text[..901])[1], text[900..901]);
        assert_eq!(cut(""), vec!["(empty)"]);
        assert_eq!(cut("short"), vec!["short"]);
        // Characters, not bytes: accented text is cut between characters.
        let accented = "é".repeat(1000);
        assert_eq!(cut(&accented).iter().map(|p| p.chars().count()).collect::<Vec<_>>(), vec![1000, 100]);
    }

    #[test]
    fn one_byte_roundtrip_ranks_the_same() {
        // Made-up vectors of the real size, spread around a question.
        let mut seed = 99u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f32 / (1u64 << 31) as f32) - 0.5
        };
        let question: Vec<f32> = (0..768).map(|_| next()).collect();
        let vectors: Vec<Vec<f32>> = (0..200).map(|i| (0..768).map(|d| next() + question[d] * (i as f32 / 200.0)).collect()).collect();
        let exact = |x: &[f32]| unit(&question).iter().zip(unit(x)).map(|(a, b)| a * b).sum::<f32>();
        let mut by_exact: Vec<usize> = (0..200).collect();
        by_exact.sort_by(|a, b| exact(&vectors[*b]).partial_cmp(&exact(&vectors[*a])).unwrap());
        let mut cache = Cache::default();
        for (i, x) in vectors.iter().enumerate() {
            assert!(cache.append("c", &meta(1), i as u32, ROLE_PERSON, 0, &quantize(x)));
        }
        let found = cache.search(&question, 12, &all);
        assert_eq!(found.iter().map(|h| h.seq as usize).collect::<Vec<_>>()[..3], by_exact[..3], "the first three are the same");
        let same = found.iter().filter(|h| by_exact[..12].contains(&(h.seq as usize))).count();
        assert!(same >= 11, "{same} of the first 12 are shared");
        // The score is the cosine, to within the rounding.
        assert!((found[0].score - exact(&vectors[by_exact[0]])).abs() < 0.01);
        // One byte per number, plus a little per passage.
        assert!(cache.bytes_held() < 200 * (768 + 32));
    }

    #[test]
    fn both_speakers_are_indexed() {
        let mut cache = Cache::default();
        cache.append("c", &meta(1), 0, ROLE_PERSON, 0, &quantize(&v(0, 0.1)));
        cache.append("c", &meta(1), 1, ROLE_AI, 0, &quantize(&v(0, 0.0)));
        let found = cache.search(&v(0, 0.0), 12, &all);
        assert_eq!(found.iter().map(|h| (h.seq, h.role)).collect::<Vec<_>>(), vec![(1, ROLE_AI), (0, ROLE_PERSON)]);
    }

    #[test]
    fn no_cap() {
        let mut cache = Cache::default();
        for i in 0..5000u32 {
            cache.append("c", &meta(1), i, ROLE_PERSON, 0, &quantize(&v((i % 8) as usize, (i as f32) / 5000.0)));
        }
        assert_eq!(cache.passages(), 5000);
        // The very first message is still found.
        let found = cache.search(&v(0, 0.0), 1, &all);
        assert_eq!(found[0].seq, 0);
    }

    #[test]
    fn a_messages_score_is_its_best_passage_and_twelve_messages_come_back() {
        let mut cache = Cache::default();
        // One long message of three passages; the middle one answers.
        for q in [v(3, 0.0), v(0, 0.02), v(5, 0.0)] {
            cache.append("c", &meta(1), 0, ROLE_AI, 0, &quantize(&q));
        }
        for i in 1..30u32 {
            cache.append("c", &meta(1), i, ROLE_PERSON, 0, &quantize(&v(0, 0.2 + i as f32 / 30.0)));
        }
        let found = cache.search(&v(0, 0.0), FOUND_AT_MOST, &all);
        assert_eq!(found.len(), 12);
        assert_eq!(found[0].seq, 0, "found by its middle passage");
        assert_eq!(found.iter().filter(|h| h.seq == 0).count(), 1, "one result per message");
    }

    #[test]
    fn append_does_not_drop_the_cache() {
        let mut cache = Cache::default();
        cache.append("old", &meta(1), 0, ROLE_PERSON, 0, &quantize(&v(1, 0.0)));
        let before = cache.search(&v(1, 0.0), 12, &all);
        cache.append("new", &meta(2), 0, ROLE_PERSON, 0, &quantize(&v(2, 0.0)));
        assert_eq!(cache.passages(), 2);
        assert_eq!(cache.search(&v(1, 0.0), 1, &all), before[..1]);
        assert_eq!(cache.search(&v(2, 0.0), 1, &all)[0].k, "new");
        // A vector from another embedding model is refused, and changes nothing.
        assert!(!cache.append("new", &meta(2), 1, ROLE_PERSON, 0, &quantize(&[1.0, 0.0, 0.0])));
        assert_eq!(cache.passages(), 2);
    }

    #[test]
    fn delete_removes_passages() {
        let path = scratch("delete");
        let (mut store, _) = Store::open(&path, &made()).unwrap();
        store.put_batch(&KEY, &state("gone", 1, 2, 10), &[message(0, ROLE_PERSON, &[v(0, 0.0)]), message(1, ROLE_AI, &[v(1, 0.0), v(2, 0.0)])]).unwrap();
        store.put_batch(&KEY, &state("kept", 0, 1, 20), &[message(0, ROLE_PERSON, &[v(0, 0.1)])]).unwrap();
        let mut cache = store.load(&KEY).unwrap();
        assert_eq!(cache.passages(), 4);
        assert_eq!(store.forget("gone").unwrap(), 3);
        assert_eq!(cache.forget("gone"), 3);
        assert_eq!(store.passage_count().unwrap(), 1);
        assert_eq!(store.convs(&KEY).unwrap().iter().map(|c| c.k.as_str()).collect::<Vec<_>>(), vec!["kept"]);
        assert_eq!(cache.search(&v(0, 0.0), 12, &all).iter().map(|h| h.k.as_str()).collect::<Vec<_>>(), vec!["kept"]);
        // What is read from disk afterwards agrees.
        assert_eq!(store.load(&KEY).unwrap().search(&v(0, 0.0), 12, &all).len(), 1);
        // A conversation added after a delete is found under its own key.
        cache.append("later", &meta(3), 0, ROLE_PERSON, 0, &quantize(&v(4, 0.0)));
        assert_eq!(cache.search(&v(4, 0.0), 1, &all)[0].k, "later");
    }

    #[test]
    fn resume_after_a_stop_loses_at_most_one_batch() {
        let path = scratch("resume");
        let (mut store, opened) = Store::open(&path, &made()).unwrap();
        assert_eq!(opened, Opened::Empty);
        // Two batches of four are stored; the third is cut off before it is.
        for batch in 0..2i64 {
            let messages: Vec<ReadMessage> = (0..4).map(|i| message((batch * 4 + i) as u32, ROLE_PERSON, &[v(i as usize, 0.0)])).collect();
            store.put_batch(&KEY, &state("c", batch * 4 + 3, 12, 50), &messages).unwrap();
        }
        drop(store);
        let (store, opened) = Store::open(&path, &made()).unwrap();
        assert_eq!(opened, Opened::AsLeft);
        let known = store.convs(&KEY).unwrap();
        assert_eq!(known[0].read_through, 7);
        assert_eq!(store.passage_count().unwrap(), 8);
        // Reading carries on from message 8, not from the start.
        assert_eq!(to_read(&[("c".into(), 12, 50)], &known), vec![("c".to_string(), 8)]);
        assert_eq!(share_read(&[("c".into(), 12, 50)], &known), (8, 12));
        // A batch stored twice (the stop came after the write, before the
        // reader knew) leaves each message once.
        let (mut store, _) = Store::open(&path, &made()).unwrap();
        let again: Vec<ReadMessage> = (4..8).map(|i| message(i, ROLE_PERSON, &[v(0, 0.0)])).collect();
        store.put_batch(&KEY, &state("c", 7, 12, 50), &again).unwrap();
        assert_eq!(store.passage_count().unwrap(), 8);
    }

    #[test]
    fn newest_first() {
        let known = vec![state("read", 4, 5, 10), state("grew", 2, 5, 30)];
        let listed = vec![("old".to_string(), 3, 5), ("read".to_string(), 5, 10), ("newest".to_string(), 2, 90), ("grew".to_string(), 6, 40), ("empty".to_string(), 0, 99)];
        assert_eq!(to_read(&listed, &known), vec![("newest".to_string(), 0), ("grew".to_string(), 3), ("old".to_string(), 0)]);
        assert_eq!(share_read(&listed, &known), (5 + 3, 3 + 5 + 2 + 6));
        // A conversation that lost messages is never counted as more than it holds.
        assert_eq!(share_read(&[("read".into(), 2, 10)], &known), (2, 2));
    }

    #[test]
    fn source_rule() {
        assert!(source_is_indexed(None) && source_is_indexed(Some("import:chatgpt")) && source_is_indexed(Some("some-editor/1.2")));
        assert!(!source_is_indexed(Some("workspace-memory")));
        assert!(source_is_searched_by_default(None));
        assert!(source_is_searched_by_default(Some("import:chatgpt")));
        assert!(source_is_searched_by_default(Some("copy:device-b")));
        assert!(!source_is_searched_by_default(Some("some-editor/1.2")));
        // Indexed, and left out of an ordinary search.
        let mut cache = Cache::default();
        cache.append("app", &meta(1), 0, ROLE_PERSON, 0, &quantize(&v(0, 0.2)));
        cache.append("port", &ConvMeta { source: Some("some-editor/1.2".into()), ..meta(2) }, 0, ROLE_PERSON, 0, &quantize(&v(0, 0.0)));
        assert_eq!(cache.passages(), 2);
        assert_eq!(cache.search(&v(0, 0.0), 12, &searched_by_default).iter().map(|h| h.k.as_str()).collect::<Vec<_>>(), vec!["app"]);
        // "Use these": the person's say-so widens the filter.
        let with_port = |c: &Candidate| c.flags & FLAG_GIVEN_A_BLOCK == 0;
        assert_eq!(cache.search(&v(0, 0.0), 12, &with_port)[0].k, "port");
    }

    #[test]
    fn duplicate_passages_collapse() {
        let mut cache = Cache::default();
        // The same history imported twice: the same words, the same vectors.
        cache.append("import-1", &meta(1), 4, ROLE_PERSON, 0, &quantize(&v(0, 0.0)));
        cache.append("import-2", &meta(2), 4, ROLE_PERSON, 0, &quantize(&v(0, 0.0)));
        cache.append("other", &meta(3), 0, ROLE_PERSON, 0, &quantize(&v(0, 0.3)));
        let found = cache.search(&v(0, 0.0), 12, &all);
        assert_eq!(found.iter().map(|h| h.k.as_str()).collect::<Vec<_>>(), vec!["import-1", "other"]);
    }

    #[test]
    fn a_reply_given_a_block_is_left_out() {
        let mut cache = Cache::default();
        cache.append("c", &meta(1), 0, ROLE_PERSON, 0, &quantize(&v(0, 0.3)));
        cache.append("c", &meta(1), 1, ROLE_AI, FLAG_GIVEN_A_BLOCK, &quantize(&v(0, 0.0)));
        cache.append("c", &meta(1), 2, ROLE_AI, FLAG_STOPPED | FLAG_SECRET, &quantize(&v(0, 0.1)));
        let found = cache.search(&v(0, 0.0), 12, &searched_by_default);
        assert_eq!(found.iter().map(|h| h.seq).collect::<Vec<_>>(), vec![2, 0]);
        assert_eq!(found[0].flags, FLAG_STOPPED | FLAG_SECRET, "the flags come back with the hit");
    }

    #[test]
    fn audience_filter() {
        let mut cache = Cache::default();
        cache.append("with-a", &meta(1), 0, ROLE_PERSON, 0, &quantize(&v(0, 0.0)));
        cache.append("with-b", &ConvMeta { ai: "founding-b".into(), ..meta(2) }, 0, ROLE_PERSON, 0, &quantize(&v(0, 0.1)));
        cache.append("private", &ConvMeta { private: true, ..meta(3) }, 0, ROLE_PERSON, 0, &quantize(&v(0, 0.05)));
        cache.append("in-project", &ConvMeta { project: Some("id:p1".into()), ..meta(4) }, 0, ROLE_PERSON, 0, &quantize(&v(0, 0.2)));
        // An AI that may use only its own conversations.
        let own = |c: &Candidate| searched_by_default(c) && c.meta.ai == "founding-b";
        assert_eq!(cache.search(&v(0, 0.0), 12, &own).iter().map(|h| h.k.as_str()).collect::<Vec<_>>(), vec!["with-b"]);
        // A private conversation is found by nobody.
        assert!(cache.search(&v(0, 0.0), 12, &searched_by_default).iter().all(|h| h.k != "private"));
        // One project's conversations.
        let project = |c: &Candidate| c.meta.project.as_deref() == Some("id:p1");
        assert_eq!(cache.search(&v(0, 0.0), 12, &project).len(), 1);
        // Messages already being sent with the open conversation.
        let not_on_the_wire = |c: &Candidate| !(c.k == "with-a" && c.seq == 0);
        assert!(cache.search(&v(0, 0.0), 12, &not_on_the_wire).iter().all(|h| h.k != "with-a"));
    }

    #[test]
    fn damaged_store_rebuilds() {
        let path = scratch("damaged");
        {
            let (mut store, _) = Store::open(&path, &made()).unwrap();
            store.put_batch(&KEY, &state("c", 0, 1, 1), &[message(0, ROLE_PERSON, &[v(0, 0.0)])]).unwrap();
        }
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(PathBuf::from(format!("{}{suffix}", path.display())));
        }
        std::fs::write(&path, b"this is not a database, it is a damaged file of some length").unwrap();
        let (store, opened) = Store::open(&path, &made()).unwrap();
        assert_eq!(opened, Opened::Rebuilt);
        assert_eq!(store.passage_count().unwrap(), 0);
        assert!(store.convs(&KEY).unwrap().is_empty());
    }

    #[test]
    fn another_model_or_mask_empties_the_store() {
        let path = scratch("model");
        {
            let (mut store, _) = Store::open(&path, &made()).unwrap();
            store.put_batch(&KEY, &state("c", 0, 1, 1), &[message(0, ROLE_PERSON, &[v(0, 0.0)])]).unwrap();
        }
        let (store, opened) = Store::open(&path, &made()).unwrap();
        assert_eq!((opened, store.passage_count().unwrap()), (Opened::AsLeft, 1));
        drop(store);
        let (store, opened) = Store::open(&path, &Made { model: "embed-b.gguf".into(), ..made() }).unwrap();
        assert_eq!((opened, store.passage_count().unwrap()), (Opened::Empty, 0));
        assert!(store.convs(&KEY).unwrap().is_empty(), "the resume points go with the passages");
        drop(store);
        let (mut store, _) = Store::open(&path, &Made { model: "embed-b.gguf".into(), ..made() }).unwrap();
        store.put_batch(&KEY, &state("c", 0, 1, 1), &[message(0, ROLE_PERSON, &[v(0, 0.0)])]).unwrap();
        drop(store);
        let (store, opened) = Store::open(&path, &Made { model: "embed-b.gguf".into(), mask: "mask-2".into() }).unwrap();
        assert_eq!((opened, store.passage_count().unwrap()), (Opened::Empty, 0));
    }

    #[test]
    fn nothing_readable_is_on_disk_without_the_key() {
        let path = scratch("sealed");
        let (mut store, _) = Store::open(&path, &made()).unwrap();
        let m = ConvMeta { ai: "founding-key-of-the-bakery-assistant".into(), source: Some("import:somewhere-recognizable".into()), project: Some("id:project-recognizable".into()), ..meta(1) };
        store.put_batch(&KEY, &ConvState { k: "k1".into(), read_through: 0, entries: 1, touched: 1, meta: m.clone() }, &[message(0, ROLE_PERSON, &[v(0, 0.0)])]).unwrap();
        assert_eq!(store.convs(&KEY).unwrap()[0].meta, m);
        assert!(store.convs(&[8; 32]).is_err(), "another key opens nothing");
        drop(store);
        let mut raw = std::fs::read(&path).unwrap();
        raw.extend(std::fs::read(PathBuf::from(format!("{}-wal", path.display()))).unwrap_or_default());
        for word in ["founding-key-of-the-bakery", "somewhere-recognizable", "project-recognizable"] {
            assert!(!raw.windows(word.len()).any(|w| w == word.as_bytes()), "{word} is readable in the file");
        }
    }

    #[test]
    fn an_items_vector_is_kept_with_its_stamp_and_replaced_by_a_later_one() {
        let path = scratch("items");
        let (store, _) = Store::open(&path, &made()).unwrap();
        store.item_vec_put(&KEY, "it-1", 10, &quantize(&v(0, 0.0))).unwrap();
        store.item_vec_put(&KEY, "it-2", 11, &quantize(&v(3, 0.0))).unwrap();
        store.item_vec_put(&KEY, "it-1", 20, &quantize(&v(1, 0.0))).unwrap();
        let mut held = store.item_vecs(&KEY).unwrap();
        held.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(held.iter().map(|(id, at, _)| (id.as_str(), *at)).collect::<Vec<_>>(), vec![("it-1", 20), ("it-2", 11)]);
        // Alike to what it was made from, unlike another direction.
        assert!(likeness(&v(1, 0.0), &held[0].2) > 0.99);
        assert!(likeness(&v(5, 0.0), &held[0].2) < 0.3);
        assert_eq!(likeness(&[1.0, 0.0], &held[0].2), 0.0, "a vector of another size is not alike");
        assert!(store.item_vecs(&[8; 32]).unwrap().is_empty(), "another key opens none of them");
        // The numbers come back close to the unit vector they were.
        let back = numbers(&held[1].2);
        assert!(unit(&v(3, 0.0)).iter().zip(&back).all(|(a, b)| (a - b).abs() < 0.01));
        // A store made again for another model drops them with the passages.
        drop(store);
        let (store, _) = Store::open(&path, &Made { model: "embed-b.gguf".into(), ..made() }).unwrap();
        assert!(store.item_vecs(&KEY).unwrap().is_empty());
    }

    #[test]
    fn a_message_is_as_alike_as_its_best_passage() {
        let mut cache = Cache::default();
        cache.append("c", &meta(77), 4, ROLE_PERSON, 0, &quantize(&v(0, 0.0)));
        cache.append("c", &meta(77), 4, ROLE_PERSON, 0, &quantize(&v(2, 0.0)));
        cache.append("c", &meta(77), 5, ROLE_AI, 0, &quantize(&v(6, 0.0)));
        assert_eq!(cache.started_at("c"), Some(77));
        assert_eq!(cache.started_at("other"), None);
        assert!(cache.message_likeness("c", 4, &v(2, 0.0)).unwrap() > 0.99);
        assert!(cache.message_likeness("c", 5, &v(2, 0.0)).unwrap() < 0.3);
        assert_eq!(cache.message_likeness("c", 9, &v(2, 0.0)), None);
        assert_eq!(cache.message_likeness("other", 4, &v(2, 0.0)), None);
    }
}
