//! The transcript cells on the 0.7 line (DNA v2): one per AI cell on the
//! 0.6 line, with the same agent key (the 0.7 key store is a copy of the 0.6
//! one) and the person's network seed. Calls go through flowsta-hc-lines'
//! `AppClient`; inputs are hashes and bytes, which both lines serialize
//! alike, and outputs holding records decode with the 0.7 types into the
//! library's line-free `RecordView`.

use flowsta_hc_lines::admin::AppConnect;
use flowsta_hc_lines::v07::holochain_types::prelude::Record as Record07;
use flowsta_hc_lines::{AdminClient, AppClient, InstallRequest, Line, RecordView};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

pub const ROLE: &str = "transcript";
pub const ZOME: &str = "transcript";
/// The v2 app of `transcript_v1_<suffix>` is `transcript_v2_<suffix>`.
pub const APP_ID_PREFIX: &str = "transcript_v2_";
/// The bundle (in `resources/`), copied from `dna/v2/workdir`; never rebuilt.
pub const HAPP_FILENAME: &str = "yourown_ai_transcript_v2_happ.happ";
/// Its SHA-256, as `DNA_VERSION_HISTORY.md` records it.
pub const HAPP_SHA256: &str = "7c3a6b7b452e0b4f9b95f193d9cc0ea0670bbba2edbb091e112a1dc4cad4d77f";

/// The v2 app id for a v1 app id; None for an app that is not a transcript.
pub fn app_id_for(v1_app_id: &str) -> Option<String> {
    v1_app_id.strip_prefix(crate::dna::APP_ID_PREFIX).map(|suffix| format!("{APP_ID_PREFIX}{suffix}"))
}

async fn admin() -> Result<AdminClient, String> {
    let (port, _) = crate::conductor::ports(Line::V07).ok_or("the 0.7 line is not running")?;
    AdminClient::connect(Line::V07, port, &crate::conductor::origin()).await.map_err(|e| format!("0.7 admin: {e}"))
}

/// Make sure each v1 transcript app has its v2 app: an app that exists is
/// adopted (enabled if it is not), never reinstalled, since a second install
/// under a key that has a 0.7 chain forks it. `v1_apps` is (app id, agent
/// key 39 bytes). Returns how many apps were installed.
pub async fn ensure_apps(resource_dir: &Path, network_seed: &str, v1_apps: &[(String, Vec<u8>)]) -> Result<usize, String> {
    ensure_apps_on(&admin().await?, resource_dir, network_seed, v1_apps).await
}

/// [`ensure_apps`] on a 0.7 admin connection the caller holds (the line's
/// setup, before its ports are handed out).
pub async fn ensure_apps_on(admin: &AdminClient, resource_dir: &Path, network_seed: &str, v1_apps: &[(String, Vec<u8>)]) -> Result<usize, String> {
    let bundle = resource_dir.join(HAPP_FILENAME);
    if !bundle.is_file() {
        return Err(format!("the 0.7 transcript bundle is missing at {bundle:?}"));
    }
    let have = admin.list_apps().await.map_err(|e| format!("0.7 list_apps: {e}"))?;
    let mut installed = 0;
    for (v1_app_id, agent) in v1_apps {
        let Some(app_id) = app_id_for(v1_app_id) else { continue };
        match have.iter().find(|a| a.app_id == app_id) {
            Some(a) if a.agent_key != *agent => {
                return Err(format!("{app_id} runs as another key than {v1_app_id}: left as it is"));
            }
            Some(a) if !a.enabled => admin.enable(&app_id).await.map_err(|e| format!("0.7 enable {app_id}: {e}"))?,
            Some(_) => {}
            None => {
                admin
                    .install(&InstallRequest { app_id: &app_id, bundle: &bundle, agent_key: agent, network_seed: Some(network_seed) })
                    .await
                    .map_err(|e| format!("0.7 install {app_id}: {e}"))?;
                admin.enable(&app_id).await.map_err(|e| format!("0.7 enable {app_id}: {e}"))?;
                log::info!("[0.7] installed {app_id}");
                installed += 1;
            }
        }
    }
    Ok(installed)
}

/// The app's connections to its v2 cells, one per agent (39-byte key, hex),
/// each with its signing grant kept for the session (a grant is a chain
/// write). A new 0.7 start (new ports, a new origin) needs a new
/// `Cells07`.
pub struct Cells07 {
    clients: Mutex<HashMap<String, Arc<AppClient>>>,
    /// The 0.7 ports the connections were made on: a new start (new ports,
    /// a new origin) drops them.
    ports: Mutex<Option<(u16, u16)>>,
    /// One write at a time per chain: two writes racing on one chain lose
    /// one to "source chain head has moved".
    chain_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Default for Cells07 {
    fn default() -> Self {
        Self { clients: Mutex::new(HashMap::new()), ports: Mutex::new(None), chain_locks: Mutex::new(HashMap::new()) }
    }
}

/// The app's own connections to its v2 cells (the move keeps its own).
pub fn app_cells() -> &'static Cells07 {
    static CELLS: std::sync::OnceLock<Cells07> = std::sync::OnceLock::new();
    CELLS.get_or_init(Cells07::default)
}

impl Cells07 {
    /// The connection to `agent`'s v2 cell.
    pub async fn client(&self, agent_hex: &str) -> Result<Arc<AppClient>, String> {
        let now = crate::conductor::ports(Line::V07);
        {
            let mut ports = self.ports.lock().await;
            if *ports != now {
                self.clients.lock().await.clear();
                *ports = now;
            }
        }
        if let Some(c) = self.clients.lock().await.get(agent_hex) {
            return Ok(c.clone());
        }
        let agent = hex::decode(agent_hex).map_err(|_| format!("bad agent key {agent_hex}"))?;
        let admin = admin().await?;
        let apps = admin.list_apps().await.map_err(|e| format!("0.7 list_apps: {e}"))?;
        let app = apps
            .iter()
            .find(|a| a.app_id.starts_with(APP_ID_PREFIX) && a.agent_key == agent)
            .ok_or_else(|| format!("no 0.7 transcript app for {agent_hex}"))?;
        let cell = admin.app_cell(&app.app_id, ROLE).await.map_err(|e| format!("0.7 cell of {}: {e}", app.app_id))?;
        let grant = admin.authorize_signing(&cell).await.map_err(|e| format!("0.7 grant for {}: {e}", app.app_id))?;
        let (_, app_port) = crate::conductor::ports(Line::V07).ok_or("the 0.7 line is not running")?;
        let client = admin
            .connect_app(AppConnect {
                app_port,
                origin: &crate::conductor::origin(),
                app_id: &app.app_id,
                cell: &cell,
                grant,
                request_timeout: std::time::Duration::from_secs(crate::dna::BACKUP_READ_TIMEOUT_SECS),
            })
            .await
            .map_err(|e| format!("0.7 connect {}: {e}", app.app_id))?;
        let client = Arc::new(client);
        self.clients.lock().await.insert(agent_hex.to_string(), client.clone());
        Ok(client)
    }

    /// A write on `agent`'s v2 chain: one at a time per chain, tried again
    /// when another write moved the chain's head under it.
    pub async fn write<I: Serialize, O: DeserializeOwned>(&self, agent_hex: &str, fn_name: &str, input: &I) -> Result<O, String> {
        let lock = self.chain_locks.lock().await.entry(agent_hex.to_string()).or_default().clone();
        let _one = lock.lock().await;
        let mut tries = 0;
        loop {
            tries += 1;
            match self.call(agent_hex, fn_name, input).await {
                Err(e) if tries < 3 && e.contains("head has moved") => tokio::time::sleep(std::time::Duration::from_millis(300)).await,
                other => return other,
            }
        }
    }

    /// Call `fn_name` on `agent`'s v2 cell.
    pub async fn call<I: Serialize, O: DeserializeOwned>(&self, agent_hex: &str, fn_name: &str, input: &I) -> Result<O, String> {
        let payload = rmp_serde::to_vec_named(input).map_err(|e| format!("encode {fn_name}: {e}"))?;
        let client = self.client(agent_hex).await?;
        let out = client.call(ROLE, ZOME, fn_name, payload).await.map_err(|e| format!("{fn_name}: {e}"))?;
        rmp_serde::from_slice(&out).map_err(|e| format!("decode {fn_name}: {e}"))
    }
}

// ── The v2 coordinator's inputs and outputs ─────────────────────────────
//
// The same field names and order as `dna/v2/zomes/transcript/coordinator`.
// Hashes are the app's own 0.6 types: both lines serialize them as the
// same 39 raw bytes.

use holochain_types::prelude::{AgentPubKey, AnyLinkableHash, ExternalHash, Timestamp};

#[derive(Serialize)]
pub struct StartConversation {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
    pub anchor: AgentPubKey,
    pub conversation_anchor: ExternalHash,
}

#[derive(Serialize)]
pub struct RecordMessage {
    pub conversation_anchor: ExternalHash,
    pub message_key: Vec<u8>,
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
}

#[derive(Serialize)]
pub struct SaveMemoryRow {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
    pub anchor: AnyLinkableHash,
}

#[derive(Serialize)]
pub struct DeleteConversation {
    pub anchor: AgentPubKey,
    pub conversation_anchor: ExternalHash,
}

#[derive(Serialize)]
pub struct ConversationsPage {
    pub anchor: AgentPubKey,
    pub before: Option<Timestamp>,
    pub limit: u32,
}

#[derive(Serialize)]
pub struct EntriesPage {
    pub conversation_anchor: ExternalHash,
    pub after: Option<Timestamp>,
    pub limit: u32,
}

#[derive(Serialize)]
pub struct MemoryRows {
    pub anchor: AnyLinkableHash,
}

#[derive(Serialize)]
pub struct MyEntriesPage {
    pub anchor: AgentPubKey,
    pub after_seq: Option<u32>,
    pub limit: u32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct KeyItem {
    pub key: Vec<u8>,
    pub at: Timestamp,
    pub author: AgentPubKey,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ConversationKeys {
    pub conversations: Vec<KeyItem>,
    pub tombstoned: Vec<Vec<u8>>,
}

#[derive(Deserialize)]
struct PageItemRaw {
    record: Record07,
    key: Vec<u8>,
}

#[derive(Deserialize)]
struct RecordsPageRaw {
    items: Vec<PageItemRaw>,
    next: Option<Timestamp>,
    total: u32,
    missing: u32,
}

#[derive(Deserialize)]
struct MyEntryRaw {
    record: Record07,
    conversation_anchor: ExternalHash,
    key: Vec<u8>,
}

#[derive(Deserialize)]
struct MyEntriesPageRaw {
    items: Vec<MyEntryRaw>,
    next_seq: Option<u32>,
    total: u32,
}

/// A page of records with the keys their links carry.
pub struct RecordsPage {
    pub items: Vec<(RecordView, Vec<u8>)>,
    pub next: Option<Timestamp>,
    pub total: u32,
    pub missing: u32,
}

/// A page of this agent's own entries, each with its conversation's anchor.
pub struct MyEntries {
    pub items: Vec<(RecordView, ExternalHash, Vec<u8>)>,
    pub next_seq: Option<u32>,
    pub total: u32,
}

fn view(r: &Record07) -> Result<RecordView, String> {
    flowsta_hc_lines::view_07(r).map_err(|e| e.to_string())
}

impl Cells07 {
    pub async fn conversations_page(&self, agent_hex: &str, input: &ConversationsPage) -> Result<RecordsPage, String> {
        let raw: RecordsPageRaw = self.call(agent_hex, "get_conversations_page", input).await?;
        let items = raw.items.iter().map(|i| Ok((view(&i.record)?, i.key.clone()))).collect::<Result<_, String>>()?;
        Ok(RecordsPage { items, next: raw.next, total: raw.total, missing: raw.missing })
    }

    pub async fn entries_page(&self, agent_hex: &str, input: &EntriesPage) -> Result<RecordsPage, String> {
        let raw: RecordsPageRaw = self.call(agent_hex, "get_conversation_entries_page", input).await?;
        let items = raw.items.iter().map(|i| Ok((view(&i.record)?, i.key.clone()))).collect::<Result<_, String>>()?;
        Ok(RecordsPage { items, next: raw.next, total: raw.total, missing: raw.missing })
    }

    pub async fn my_entries_page(&self, agent_hex: &str, input: &MyEntriesPage) -> Result<MyEntries, String> {
        let raw: MyEntriesPageRaw = self.call(agent_hex, "get_my_entries_page", input).await?;
        let items = raw.items.iter().map(|i| Ok((view(&i.record)?, i.conversation_anchor.clone(), i.key.clone()))).collect::<Result<_, String>>()?;
        Ok(MyEntries { items, next_seq: raw.next_seq, total: raw.total })
    }

    /// Every conversation listed under `founding`, with its anchor.
    pub async fn conversations_all(&self, agent_hex: &str, founding: &AgentPubKey) -> Result<Vec<(RecordView, Vec<u8>)>, String> {
        let mut out = Vec::new();
        let mut before = None;
        loop {
            let page = self.conversations_page(agent_hex, &ConversationsPage { anchor: founding.clone(), before, limit: 100 }).await?;
            out.extend(page.items);
            match page.next {
                Some(n) => before = Some(n),
                None => return Ok(out),
            }
        }
    }

    /// Every entry under a conversation's anchor, oldest first, with its key.
    pub async fn entries_all(&self, agent_hex: &str, conversation_anchor: &ExternalHash) -> Result<Vec<(RecordView, Vec<u8>)>, String> {
        let mut out = Vec::new();
        let mut after = None;
        loop {
            let page = self.entries_page(agent_hex, &EntriesPage { conversation_anchor: conversation_anchor.clone(), after, limit: 500 }).await?;
            out.extend(page.items);
            match page.next {
                Some(n) => after = Some(n),
                None => return Ok(out),
            }
        }
    }

    pub async fn memory_rows(&self, agent_hex: &str, anchor: AnyLinkableHash) -> Result<Vec<RecordView>, String> {
        let raw: Vec<Record07> = self.call(agent_hex, "get_memory_rows", &MemoryRows { anchor }).await?;
        raw.iter().map(view).collect()
    }
}

// ── Writes to both lines ────────────────────────────────────────────────
//
// While the 0.6 line is in use, every record is written there first (its v1
// hash is its id), then here with that id. A write here runs in the
// background, so nothing the person does waits on the 0.7 line, and one
// that fails is carried by the move's next pass: the record is on 0.6.

fn agent(hex_key: &str) -> Option<AgentPubKey> {
    hex::decode(hex_key).ok().filter(|b| b.len() == 39).map(AgentPubKey::from_raw_39)
}

fn sealed(data_key: &[u8; 32], plain: &serde_json::Value) -> Option<(Vec<u8>, Vec<u8>)> {
    let bytes = serde_json::to_vec(plain).ok()?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(data_key, &bytes).ok()?;
    Some((cipher, nonce.to_vec()))
}

fn in_background(what: &'static str, agent_hex: String, job: impl std::future::Future<Output = Result<(), String>> + Send + 'static) {
    if crate::conductor::ports(Line::V07).is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = job.await {
            log::info!("[0.7] {what} for {}.. not written yet ({e}); the move's next pass carries it", &agent_hex[..8.min(agent_hex.len())]);
        }
    });
}

/// A conversation just written to the 0.6 line as `id`, written here too.
pub fn also_start_conversation(agent_hex: &str, founding_hex: &str, data_key: [u8; 32], id: String, plain: serde_json::Value) {
    let (agent_hex, founding_hex) = (agent_hex.to_string(), founding_hex.to_string());
    in_background("a conversation", agent_hex.clone(), async move {
        let anchor = agent(&founding_hex).ok_or("bad founding key")?;
        let (cipher, nonce) = sealed(&data_key, &crate::transcript_ids::conversation_plaintext_v2(plain, &id)).ok_or("not sealed")?;
        let conversation_anchor = ExternalHash::from_raw_32(crate::transcript_ids::conversation_anchor(&data_key, &id).to_vec());
        let _: holochain_types::prelude::ActionHash = app_cells().write(&agent_hex, "start_conversation", &StartConversation { cipher, nonce, anchor, conversation_anchor }).await?;
        Ok(())
    });
}

/// A message or annotation just written to the 0.6 line as `id`, written
/// here too.
pub fn also_record_message(agent_hex: &str, data_key: [u8; 32], conversation_id: String, id: String, plain: serde_json::Value) {
    let agent_hex = agent_hex.to_string();
    in_background("a message", agent_hex.clone(), async move {
        let (cipher, nonce) = sealed(&data_key, &crate::transcript_ids::message_plaintext_v2(plain, &id, &conversation_id)).ok_or("not sealed")?;
        let input = RecordMessage {
            conversation_anchor: ExternalHash::from_raw_32(crate::transcript_ids::conversation_anchor(&data_key, &conversation_id).to_vec()),
            message_key: crate::transcript_ids::message_key(&data_key, &id).to_vec(),
            cipher,
            nonce,
        };
        let _: holochain_types::prelude::ActionHash = app_cells().write(&agent_hex, "record_message", &input).await?;
        Ok(())
    });
}

/// A conversation just deleted on the 0.6 line, deleted here too: this
/// device's parts removed and the tombstone written.
pub fn also_delete_conversation(agent_hex: &str, founding_hex: &str, data_key: [u8; 32], conversation_id: String) {
    let (agent_hex, founding_hex) = (agent_hex.to_string(), founding_hex.to_string());
    in_background("a deletion", agent_hex.clone(), async move {
        let anchor = agent(&founding_hex).ok_or("bad founding key")?;
        let conversation_anchor = ExternalHash::from_raw_32(crate::transcript_ids::conversation_anchor(&data_key, &conversation_id).to_vec());
        let _: u32 = app_cells().write(&agent_hex, "delete_conversation", &DeleteConversation { anchor, conversation_anchor }).await?;
        Ok(())
    });
}

/// A memory row just saved on the 0.6 line, saved here too: the same sealed
/// bytes (a row's plaintext carries its own id).
pub fn also_save_memory_row(agent_hex: &str, cipher: Vec<u8>, nonce: Vec<u8>, anchor: AnyLinkableHash) {
    let agent_hex = agent_hex.to_string();
    in_background("a memory row", agent_hex.clone(), async move {
        let _: holochain_types::prelude::ActionHash = app_cells().write(&agent_hex, "save_memory_row", &SaveMemoryRow { cipher, nonce, anchor }).await?;
        Ok(())
    });
}

/// The resource directory holds the bundle this build ships.
pub fn bundle_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join(HAPP_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn the_bundled_v2_happ_is_the_canonical_one() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let shipped = std::fs::read(manifest.join("resources").join(HAPP_FILENAME)).expect("bundled");
        assert_eq!(hex::encode(Sha256::digest(&shipped)), HAPP_SHA256, "the bundle in resources/ is the one DNA_VERSION_HISTORY.md records");
        let built = std::fs::read(manifest.join("../dna/v2/workdir").join("yourown_ai_transcript_v2_happ.happ")).expect("dna/v2 bundle");
        assert_eq!(shipped, built, "copied from dna/v2/workdir, never rebuilt");
    }

    #[test]
    fn each_v1_app_has_one_v2_app_id() {
        assert_eq!(app_id_for("transcript_v1_local-17-abc").as_deref(), Some("transcript_v2_local-17-abc"));
        assert_eq!(app_id_for("flowsta_private"), None);
    }
}
