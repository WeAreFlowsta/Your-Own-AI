//! The open profile's Holochain key store and conductor, run through
//! flowsta-hc-lines. The library starts them (the key store made on the
//! first run, a free admin port, a config only this OS account can read,
//! the conductor, its admin interface), holds them for the session and
//! stops them: the conductor first, asked and given a grace, then the key
//! store.
//!
//! Every interface accepts one origin, made fresh for each start and kept by
//! the supervisor ([`origin`]). Every connection the app makes presents it;
//! a web page always presents its own address, and another account on the
//! device cannot read the config, so nothing else can open these interfaces.
//!
//! Agents are made while the app runs, one per AI generation
//! (`HolochainManager::provision_agent`), in the line's own key store.

use flowsta_hc_lines::conductor::NetworkTuning;
use flowsta_hc_lines::keystore::SocketPlan;
use flowsta_hc_lines::rendezvous::{answers, Rendezvous, PROBE_TIMEOUT};
use flowsta_hc_lines::supervisor::{self, LineHooks, LineProcess, LineStart, Role, Setup, StartStep, Supervisor, Timeouts, NOT_WANTED};
use flowsta_hc_lines::{AdminClient, Line};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use tauri::Emitter;

/// Each line's network generation, in the Vault's vocabulary (`devices.rs`
/// NETWORK_GENERATION), so a device reads the same way everywhere. A
/// profile's own follows its records to 0.7 (`line_facts::own_generation`).
pub fn network_generation(line: Line) -> &'static str {
    match line {
        Line::V06 => "holochain-0.6",
        Line::V07 => "holochain-0.7",
    }
}

/// The passphrase of the 0.6 line's key store and conductor databases.
/// Kept as it has always been until the 0.6 line retires, so nothing that
/// exists is rewritten.
pub(crate) const PASSPHRASE_06: &str = "your-own-ai-local";

static SUPERVISOR: LazyLock<Arc<Supervisor>> = LazyLock::new(|| Arc::new(Supervisor::new("yoai")));

/// The open profile's root, once its lines have been asked to start.
static PROFILE_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The open profile's root (None before the start).
pub fn profile_root() -> Option<&'static Path> {
    PROFILE_ROOT.get().map(|p| p.as_path())
}

/// The open profile's lines.
pub fn supervisor() -> &'static Arc<Supervisor> {
    &SUPERVISOR
}

/// The origin every connection to this start's conductor presents.
pub fn origin() -> String {
    supervisor().origin()
}

/// The 0.6 conductor's admin port, while it runs.
pub fn admin_port() -> Option<u16> {
    supervisor().ports(Line::V06).map(|(admin, _)| admin)
}

/// The 0.6 conductor's app interface port, while it runs.
pub fn app_port() -> Option<u16> {
    supervisor().ports(Line::V06).map(|(_, app)| app)
}

/// A running line's key store, for calls signed with an AI's own key: its
/// address and its passphrase. The address carries the connection key:
/// never log either.
pub(crate) fn key_store(line: Line) -> Option<(String, String)> {
    let url = supervisor().key_store_url(line)?;
    let passphrase = match line {
        Line::V06 => PASSPHRASE_06.to_string(),
        Line::V07 => LINE_07.lock().unwrap_or_else(|p| p.into_inner()).as_ref()?.passphrase.clone(),
    };
    Some((url, passphrase))
}

/// A line's admin and app ports, while it runs.
pub fn ports(line: Line) -> Option<(u16, u16)> {
    supervisor().ports(line)
}

/// Where a key store's socket goes when its own folder's path is over the
/// Unix-socket limit: the folder and naming the app has always used, so an
/// existing key store keeps its socket.
pub fn socket_plan() -> SocketPlan {
    SocketPlan::new("fv", "lair")
}

/// A line's admin port candidates, the first free one used: 4477-4479 for
/// 0.6, 4480-4482 for 0.7 (both shifted by the port offset).
fn admin_ports(line: Line) -> [u16; 3] {
    let base = crate::ports::admin_ws() + if line == Line::V07 { 3 } else { 0 };
    [base, base + 1, base + 2]
}

/// A line's conductor settings. SQLite synced in full on every write
/// (0.6 `Resilient`, 0.7 `Full`), errors logged plus the bootstrap client's
/// warnings (a refused registration, "Bootstrap PUT returned HTTP error", is
/// the one line that tells a stranded device from a quiet one).
///
/// On 0.7, NO gossip cap (the library's default is 1 MiB an exchange).
/// Kitsune2 sends the ops of one sector and time slice as one batch and
/// skips a batch over the cap, every round: one AI's activity during a busy
/// spell (the move writes thousands of actions in minutes) is such a batch,
/// and anything older than the new-ops window (about 30 minutes back for
/// a peer with no bookmark, which is any peer after a restart) travels only
/// in those batches. So under the cap history older than ~30 minutes did
/// not reach another conductor (0.9.1 betas, 2026-11-11: two computers
/// offering each other the same ids for hours). The cap was there for the 0.7.1 presence
/// check, which failed above ~10.9k offered ids; the conductor this app
/// ships checks in chunks (WeAreFlowsta/holochain-build patch 0006), so
/// Kitsune2's own 100 MB an exchange applies.
fn tuning(line: Line) -> NetworkTuning {
    let mut t = NetworkTuning::for_line(line);
    match line {
        Line::V06 => t.db_sync_strategy = Some("Resilient".into()),
        Line::V07 => {
            t.db_sync_level = Some("Full".into());
            t.gossip_max_op_bytes = None;
        }
    }
    t.rust_log = Some("error,kitsune2_bootstrap_client=warn".into());
    t
}

/// A line's key store and conductor folders in the profile.
pub fn line_dirs(data_dir: &Path, line: Line) -> (PathBuf, PathBuf) {
    match line {
        Line::V06 => (data_dir.join("lair"), data_dir.join("conductor")),
        Line::V07 => (data_dir.join("lair-07"), data_dir.join("conductor-07")),
    }
}

/// A line's sidecars: its key store, then its conductor.
fn sidecars(line: Line) -> (&'static str, &'static str) {
    match line {
        Line::V06 => ("yourowai-lair-keystore", "yourowai-holochain"),
        Line::V07 => ("yourowai-lair-keystore-07", "yourowai-holochain-07"),
    }
}

// ── The 0.7 key store's passphrase ──────────────────────────────────────

/// The file in the 0.7 key store's folder holding its passphrase salt: 32
/// random bytes, hex, made once for this install.
const SALT_FILE_07: &str = "passphrase-salt";

/// The 0.7 key store's passphrase: derived from the person's data key and
/// this install's salt, never stored. The data key is already the one
/// secret that must be kept (and the Vault backup carries it), so the
/// passphrase adds no way to lose data and no key material for a backup to
/// carry; the salt makes it this install's own.
pub(crate) fn passphrase_07(data_key: &[u8; 32], salt: &[u8; 32]) -> String {
    let mut material = [0u8; 64];
    material[..32].copy_from_slice(data_key);
    material[32..].copy_from_slice(salt);
    hex::encode(blake3::derive_key("Your Own AI 2026-11 lair 0.7 key store passphrase", &material))
}

/// The 0.7 key store's passphrase as it stands (its salt is in it), for a
/// start beside a running 0.6 line, when the store cannot be copied again.
pub(crate) fn passphrase_of_store_07(data_dir: &Path, data_key: &[u8; 32]) -> Result<String, String> {
    let (lair_07, _) = line_dirs(data_dir, Line::V07);
    let text = std::fs::read_to_string(lair_07.join(SALT_FILE_07)).map_err(|e| format!("read the 0.7 key store's salt: {e}"))?;
    let salt: [u8; 32] = hex::decode(text.trim()).ok().and_then(|b| b.try_into().ok()).ok_or("the 0.7 key store's salt is malformed")?;
    Ok(passphrase_07(data_key, &salt))
}

/// Start the 0.7 line again beside the running session (its conductor
/// died, or it was stopped for its memory).
pub fn attach_07(app: tauri::AppHandle, data_dir: PathBuf, line_07: Line07) -> Result<(), String> {
    let session = supervisor().session().ok_or("no session runs")?;
    *LINE_07.lock().unwrap_or_else(|p| p.into_inner()) = Some(line_07.clone());
    supervisor().attach_line(session, Line::V07, Arc::new(YoaiLines { app: Some(app), data_dir, line_07: Some(line_07) }));
    Ok(())
}

/// Make the 0.7 key store ready to start, and return its passphrase.
///
/// While the 0.6 line is in use, the 0.7 store is a fresh copy of the 0.6
/// one at every start: the same keys, so every AI keeps its id, including
/// an AI made since the last start (its key is made in the 0.6 store, and
/// cannot be exported). The copy is built beside the store, given the
/// store's salt (so the passphrase stays the same), rewrapped from the 0.6
/// passphrase to the derived one, and swapped in; a start stopped part way
/// finds one whole store, and an interrupted swap is put back. Neither
/// line may be running: the copy reads a quiet 0.6 store, and the rewrap
/// writes a store no lair has open.
/// A key went into the 0.6 store alone after the move (the 0.7 line was not
/// up yet): the next start copies the store again, which loses nothing
/// while the 0.7 store holds no key of its own.
pub(crate) const COPY_NEEDED_FILE: &str = "lair-07-copy-needed";
/// The 0.7 store holds a key the 0.6 store does not (an AI made on the 0.7
/// line alone): it is never replaced by a copy again.
pub(crate) const KEYS_07_ONLY_FILE: &str = "lair-07-own-keys";

/// Note a key made in one store alone ([`COPY_NEEDED_FILE`] or
/// [`KEYS_07_ONLY_FILE`]).
pub(crate) fn note_key_store(file: &str) {
    if let Some(root) = profile_root() {
        if let Err(e) = flowsta_hc_lines::files::write_atomic_private(&root.join(file), b"") {
            log::warn!("[keys] could not note {file}: {e}");
        }
    }
}

pub(crate) async fn prepare_store_07(data_dir: &Path, data_key: &[u8; 32]) -> Result<String, String> {
    let (lair_06, _) = line_dirs(data_dir, Line::V06);
    let (lair_07, _) = line_dirs(data_dir, Line::V07);
    let next = data_dir.join("lair-07.next");
    let prev = data_dir.join("lair-07.prev");
    if !lair_07.exists() && prev.exists() {
        std::fs::rename(&prev, &lair_07).map_err(|e| format!("put back the 0.7 key store: {e}"))?;
        log::warn!("[0.7] a key store swap was stopped part way: the previous store is back");
    }
    for leftover in [&next, &prev] {
        if leftover.exists() {
            std::fs::remove_dir_all(leftover).map_err(|e| format!("clear {leftover:?}: {e}"))?;
        }
    }
    // Once the profile has moved, the 0.7 store is the source: every key
    // made since went into both stores (or into this one alone, after the
    // 0.6 line stopped), and a copy from the 0.6 store would delete the
    // keys only this one holds.
    let moved = matches!(
        flowsta_hc_lines::marker::Marker::load(data_dir, &crate::move_07::MARKER),
        flowsta_hc_lines::marker::MarkerRead::Present(_)
    );
    let copy_needed = data_dir.join(COPY_NEEDED_FILE).exists() && !data_dir.join(KEYS_07_ONLY_FILE).exists();
    if moved && !copy_needed && lair_07.join(SALT_FILE_07).exists() && flowsta_hc_lines::keystore::config_path(&lair_07).exists() {
        log::info!("[0.7] the profile has moved: its 0.7 key store stands as it is");
        return passphrase_of_store_07(data_dir, data_key);
    }
    if moved && copy_needed {
        log::info!("[0.7] a key went into the 0.6 store alone: the 0.7 store is copied again (it holds no key of its own)");
    }
    let salt_path = lair_07.join(SALT_FILE_07);
    let salt: [u8; 32] = match std::fs::read_to_string(&salt_path) {
        Ok(text) => hex::decode(text.trim()).ok().and_then(|b| b.try_into().ok()).ok_or("the 0.7 key store's salt is malformed")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut salt = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
            salt
        }
        Err(e) => return Err(format!("read the 0.7 key store's salt: {e}")),
    };
    let passphrase = passphrase_07(data_key, &salt);
    let copied = flowsta_hc_lines::keystore::copy_store(&lair_06, &next, &socket_plan()).map_err(|e| format!("copy the key store for 0.7: {e}"))?;
    flowsta_hc_lines::files::write_atomic_private(&next.join(SALT_FILE_07), hex::encode(salt).as_bytes()).map_err(|e| format!("write the 0.7 key store's salt: {e}"))?;
    flowsta_hc_lines::keystore::rekey_store(&next, Line::V07, PASSPHRASE_06, &passphrase).await.map_err(|e| format!("rewrap the 0.7 key store: {e}"))?;
    if lair_07.exists() {
        std::fs::rename(&lair_07, &prev).map_err(|e| format!("set the previous 0.7 key store aside: {e}"))?;
    }
    std::fs::rename(&next, &lair_07).map_err(|e| format!("put the 0.7 key store in place: {e}"))?;
    if prev.exists() {
        let _ = std::fs::remove_dir_all(&prev);
    }
    log::info!("[0.7] key store refreshed from the 0.6 one ({} bytes) under this install's passphrase", copied.bytes);
    let _ = std::fs::remove_file(data_dir.join(COPY_NEEDED_FILE));
    Ok(passphrase)
}

/// What a 0.6-led start does once the 0.6 line is up, for a 0.6 line that
/// joined as secondary: the coordinator sweep and the agents' connections.
async fn connect_06_after_join(app: tauri::AppHandle, data_dir: PathBuf) {
    use tauri::Manager as _;
    let Some(hc) = app.try_state::<Arc<crate::commands_holochain::HolochainState>>() else { return };
    let hc = hc.inner().clone();
    let mut waited = std::time::Duration::ZERO;
    let manager = loop {
        if let Ok(m) = hc.get() {
            break m;
        }
        if waited > std::time::Duration::from_secs(600) {
            log::warn!("[0.6] joined, but the app's records never became ready: its agents wait for the next start");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        waited += std::time::Duration::from_secs(2);
    };
    if let Err(e) = crate::dna::update_coordinators_sweep(manager.admin_port(), &manager.resource_dir, &data_dir).await {
        log::warn!("[0.6] coordinator sweep after joining: {e}");
    }
    if let Err(e) = manager.reconnect_existing_agents().await {
        log::warn!("[0.6] agents after joining: {e}");
    }
}

// ── Closing ─────────────────────────────────────────────────────────────

static CLOSING: AtomicBool = AtomicBool::new(false);

/// The app is quitting, resetting or swapping its material: no start that
/// is under way goes on (each ends at its next step), and none begins.
pub fn begin_closing() {
    CLOSING.store(true, Ordering::SeqCst);
}

fn closing() -> bool {
    CLOSING.load(Ordering::SeqCst)
}

/// Whether the app is quitting, resetting or swapping its material.
pub fn is_closing() -> bool {
    closing()
}

/// Stop the lines for good: no start goes on, a start under way is waited
/// for (up to 30 s), then every line stops, the conductor first. Blocking.
pub fn stop_for_good() {
    begin_closing();
    if !supervisor().wait_for_starts(std::time::Duration::from_secs(30)) {
        log::warn!("[conductor] a start was still under way after 30 s - stopping anyway");
    }
    if supervisor().stop() {
        log::info!("[conductor] the key store and conductor are stopped");
    }
}

/// [`stop_for_good`] from async code.
pub async fn stop_for_good_async() {
    let _ = tokio::task::spawn_blocking(stop_for_good).await;
}

// ── Status for the window ───────────────────────────────────────────────

/// Conductor status reported to the frontend.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum ConductorStatus {
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "starting")]
    Starting { message: String },
    #[serde(rename = "ready")]
    Ready { admin_port: u16, app_port: u16 },
    #[serde(rename = "error")]
    Error { message: String },
}

fn step_message(step: StartStep) -> &'static str {
    match step {
        StartStep::KeyStore => "Starting lair-keystore...",
        StartStep::Connect => "Connecting to lair-keystore...",
        StartStep::Agents => "Reading the key store...",
        StartStep::Conductor => "Starting Holochain conductor...",
        StartStep::WaitAdmin => "Waiting for conductor...",
        _ => "Starting...",
    }
}

// ── The rendezvous ──────────────────────────────────────────────────────
//
// Where this conductor meets the person's OTHER devices. The network is
// private by construction - the seed is per identity (or per install) and
// every record is ciphertext under the person's data key before it reaches
// the DNA - so a reachable peer is only ever one of the person's own
// devices, and the rendezvous learns agent keys, a space hash and an
// address, never content or an identity.
//
// Bootstrap + signal URL and the per-app auth material are baked at build
// time (`FLOWSTA_BOOTSTRAP_URL`, `FLOWSTA_SIGNAL_URL`, `FLOWSTA_AUTH_MATERIAL`
// - the Vault's names) or set at run time for a dev box. Without auth
// material the conductor keeps the localhost black hole it always had: one
// device, no peers, nothing to reach - the bootstrap refuses unauthenticated
// clients anyway.

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Flowsta's own rendezvous, usable only with this app's auth material.
fn primary() -> Option<Rendezvous> {
    let auth_material = env("FLOWSTA_AUTH_MATERIAL").or_else(|| option_env!("FLOWSTA_AUTH_MATERIAL").filter(|m| !m.is_empty()).map(String::from))?;
    let bootstrap_url = env("FLOWSTA_BOOTSTRAP_URL")
        .or_else(|| option_env!("FLOWSTA_BOOTSTRAP_URL").map(String::from))
        .unwrap_or_else(|| "https://bootstrap.flowsta.com".to_string());
    let signal_url = env("FLOWSTA_SIGNAL_URL")
        .or_else(|| option_env!("FLOWSTA_SIGNAL_URL").map(String::from))
        .unwrap_or_else(|| bootstrap_url.replacen("https://", "wss://", 1));
    Some(Rendezvous { bootstrap_url, signal_url: Some(signal_url), auth_material: Some(auth_material), relay_url: None })
}

/// Open rendezvous for when Flowsta's is dark (community nodes): a
/// comma-separated list, https only (the conductor refuses a plaintext
/// relay), no auth material by design - the Vault's rule.
fn fallbacks() -> Vec<Rendezvous> {
    let list = env("FLOWSTA_BOOTSTRAP_FALLBACKS").or_else(|| option_env!("FLOWSTA_BOOTSTRAP_FALLBACKS").map(String::from)).unwrap_or_default();
    parse_fallbacks(&list)
}

fn parse_fallbacks(list: &str) -> Vec<Rendezvous> {
    list.split(',')
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .filter(|u| {
            let ok = u.starts_with("https://");
            if !ok {
                log::error!("[conductor] ignoring non-https fallback {} - the conductor forbids plaintext relays", u);
            }
            ok
        })
        .map(|u| {
            let bootstrap_url = u.trim_end_matches('/').to_string();
            let host = bootstrap_url.trim_start_matches("https://").to_string();
            Rendezvous { bootstrap_url, signal_url: Some(format!("wss://{}", host)), auth_material: None, relay_url: None }
        })
        .collect()
}

/// The 0.7 line's rendezvous: its own bootstrap host beside the 0.6 one (a
/// 0.7.1 conductor speaks to a kitsune2 0.5 bootstrap), with the same auth
/// material. The open fallbacks serve the 0.6 line only.
fn primary_07() -> Option<Rendezvous> {
    let auth_material = env("FLOWSTA_AUTH_MATERIAL").or_else(|| option_env!("FLOWSTA_AUTH_MATERIAL").filter(|m| !m.is_empty()).map(String::from))?;
    let bootstrap_url = env("FLOWSTA_BOOTSTRAP_URL_07")
        .or_else(|| option_env!("FLOWSTA_BOOTSTRAP_URL_07").map(String::from))
        .unwrap_or_else(|| "https://bootstrap-7.flowsta.com".to_string());
    Some(Rendezvous { bootstrap_url, signal_url: None, auth_material: Some(auth_material), relay_url: None })
}

/// The 0.7 line's rendezvous for the registration keeper.
pub(crate) fn rendezvous_servers_07() -> Vec<(String, Option<String>)> {
    primary_07().map(|p| vec![(p.bootstrap_url, p.auth_material)]).unwrap_or_default()
}

/// No rendezvous at all: a localhost black hole, one device.
fn alone() -> Rendezvous {
    Rendezvous { bootstrap_url: "https://localhost/".into(), signal_url: Some("wss://localhost/".into()), auth_material: None, relay_url: None }
}

/// The rendezvous for this conductor session: the primary when it answers
/// (or when there is nothing else to try); else the first live fallback;
/// with everything dark, the primary anyway - the conductor works alone and
/// reconnects when anything returns. None: no material and no fallbacks.
async fn resolve() -> Option<Rendezvous> {
    let primary = primary();
    let fallbacks = fallbacks();
    if fallbacks.is_empty() {
        return primary;
    }
    if let Some(p) = &primary {
        if answers(&p.bootstrap_url, PROBE_TIMEOUT).await {
            return primary;
        }
        log::warn!("[conductor] rendezvous {} unreachable - trying {} fallback(s)", p.bootstrap_url, fallbacks.len());
    }
    for f in &fallbacks {
        if answers(&f.bootstrap_url, PROBE_TIMEOUT).await {
            log::info!("[conductor] using open rendezvous {}", f.bootstrap_url);
            return Some(f.clone());
        }
    }
    log::warn!("[conductor] every rendezvous is dark - starting alone, reconnecting when one returns");
    primary.or_else(|| fallbacks.into_iter().next())
}

/// The rendezvous the running conductor was configured with at its start
/// (the main one, or the first fallback that answered when it was dark).
/// The registration keeper calls a stall only against this one: the
/// conductor never registers anywhere else, so a missing note on a fallback
/// is not a stall (10-19: "did not reach node1.flowsta.com" every few
/// minutes).
pub(crate) fn conductor_rendezvous() -> Option<String> {
    supervisor().rendezvous(Line::V06)
}

/// Every rendezvous this build knows, for the registration keeper
/// (the keeper, `Supervisor::start_keeper`): Flowsta's with this app's auth material first,
/// then the open community nodes. `(bootstrap url, base64 auth material)`.
pub(crate) fn rendezvous_servers() -> Vec<(String, Option<String>)> {
    let mut servers = Vec::new();
    if let Some(p) = primary() {
        servers.push((p.bootstrap_url, p.auth_material));
    }
    for f in fallbacks() {
        servers.push((f.bootstrap_url, None));
    }
    servers
}

/// Whether this session's conductor started with no rendezvous answering,
/// and so without its auth. Holochain 0.6.1 authenticates with the relay as
/// it starts, and a conductor that cannot (no network) refuses to start at
/// all ("Authenticate Failed", exit 101). Started without the auth it runs,
/// and everything on this device works; the next start uses the auth again.
static STARTED_OFFLINE: AtomicBool = AtomicBool::new(false);

pub(crate) fn started_offline() -> bool {
    STARTED_OFFLINE.load(Ordering::SeqCst)
}

/// Whether the leading line's rendezvous (the one with auth material)
/// answers.
pub(crate) async fn rendezvous_answers() -> bool {
    let r = if leading() == Line::V07 { primary_07() } else { primary() };
    match r {
        Some(p) => answers(&p.bootstrap_url, PROBE_TIMEOUT).await,
        None => false,
    }
}

// ── The start ───────────────────────────────────────────────────────────

/// Start one line's key store and conductor with the app's settings.
async fn start_one(
    line: Line,
    data_dir: &Path,
    passphrase: &str,
    rendezvous: &Rendezvous,
    origin: &str,
    agents: &[flowsta_hc_lines::supervisor::KeySource],
    on_step: impl FnMut(StartStep) + Send,
) -> Result<LineProcess, String> {
    let (lair_name, conductor_name) = sidecars(line);
    let (lair_bin, conductor_bin) = (crate::resolve_sidecar_bin(lair_name), crate::resolve_sidecar_bin(conductor_name));
    log::info!("[{line}] key store {:?}, conductor {:?}", lair_bin, conductor_bin);
    let (lair_dir, conductor_dir) = line_dirs(data_dir, line);
    supervisor::start_line(
        &LineStart {
            line,
            lair_bin: &lair_bin,
            conductor_bin: &conductor_bin,
            lair_dir: &lair_dir,
            conductor_dir: &conductor_dir,
            passphrase,
            sockets: &socket_plan(),
            admin_ports: &admin_ports(line),
            origin,
            rendezvous,
            tuning: &tuning(line),
            agents,
            timeouts: Timeouts::default(),
        },
        on_step,
    )
    .await
    .map_err(|e| e.to_string())
}

/// A line's rendezvous as this start uses it: without its auth while it
/// does not answer, the black hole when there is none.
async fn entry_for(line: Line, role: Role) -> Rendezvous {
    let found = match line {
        Line::V06 => resolve().await,
        Line::V07 => primary_07(),
    };
    match found {
        Some(r) => {
            let start = r.for_start(PROBE_TIMEOUT).await;
            if start.without_auth {
                log::warn!("[{line}] no rendezvous answers ({}): starting without its auth, offline - the next start uses it", start.rendezvous.bootstrap_url);
            }
            if role == Role::Primary {
                STARTED_OFFLINE.store(start.without_auth, Ordering::SeqCst);
            }
            log::info!("[{line}] rendezvous {} (the person's own devices can meet)", start.rendezvous.bootstrap_url);
            supervisor().set_rendezvous(line, Some(&start.rendezvous.bootstrap_url));
            start.rendezvous
        }
        None => {
            log::info!("[{line}] no rendezvous - this conductor stays alone");
            supervisor().set_rendezvous(line, None);
            alone()
        }
    }
}

/// What the app's lines need from the app (flowsta-hc-lines `LineHooks`):
/// the profile's folders, the 0.7 line's passphrase, and the window to
/// narrate the primary's start to.
struct YoaiLines {
    app: Option<tauri::AppHandle>,
    /// The open profile's root.
    data_dir: PathBuf,
    /// What the 0.7 line needs; None when this start runs no 0.7 line.
    line_07: Option<Line07>,
}

/// What a start's 0.7 line needs.
#[derive(Clone)]
pub struct Line07 {
    /// The key store's passphrase ([`prepare_store_07`]).
    pub passphrase: String,
    /// The person's network seed, which every transcript cell joins.
    pub network_seed: String,
    /// Where the bundled v2 hApp is.
    pub resource_dir: PathBuf,
}

impl YoaiLines {
    fn emit(&self, role: Role, status: ConductorStatus) {
        if role == Role::Primary {
            if let Some(app) = &self.app {
                let _ = app.emit("conductor-status", status);
            }
        }
    }

    fn say(&self, role: Role, message: &str) {
        self.emit(role, ConductorStatus::Starting { message: message.into() });
    }
}

#[async_trait::async_trait]
impl LineHooks for YoaiLines {
    async fn start(&self, line: Line, role: Role) -> Result<LineProcess, String> {
        let passphrase = match line {
            Line::V06 => PASSPHRASE_06,
            Line::V07 => self.line_07.as_ref().map(|l| l.passphrase.as_str()).ok_or("the 0.7 key store is not prepared")?,
        };
        // Beside a running 0.6 line, the 0.7 key store is a stopped copy of
        // the 0.6 one: it keeps only the keys in use when its listing would
        // not fit one lair message (Holochain 0.7.1 lists every key at each
        // install). Even those not fitting: the 0.7 line waits, and says why.
        if line == Line::V07 && role == Role::Secondary && ports(Line::V06).is_some() {
            if let Err(why) = trim_store_07(&self.data_dir, passphrase).await {
                crate::move_07::note_paused(why.clone());
                return Err(why);
            }
        } else if line == Line::V07 {
            // With no 0.6 line to ask, the keys in use are the ones noted
            // while the 0.7 line last ran. The line starts either way.
            prune_store_07(&self.data_dir, passphrase).await;
        }
        let entry = entry_for(line, role).await;
        // One origin per app start, shared by both lines: the primary makes
        // it, a line attached beside it keeps the one in force (a fresh one
        // would leave the running line refusing every connection).
        let origin = if role == Role::Primary || supervisor().origin().is_empty() { supervisor().fresh_origin() } else { supervisor().origin() };
        start_one(line, &self.data_dir, passphrase, &entry, &origin, &[], |step| self.say(role, step_message(step))).await
    }

    /// The app interface, accepting only this start's origin. On 0.6 the
    /// coordinator sweep and the agents follow once the line has joined. On
    /// 0.7, every AI cell on the running 0.6 line gets its v2 app (adopted
    /// when it exists, never reinstalled), and a start that installed any
    /// restarts the line once to give back the memory the installs kept.
    async fn setup(&self, line: Line, role: Role, process: &mut LineProcess) -> Result<Setup, String> {
        self.say(role, "Setting up app interface...");
        let origin = supervisor().origin();
        let admin = AdminClient::connect(line, process.admin_port, &origin).await.map_err(|e| format!("Failed to connect to admin WebSocket: {e}"))?;
        let app_port = admin.attach_app_interface(&origin).await.map_err(|e| format!("Failed to attach app interface: {e}"))?;
        log::info!("[{line}] app interface attached on port {}", app_port);
        if line == Line::V07 {
            let l07 = self.line_07.as_ref().ok_or("the 0.7 line has no settings")?;
            crate::hc07::purge_noted_on(&admin, &self.data_dir).await;
            let v1_apps = transcript_apps_06().await?;
            let installed = crate::hc07::ensure_apps_on(&admin, &l07.resource_dir, &l07.network_seed, &v1_apps).await?;
            if installed > 0 {
                log::info!("[0.7] {installed} transcript app(s) installed: the line restarts once to give back the memory the installs kept");
                return Ok(Setup { app_port, restart: true });
            }
        }
        Ok(app_port.into())
    }

    async fn joined(&self, line: Line, role: Role, _session: u64, admin_port: u16, app_port: u16) {
        self.emit(role, ConductorStatus::Ready { admin_port, app_port });
        log::info!("[{line}] Holochain conductor ready (admin: {}, app: {})", admin_port, app_port);
        // Once a run, after a line is up: both lines' kits set aside for a
        // later release that no longer bundles them (kits.rs).
        if let Some(app) = &self.app {
            match crate::profile::device_root(app) {
                Ok(root) => crate::kits::refresh_once(root.clone(), crate::resolve_resource_dir(app, &root)),
                Err(e) => log::warn!("[kit] no kits cached: {e}"),
            }
        }
        // The 0.6 line joined after the 0.7 one (0.7 leads, or 0.6 started
        // again for a device still on it): its coordinators and its agents'
        // connections, which a 0.6-led start does after the start, are done
        // now; its AI cells get their v2 apps as the 0.7 setup would have
        // given them; and the passes run again (they stop whenever either
        // line is down).
        if line == Line::V06 {
            OLD_LINE_FAILED.store(false, Ordering::SeqCst);
        }
        if line == Line::V07 {
            match AdminClient::connect(Line::V07, admin_port, &supervisor().origin()).await {
                Ok(admin) => note_apps_07(&admin, &self.data_dir).await,
                Err(e) => log::warn!("[0.7] key store: the keys in use were not noted ({e})"),
            }
        }
        if line == Line::V06 && role == Role::Secondary {
            if let Some(app) = &self.app {
                let (app, data_dir) = (app.clone(), self.data_dir.clone());
                tauri::async_runtime::spawn(async move { connect_06_after_join(app, data_dir).await });
            }
        }
        if line == Line::V06 && ports(Line::V07).is_some() {
            if let Some(app) = &self.app {
                crate::move_07::run_passes(app.clone(), self.data_dir.clone());
            }
            if let Some(l07) = &self.line_07 {
                let (resource_dir, seed) = (l07.resource_dir.clone(), l07.network_seed.clone());
                tauri::async_runtime::spawn(async move {
                    match transcript_apps_06().await {
                        Ok(apps) => match crate::hc07::ensure_apps(&resource_dir, &seed, &apps).await {
                            Ok(0) => {}
                            Ok(n) => log::info!("[0.7] {n} transcript app(s) installed after the 0.6 line joined"),
                            Err(e) => log::warn!("[0.7] v2 apps after the 0.6 line joined: {e}"),
                        },
                        Err(e) => log::warn!("[0.7] the 0.6 line's apps: {e}"),
                    }
                });
            }
        }
        if line == Line::V07 {
            if let Some(app) = &self.app {
                crate::move_07::run_passes(app.clone(), self.data_dir.clone());
            }
        }
    }

    async fn secondary_failed(&self, line: Line, error: &str) {
        if line == Line::V06 {
            // Not running this session after all: chat must still work, so
            // writes may go to the 0.7 line alone until it joins (this
            // session only; the marker still says it is wanted).
            OLD_LINE_FAILED.store(true, Ordering::SeqCst);
        }
        log::warn!("[{line}] did not start ({error}); the app goes on with its other line");
    }

    fn wanted(&self) -> bool {
        !closing()
    }
}

/// The agent keys an AI list names (its 39-byte keys, hex): every AI's
/// current, founding and earlier keys, whatever field holds them. A key an
/// AI may ask for again (a restore, a sync) is a key in use.
pub(crate) fn ai_list_keys(data_dir: &Path) -> Vec<[u8; 32]> {
    let Ok(text) = std::fs::read_to_string(data_dir.join("ai-data.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    let mut out = Vec::new();
    fn walk(v: &serde_json::Value, out: &mut Vec<[u8; 32]>) {
        match v {
            serde_json::Value::String(s) => {
                if let Some(k) = core_key_hex(s) {
                    out.push(k);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(&v["custom-ais"], &mut out);
    out.sort();
    out.dedup();
    out
}

/// An agent key's 32 core bytes, from its 39 bytes in hex.
fn core_key_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 78 || !s.starts_with("842024") {
        return None;
    }
    core_key(&hex::decode(s).ok()?)
}

/// An agent key's 32 core bytes (lair's Ed25519 key), from its 39 bytes.
fn core_key(raw: &[u8]) -> Option<[u8; 32]> {
    (raw.len() == 39).then(|| raw[3..35].try_into().ok()).flatten()
}

/// Holochain 0.7.1 checks an existing agent key at app install by listing
/// every key its lair holds, and the listing must fit one lair message
/// (about forty keys; `flowsta_hc_lines::keystore::entries`). A 0.7 key
/// store copied from a 0.6 one that grew past that (every AI ever made has
/// a key there, tidied ones included) keeps only the keys in use: every
/// enabled AI cell on the running 0.6 line and every key the AI list names.
/// A store whose listing fits is left as it is. Err: even the keys in use
/// do not fit, and the 0.7 line waits this session.
async fn trim_store_07(data_dir: &Path, passphrase: &str) -> Result<(), String> {
    let (lair_07, _) = line_dirs(data_dir, Line::V07);
    if !flowsta_hc_lines::keystore::config_path(&lair_07).exists() {
        return Ok(());
    }
    let listing = flowsta_hc_lines::keystore::entries(&lair_07, Line::V07, passphrase).await.map_err(|e| format!("read the 0.7 key store: {e}"))?;
    if listing.fits() {
        return Ok(());
    }
    let mut in_use: std::collections::HashSet<[u8; 32]> = transcript_apps_06().await?.iter().filter_map(|(_, k)| core_key(k)).collect();
    in_use.extend(ai_list_keys(data_dir));
    in_use.extend(noted_keys_07(data_dir).unwrap_or_default());
    trim_to(&lair_07, passphrase, &in_use).await
}

/// The agent keys the 0.7 line's transcript apps run under (32 core bytes,
/// hex, one a line), noted while the line runs: what [`prune_store_07`]
/// keeps when no 0.6 line can be asked.
pub(crate) const KEYS_07_IN_USE_FILE: &str = "lair-07-keys-in-use";

/// The room a 0.7 key store keeps for new AIs: [`prune_store_07`] acts once
/// a listing of the store comes within this of one lair message (three
/// keys, at about 200 bytes each).
const PRUNE_ROOM: usize = 600;

fn write_noted_keys_07(data_dir: &Path, keys: &std::collections::BTreeSet<[u8; 32]>) {
    let text: String = keys.iter().map(|k| hex::encode(k) + "\n").collect();
    if let Err(e) = flowsta_hc_lines::files::write_atomic_private(&data_dir.join(KEYS_07_IN_USE_FILE), text.as_bytes()) {
        log::warn!("[0.7] key store: the keys in use were not noted ({e})");
    }
}

/// The keys noted as in use on the 0.7 line; None before any 0.7 session
/// has noted them.
pub(crate) fn noted_keys_07(data_dir: &Path) -> Option<std::collections::BTreeSet<[u8; 32]>> {
    let text = std::fs::read_to_string(data_dir.join(KEYS_07_IN_USE_FILE)).ok()?;
    Some(text.lines().filter_map(|l| hex::decode(l.trim()).ok()?.try_into().ok()).collect())
}

/// Note every transcript app the 0.7 line holds (disabled ones too) as the
/// keys in use there, replacing the last note. An app that was removed
/// gives up its key unless the AI list still names it.
pub(crate) async fn note_apps_07(admin: &AdminClient, data_dir: &Path) {
    match admin.list_apps().await {
        Ok(apps) => {
            let keys = apps.iter().filter(|a| a.app_id.starts_with(crate::hc07::APP_ID_PREFIX)).filter_map(|a| core_key(&a.agent_key)).collect();
            write_noted_keys_07(data_dir, &keys);
        }
        Err(e) => log::warn!("[0.7] key store: the keys in use were not noted ({e})"),
    }
}

/// Note one more key as in use on the 0.7 line: a key just made there,
/// before its app is installed.
pub(crate) fn note_key_07(key: [u8; 32]) {
    let Some(root) = profile_root() else { return };
    let mut keys = noted_keys_07(root).unwrap_or_default();
    if keys.insert(key) {
        write_noted_keys_07(root, &keys);
    }
}

/// A 0.7 key store with no 0.6 line beside it (the profile has moved, or
/// the 0.6 line did not start) grows by a key for every AI made, and keeps
/// the keys of AIs since removed. Before a listing of it outgrows one lair
/// message, when Holochain 0.7.1 could no longer install an app for a new
/// AI, it keeps only the keys in use: the ones noted while the line ran
/// ([`KEYS_07_IN_USE_FILE`]) and every key the AI list names. With no note
/// yet the store is left as it is. Nothing here stops the line starting.
async fn prune_store_07(data_dir: &Path, passphrase: &str) {
    let (lair_07, _) = line_dirs(data_dir, Line::V07);
    if !flowsta_hc_lines::keystore::config_path(&lair_07).exists() {
        return;
    }
    let Some(noted) = noted_keys_07(data_dir) else { return };
    let listing = match flowsta_hc_lines::keystore::entries(&lair_07, Line::V07, passphrase).await {
        Ok(l) => l,
        Err(e) => return log::warn!("[0.7] key store: not read before the start ({e})"),
    };
    if listing.reply_bytes + PRUNE_ROOM <= flowsta_hc_lines::keystore::LAIR_REPLY_MAX {
        return;
    }
    let mut in_use: std::collections::HashSet<[u8; 32]> = noted.into_iter().collect();
    in_use.extend(ai_list_keys(data_dir));
    if let Err(e) = trim_to(&lair_07, passphrase, &in_use).await {
        log::warn!("[0.7] key store: {e}; new AIs wait until some are removed");
    }
}

/// Keep, in the stopped 0.7 key store at `lair_07`, the keys in `in_use`
/// and Holochain's own entries; Err when even those do not fit one lair
/// message.
async fn trim_to(lair_07: &Path, passphrase: &str, in_use: &std::collections::HashSet<[u8; 32]>) -> Result<(), String> {
    use flowsta_hc_lines::keystore::{keep_only, EntryKind, Kept, LAIR_REPLY_MAX};
    // Holochain's own entries stay too: its connection check's stub and its
    // conductor's `<state tag>_ed` seed (each made again when missing).
    let keep = |e: &flowsta_hc_lines::keystore::Entry| {
        e.kind != EntryKind::Seed || e.tag.starts_with('_') || e.tag == "HC_CON_CHK_STUB" || e.tag.ends_with("_ed") || e.ed25519.is_some_and(|k| in_use.contains(&k))
    };
    let (kept, reply_bytes) = match keep_only(lair_07, Line::V07, passphrase, &keep).await.map_err(|e| format!("trim the 0.7 key store: {e}"))? {
        Kept::Rebuilt { kept, dropped, reply_bytes } => {
            log::info!("[0.7] key store: {kept} key(s) in use kept, {dropped} left out (a listing is {reply_bytes} bytes)");
            (kept, reply_bytes)
        }
        Kept::AllKept { entries, reply_bytes } => (entries, reply_bytes),
    };
    if reply_bytes > LAIR_REPLY_MAX {
        let ais = in_use.len().min(kept);
        log::warn!("[0.7] key store: {kept} entries kept ({ais} AI keys) list in {reply_bytes} bytes, more than one lair message ({LAIR_REPLY_MAX})");
        return Err(format!("this computer has more AIs than the new version can take yet ({ais} in use)"));
    }
    Ok(())
}

/// Every enabled transcript app on the running 0.6 line: (app id, agent key
/// 39 bytes). A disabled one was tidied because it holds no records (the
/// tidy disables only cells it verified empty); it gets its v2 app at the
/// first 0.7 start after it is enabled again. With no 0.6 line there is
/// nothing to give a v2 app: the 0.7 line starts with the apps it has.
pub(crate) async fn transcript_apps_06() -> Result<Vec<(String, Vec<u8>)>, String> {
    let Some(port) = admin_port() else {
        log::info!("[0.7] no 0.6 line this start: the 0.7 line starts with the apps it has");
        return Ok(Vec::new());
    };
    let admin = AdminClient::connect(Line::V06, port, &origin()).await.map_err(|e| format!("0.6 admin: {e}"))?;
    let apps = admin.list_apps().await.map_err(|e| format!("0.6 list_apps: {e}"))?;
    Ok(apps.into_iter().filter(|a| a.enabled && a.app_id.starts_with(crate::dna::APP_ID_PREFIX)).map(|a| (a.app_id, a.agent_key)).collect())
}

/// Start the open profile's lines; returns once the 0.6 line is up and its
/// app interface is attached. With [`Line07`], the 0.7 line follows beside
/// it from a task. The primary gets one retry after a failure (2 s later,
/// so ports and processes are released).
pub async fn start_holochain(app: tauri::AppHandle, data_dir: PathBuf, line_07: Option<Line07>) -> Result<u64, String> {
    let _ = PROFILE_ROOT.set(data_dir.clone());
    crate::line_facts::reopen_if_written_since_stop(&data_dir);
    let (primary, secondary) = crate::line_facts::start_plan(&app, &data_dir, line_07.is_some());
    log::info!("[lines] {primary} leads{}", secondary.map(|l| format!(", {l} joins")).unwrap_or_default());
    set_leading(primary);
    if let Some(l07) = &line_07 {
        *LINE_07.lock().unwrap_or_else(|p| p.into_inner()) = Some(l07.clone());
    }
    // Writes go to 0.6 first exactly while it runs: the move's marker says
    // so (`old_line_needed`), and the write guard reads it.
    let with_06 = primary == Line::V06 || secondary == Some(Line::V06);
    note_old_line_needed(&data_dir, with_06);
    let hooks = Arc::new(YoaiLines { app: Some(app), data_dir, line_07 });
    match supervisor().start_lines(primary, secondary, hooks).await {
        Err(e) if e == NOT_WANTED => Err("the app is closing".into()),
        other => other,
    }
}

/// The line that leads this session (0.6 until a profile has moved).
static LEADING_07: AtomicBool = AtomicBool::new(false);

fn set_leading(line: Line) {
    LEADING_07.store(line == Line::V07, Ordering::SeqCst);
}

pub fn leading() -> Line {
    if LEADING_07.load(Ordering::SeqCst) {
        Line::V07
    } else {
        Line::V06
    }
}

/// The 0.6 line was wanted beside 0.7 this session but could not start.
static OLD_LINE_FAILED: AtomicBool = AtomicBool::new(false);

pub(crate) fn old_line_failed() -> bool {
    OLD_LINE_FAILED.load(Ordering::SeqCst)
}

/// The 0.6 line is expected but not up yet (it joins beside 0.7 a little
/// after the start, or is starting again): a read it cannot answer is
/// unanswered, not empty.
pub(crate) fn old_line_coming() -> bool {
    use flowsta_hc_lines::marker::{Marker, MarkerRead};
    if ports(Line::V06).is_some() || old_line_failed() {
        return false;
    }
    match profile_root().map(|r| Marker::load(r, &crate::move_07::MARKER)) {
        Some(MarkerRead::Present(m)) => m.old_line_needed != Some(false),
        _ => false,
    }
}

/// This session's 0.7 settings, for a 0.6 line attached beside it later.
static LINE_07: std::sync::Mutex<Option<Line07>> = std::sync::Mutex::new(None);

/// Record in the move's marker whether the 0.6 line runs (an existing
/// marker only: there is none before the move flips).
/// While the 0.6 line runs again on a moved profile, what reaches it (a
/// device back on 0.9.0, say) is new: a record that nothing is left on it
/// no longer holds, and the line stops again only after a full pass.
pub(crate) fn note_old_line_needed(data_dir: &Path, needed: bool) {
    use flowsta_hc_lines::marker::{Marker, MarkerRead};
    if let MarkerRead::Present(m) = Marker::load(data_dir, &crate::move_07::MARKER) {
        if m.old_line_needed != Some(needed) || (needed && m.carried_all_ms.is_some()) {
            let change = |m: &mut flowsta_hc_lines::marker::Marker| {
                m.old_line_needed = Some(needed);
                if needed {
                    m.carried_all_ms = None;
                }
            };
            if let Err(e) = flowsta_hc_lines::marker::update_marker(data_dir, &crate::move_07::MARKER, change) {
                log::warn!("[lines] the marker did not take old_line_needed = {needed}: {e}");
            }
        }
    }
}

/// Start the 0.6 line again beside a session that 0.7 leads: a device on
/// 0.6 came back, or something on 0.6 is not carried yet. The marker says
/// so first, so every write goes to 0.6 first again.
pub fn attach_06(app: tauri::AppHandle, data_dir: PathBuf) -> Result<(), String> {
    let session = supervisor().session().ok_or("no session runs")?;
    note_old_line_needed(&data_dir, true);
    let line_07 = LINE_07.lock().unwrap_or_else(|p| p.into_inner()).clone();
    supervisor().attach_line(session, Line::V06, Arc::new(YoaiLines { app: Some(app), data_dir, line_07 }));
    Ok(())
}

/// A 0.7 key store made fresh for a profile that starts on 0.7 alone: a
/// salt and the passphrase it gives (the key store itself is made at its
/// first start). Its marker says it is on 0.7 from the first write.
pub(crate) fn fresh_store_07(data_dir: &Path, data_key: &[u8; 32]) -> Result<String, String> {
    let (lair_07, _) = line_dirs(data_dir, Line::V07);
    std::fs::create_dir_all(&lair_07).map_err(|e| format!("make the 0.7 key store's folder: {e}"))?;
    let salt_path = lair_07.join(SALT_FILE_07);
    if !salt_path.exists() {
        let mut salt = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
        flowsta_hc_lines::files::write_atomic_private(&salt_path, hex::encode(salt).as_bytes()).map_err(|e| format!("write the 0.7 key store's salt: {e}"))?;
    }
    let mut m = flowsta_hc_lines::marker::Marker::new(&crate::move_07::MARKER, now_ms_conductor());
    m.restored = true;
    m.old_line_needed = Some(false);
    m.save(data_dir, &crate::move_07::MARKER).map_err(|e| format!("write the move's marker: {e}"))?;
    log::info!("[0.7] a fresh profile: it starts on the 0.7 line alone");
    passphrase_of_store_07(data_dir, data_key)
}

fn now_ms_conductor() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The lines of a profile in a test, without a window to narrate to.
#[cfg(test)]
pub(crate) async fn start_for_test(data_dir: PathBuf, line_07: Option<Line07>) -> Result<u64, String> {
    let secondary = line_07.is_some().then_some(Line::V07);
    if let Some(l07) = &line_07 {
        *LINE_07.lock().unwrap_or_else(|p| p.into_inner()) = Some(l07.clone());
    }
    supervisor().start_lines(Line::V06, secondary, Arc::new(YoaiLines { app: None, data_dir, line_07 })).await
}

#[cfg(test)]
mod rendezvous_tests {
    #[test]
    fn fallbacks_are_https_only_open_and_in_order() {
        let f = super::parse_fallbacks(" https://node1.example.com/, http://plain.example.com, https://node2.example.com:8443 ,");
        assert_eq!(f.len(), 2, "the plaintext one is dropped");
        assert_eq!(f[0].bootstrap_url, "https://node1.example.com");
        assert_eq!(f[0].signal_url(), "wss://node1.example.com");
        assert!(f[0].auth_material.is_none(), "fallbacks are open by design");
        assert_eq!(f[1].bootstrap_url, "https://node2.example.com:8443");
        assert_eq!(f[1].relay_url(), "https://node2.example.com.:8443/");
        assert!(super::parse_fallbacks("").is_empty());
    }

    #[test]
    fn the_config_keeps_the_settings_the_app_has_always_run_with() {
        use flowsta_hc_lines::conductor::{config_yaml, ConfigInput};
        let r = super::alone();
        let c = config_yaml(&ConfigInput {
            line: flowsta_hc_lines::Line::V06,
            conductor_dir: std::path::Path::new("/p/conductor"),
            lair_url: "unix:///p/lair/socket?k=K",
            admin_port: 4477,
            origin: "yoai-00",
            rendezvous: &r,
            tuning: &super::tuning(flowsta_hc_lines::Line::V06),
        });
        assert!(c.contains("allowed_origins: 'yoai-00'"), "the admin interface accepts only this start's origin");
        assert!(c.contains("db_sync_strategy: Resilient"));
        assert!(c.contains("bootstrap_url: https://localhost/") && !c.contains("base64_auth_material"), "no material: the black hole, alone");
        assert_eq!(super::tuning(flowsta_hc_lines::Line::V06).rust_log.as_deref(), Some("error,kitsune2_bootstrap_client=warn"));
    }

    #[test]
    fn admin_ports_start_at_the_usual_one_and_the_lines_never_share() {
        use flowsta_hc_lines::Line;
        let (p06, p07) = (super::admin_ports(Line::V06), super::admin_ports(Line::V07));
        assert_eq!(p06[0], crate::ports::admin_ws());
        assert_eq!(p06, [p06[0], p06[0] + 1, p06[0] + 2]);
        assert_eq!(p07, [p06[0] + 3, p06[0] + 4, p06[0] + 5]);
    }
}

#[cfg(test)]
mod line_07_tests {
    use super::*;

    #[test]
    fn the_07_passphrase_is_the_data_keys_and_this_installs() {
        let (key, salt) = ([7u8; 32], [9u8; 32]);
        let p = passphrase_07(&key, &salt);
        assert_eq!(p.len(), 64, "32 bytes, hex");
        assert_eq!(p, passphrase_07(&key, &salt), "derived the same at every start");
        assert_ne!(p, passphrase_07(&key, &[8u8; 32]), "another install's salt, another passphrase");
        assert_ne!(p, passphrase_07(&[6u8; 32], &salt), "another data key, another passphrase");
        assert_ne!(p, PASSPHRASE_06);
    }

    #[test]
    fn each_line_has_its_own_folders_sidecars_and_settings() {
        let root = Path::new("/p");
        assert_eq!(line_dirs(root, Line::V06), (root.join("lair"), root.join("conductor")));
        assert_eq!(line_dirs(root, Line::V07), (root.join("lair-07"), root.join("conductor-07")));
        assert_eq!(sidecars(Line::V07), ("yourowai-lair-keystore-07", "yourowai-holochain-07"));
        let t = tuning(Line::V07);
        assert_eq!(t.db_sync_level.as_deref(), Some("Full"));
        assert!(t.gossip_max_op_bytes.is_none(), "no gossip cap: this app's 0.7 conductor checks offered ids in chunks");
        assert_eq!(network_generation(Line::V07), "holochain-0.7");
    }

    /// The real sidecars: a 0.6 store holding an AI's random seed, prepared
    /// for 0.7 (copied, salted, rewrapped), starts the 0.7 line under the
    /// derived passphrase with the same key under the same tag; an AI made
    /// on 0.6 afterwards is in the next copy, under the same passphrase; the
    /// 0.6 store still opens as before.
    /// Run with the app closed: `cargo test --lib -- --ignored live_07`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_07_the_prepared_store_keeps_every_ai_key() {
        use flowsta_hc_lines::supervisor::KeySource;
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let origin = supervisor().origin_or_fresh();
        let tag = "ai-local-1794004195703-x".to_string();
        let data_key = [3u8; 32];

        let mut p06 = start_one(Line::V06, &profile, PASSPHRASE_06, &alone(), &origin, &[KeySource::NewSeed { tag: tag.clone() }], |_| {}).await.expect("0.6 starts");
        let key = p06.agents[0];
        p06.stop();

        let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
        assert!(line_dirs(&profile, Line::V07).0.join(SALT_FILE_07).is_file());

        // An AI made on 0.6 after the first copy is in the next one, under
        // the same passphrase.
        let later = "ai-made-later".to_string();
        let mut p06 = start_one(Line::V06, &profile, PASSPHRASE_06, &alone(), &origin, &[KeySource::NewSeed { tag: later.clone() }], |_| {}).await.expect("0.6 starts again");
        let later_key = p06.agents[0];
        p06.stop();
        assert_eq!(passphrase, prepare_store_07(&profile, &data_key).await.expect("refreshed"), "the salt is kept, so the passphrase too");
        assert!(!profile.join("lair-07.next").exists() && !profile.join("lair-07.prev").exists(), "no leftovers");
        let mut p07 = start_one(Line::V07, &profile, &passphrase, &alone(), &origin, &[KeySource::Existing { tag: later }], |_| {}).await.expect("0.7 has the later AI");
        assert_eq!(p07.agents[0], later_key);
        p07.stop();

        let mut p07 = start_one(Line::V07, &profile, &passphrase, &alone(), &origin, &[KeySource::Existing { tag: tag.clone() }], |_| {}).await.expect("0.7 starts under the derived passphrase");
        assert_eq!(p07.agents[0], key, "the AI's key on 0.7 is its key on 0.6");
        let config = std::fs::read_to_string(line_dirs(&profile, Line::V07).1.join("conductor-config.yaml")).unwrap();
        assert!(!config.contains("maxGossipOpBytes") && config.contains("db_sync_level: Full"));
        p07.stop();

        let mut again = start_one(Line::V06, &profile, PASSPHRASE_06, &alone(), &origin, &[KeySource::Existing { tag }], |_| {}).await.expect("0.6 still opens");
        assert_eq!(again.agents[0], key);
        again.stop();
    }

    /// The real sidecars through the supervisor, as the app runs them: an
    /// AI's cell on 0.6; then both lines, the 0.7 setup installing the AI's
    /// v2 app (with its one restart); then a conversation and a message
    /// written to v2 and read back by anchor and key.
    /// Run with the app closed: `cargo test --lib -- --ignored live_07`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_07_the_setup_installs_each_ais_v2_app_and_it_takes_records() {
        use crate::hc07::{Cells07, ConversationsPage, EntriesPage, RecordMessage, StartConversation};
        use flowsta_hc_lines::supervisor::KeySource;
        use holochain_types::prelude::{ActionHash, AgentPubKey, ExternalHash};
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let resource_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let (data_key, seed) = ([4u8; 32], format!("live-07-{}", std::process::id()));

        let only_06 = Arc::new(YoaiLines { app: None, data_dir: profile.clone(), line_07: None });
        supervisor().start_lines(Line::V06, None, only_06).await.expect("0.6 starts");
        let key = supervisor().add_agent(Line::V06, PASSPHRASE_06, &KeySource::NewSeed { tag: "ai-live".into() }).await.unwrap();
        let agent = AgentPubKey::from_raw_32(key.to_vec());
        crate::dna::install_transcript_app(admin_port().unwrap(), &resource_dir, agent.clone(), "live", &seed).await.expect("v1 app");
        supervisor().stop();

        let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
        let both = Arc::new(YoaiLines { app: None, data_dir: profile.clone(), line_07: Some(Line07 { passphrase, network_seed: seed, resource_dir }) });
        supervisor().start_lines(Line::V06, Some(Line::V07), both).await.expect("0.6 starts again");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(240);
        while !supervisor().has(Line::V07) {
            assert!(std::time::Instant::now() < until, "the 0.7 line did not join");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        let cells = Cells07::default();
        let agent_hex = hex::encode(agent.get_raw_39());
        let conversation = ExternalHash::from_raw_32(crate::transcript_ids::conversation_anchor(&data_key, "c-1").to_vec());
        let (cipher, nonce) = (vec![7u8; 40], vec![1u8; 24]);
        let _: ActionHash = cells
            .call(&agent_hex, "start_conversation", &StartConversation { cipher: cipher.clone(), nonce: nonce.clone(), anchor: agent.clone(), conversation_anchor: conversation.clone() })
            .await
            .expect("a conversation on v2");
        let mkey = crate::transcript_ids::message_key(&data_key, "m-1").to_vec();
        let _: ActionHash = cells
            .call(&agent_hex, "record_message", &RecordMessage { conversation_anchor: conversation.clone(), message_key: mkey.clone(), cipher: vec![8u8; 40], nonce: nonce.clone() })
            .await
            .expect("a message on v2");

        let page = cells.conversations_page(&agent_hex, &ConversationsPage { anchor: agent.clone(), before: None, limit: 10 }).await.expect("listed");
        assert_eq!(page.items.len(), 1, "one conversation under the founding key");
        assert_eq!(page.items[0].0.author, agent.get_raw_39().to_vec());
        let entries = cells.entries_page(&agent_hex, &EntriesPage { conversation_anchor: conversation, after: None, limit: 10 }).await.expect("entries");
        assert_eq!(entries.items.len(), 1, "one message under the conversation's anchor");
        assert_eq!(entries.items[0].1, mkey, "listed with its key");

        // A purge reaches the 0.7 line: the AI's v2 app is uninstalled.
        crate::hc07::purge("live").await;
        let admin = AdminClient::connect(Line::V07, ports(Line::V07).unwrap().0, &origin()).await.unwrap();
        assert!(!admin.list_apps().await.unwrap().iter().any(|a| a.app_id == "transcript_v2_live"), "purged on 0.7 too");
        supervisor().stop();
    }
}

#[cfg(test)]
mod store_07_trim_tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::Duration;

    #[test]
    fn an_ai_lists_keys_are_every_agent_key_it_names() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b, c) = (format!("842024{}", "ab".repeat(36)), format!("842024{}", "cd".repeat(36)), format!("842024{}", "ef".repeat(36)));
        let not_a_key = format!("842025{}", "ab".repeat(36));
        let list = serde_json::json!({ "custom-ais": [
            { "agentPubKey": a, "foundingKey": a, "id": a, "restoredFromId": b, "name": "Reeves" },
            { "agentPubKey": c, "skills": [{ "owner": a }], "seedSlot": "3", "id": not_a_key },
        ]});
        std::fs::write(tmp.path().join("ai-data.json"), list.to_string()).unwrap();
        let got: HashSet<[u8; 32]> = ai_list_keys(tmp.path()).into_iter().collect();
        let want: HashSet<[u8; 32]> = [&a, &b, &c].iter().map(|h| core_key_hex(h).unwrap()).collect();
        assert_eq!(got, want, "current, founding and earlier keys, deduplicated; a non-key left out");
        assert_eq!(core_key_hex(&a).unwrap(), [0xab; 32]);
        assert!(ai_list_keys(&tmp.path().join("none")).is_empty(), "no AI list: no keys");
    }

    /// A stopped 0.7 key store of `n` seeds made by the real 0.7 key store,
    /// with Holochain's connection stub: the seeds' keys.
    async fn store_07_with(dir: &Path, passphrase: &str, n: usize) -> Vec<[u8; 32]> {
        use flowsta_hc_lines::keystore::{start, v07, wait_ready, SocketPlan};
        let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(format!("yourowai-lair-keystore-07-{}", flowsta_hc_lines::TARGET_TRIPLE));
        let plan = SocketPlan::new("yoai-trim", "lair");
        let (mut child, url) = start(dir, &bin, passphrase, &plan).unwrap();
        wait_ready(&url, Duration::from_secs(15), &mut child, dir).await.unwrap();
        let client = v07::connect(&url, passphrase, Duration::from_secs(30)).await.unwrap();
        let mut keys = Vec::new();
        for i in 0..n {
            keys.push(v07::new_seed(&client, &format!("ai-{i:04}-6f1e3c2a-9b7d-4e21-a8f0-5c3d2b1a0e9f")).await.unwrap());
        }
        v07::new_seed(&client, "HC_CON_CHK_STUB").await.unwrap();
        drop(client);
        let _ = flowsta_hc_lines::process::stop_gracefully(&mut child, flowsta_hc_lines::process::StopRequest::Terminate, Duration::from_secs(2));
        keys
    }

    /// Run by hand: `cargo test --lib -- --ignored live_trim --nocapture`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_trim_keeps_the_keys_in_use_and_waits_when_even_those_do_not_fit() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("lair-07");
        let keys = store_07_with(&dir, "p", 60).await;
        assert!(!flowsta_hc_lines::keystore::entries(&dir, Line::V07, "p").await.unwrap().fits(), "sixty keys do not fit");
        let in_use: HashSet<[u8; 32]> = keys[..10].iter().copied().collect();
        trim_to(&dir, "p", &in_use).await.expect("ten keys fit");
        let after = flowsta_hc_lines::keystore::entries(&dir, Line::V07, "p").await.unwrap();
        assert!(after.fits(), "{} bytes", after.reply_bytes);
        let held: HashSet<[u8; 32]> = after.entries.iter().filter_map(|e| e.ed25519).collect();
        assert!(in_use.iter().all(|k| held.contains(k)), "every key in use kept");
        assert!(keys[10..].iter().all(|k| !held.contains(k)), "the rest left out");
        assert!(after.entries.iter().any(|e| e.tag == "HC_CON_CHK_STUB"), "Holochain's own entry kept");
        assert_eq!(after.entries.len(), 11);

        let full = tmp.path().join("lair-07-full");
        let all: HashSet<[u8; 32]> = store_07_with(&full, "p", 60).await.into_iter().collect();
        let err = trim_to(&full, "p", &all).await.expect_err("sixty keys in use do not fit");
        assert!(err.starts_with("this computer has more AIs than the new version can take yet"), "{err}");
        println!("TRIM guard: {err}");
    }

    #[test]
    fn the_keys_noted_on_07_read_back_and_none_noted_is_not_an_empty_note() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(noted_keys_07(tmp.path()).is_none(), "no 0.7 session has noted its keys yet");
        write_noted_keys_07(tmp.path(), &[[1u8; 32], [2u8; 32]].into_iter().collect());
        assert_eq!(noted_keys_07(tmp.path()).unwrap().into_iter().collect::<Vec<_>>(), vec![[1u8; 32], [2u8; 32]]);
        write_noted_keys_07(tmp.path(), &Default::default());
        assert_eq!(noted_keys_07(tmp.path()).map(|k| k.len()), Some(0), "a line with no apps noted none");
    }

    /// A moved profile's store: `cargo test --lib -- --ignored live_prune --nocapture`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_prune_keeps_noted_and_listed_keys_once_the_store_nears_one_lair_message() {
        let held = |profile: PathBuf| async move {
            let l = flowsta_hc_lines::keystore::entries(&profile.join("lair-07"), Line::V07, "p").await.unwrap();
            (l.entries.iter().filter_map(|e| e.ed25519).collect::<HashSet<[u8; 32]>>(), l.entries.len(), l.reply_bytes)
        };
        let agent_hex = |k: &[u8; 32]| hex::encode(holochain_types::prelude::AgentPubKey::from_raw_32(k.to_vec()).get_raw_39());

        // Forty-five keys, most from AIs since removed.
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("moved");
        let keys = store_07_with(&profile.join("lair-07"), "p", 45).await;
        prune_store_07(&profile, "p").await;
        assert_eq!(held(profile.clone()).await.1, 46, "with no note of the keys in use, the store is left as it is");

        // Ten noted while the line ran, two more named by the AI list.
        write_noted_keys_07(&profile, &keys[..10].iter().copied().collect());
        let list = serde_json::json!({ "custom-ais": [{ "agentPubKey": agent_hex(&keys[10]), "foundingKey": agent_hex(&keys[11]) }] });
        std::fs::write(profile.join("ai-data.json"), list.to_string()).unwrap();
        prune_store_07(&profile, "p").await;
        let (after, entries, bytes) = held(profile.clone()).await;
        assert!(keys[..12].iter().all(|k| after.contains(k)), "the noted and the listed keys are kept");
        assert!(keys[12..].iter().all(|k| !after.contains(k)), "the rest are left out");
        assert_eq!(entries, 13, "and Holochain's own entry");
        println!("PRUNE: 46 entries -> {entries} ({bytes} bytes)");

        // A store with room is not touched, whatever the note says.
        let roomy = tmp.path().join("roomy");
        let few = store_07_with(&roomy.join("lair-07"), "p", 20).await;
        write_noted_keys_07(&roomy, &few[..5].iter().copied().collect());
        prune_store_07(&roomy, "p").await;
        assert_eq!(held(roomy).await.1, 21, "twenty keys leave room for more");

        // Every key in use and still too many: nothing is lost, and the store stands.
        let full = tmp.path().join("full");
        let all = store_07_with(&full.join("lair-07"), "p", 45).await;
        write_noted_keys_07(&full, &all.iter().copied().collect());
        prune_store_07(&full, "p").await;
        assert_eq!(held(full).await.1, 46);
    }
}
