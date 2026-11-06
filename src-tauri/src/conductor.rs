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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use tauri::Emitter;

/// The Holochain network generation this build's conductor speaks - the
/// Vault's vocabulary (`devices.rs` NETWORK_GENERATION), so a sibling on
/// another generation reads the same way everywhere: "Needs the update to
/// sync with <device>". Bump with the Holochain upgrade (0.7.x).
pub const NETWORK_GENERATION: &str = "holochain-0.6";

/// The passphrase of the 0.6 line's key store and conductor databases.
/// Kept as it has always been until the 0.6 line retires, so nothing that
/// exists is rewritten.
pub(crate) const PASSPHRASE_06: &str = "your-own-ai-local";

static SUPERVISOR: LazyLock<Arc<Supervisor>> = LazyLock::new(|| Arc::new(Supervisor::new("yoai")));

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

/// Where a key store's socket goes when its own folder's path is over the
/// Unix-socket limit: the folder and naming the app has always used, so an
/// existing key store keeps its socket.
pub fn socket_plan() -> SocketPlan {
    SocketPlan::new("fv", "lair")
}

/// The 0.6 conductor's admin port candidates, the first free one used.
fn admin_ports() -> [u16; 3] {
    let base = crate::ports::admin_ws();
    [base, base + 1, base + 2]
}

/// The 0.6 conductor's settings, as the app has always run it: SQLite synced
/// in full on every write, errors logged plus the bootstrap client's
/// warnings (a refused registration, "Bootstrap PUT returned HTTP error", is
/// the one line that tells a stranded device from a quiet one).
fn tuning() -> NetworkTuning {
    let mut t = NetworkTuning::for_line(Line::V06);
    t.db_sync_strategy = Some("Resilient".into());
    t.rust_log = Some("error,kitsune2_bootstrap_client=warn".into());
    t
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
    Some(Rendezvous { bootstrap_url, signal_url: Some(signal_url), auth_material: Some(auth_material) })
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
            Rendezvous { bootstrap_url, signal_url: Some(format!("wss://{}", host)), auth_material: None }
        })
        .collect()
}

/// No rendezvous at all: a localhost black hole, one device.
fn alone() -> Rendezvous {
    Rendezvous { bootstrap_url: "https://localhost/".into(), signal_url: Some("wss://localhost/".into()), auth_material: None }
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
/// (`rendezvous_keeper`): Flowsta's with this app's auth material first,
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

// ── The start ───────────────────────────────────────────────────────────

/// What the app's lines need from the app (flowsta-hc-lines `LineHooks`):
/// the profile's folders and the window to narrate the start to.
struct YoaiLines {
    app: tauri::AppHandle,
    /// The open profile's root.
    data_dir: PathBuf,
}

impl YoaiLines {
    fn emit(&self, status: ConductorStatus) {
        let _ = self.app.emit("conductor-status", status);
    }

    fn say(&self, message: &str) {
        self.emit(ConductorStatus::Starting { message: message.into() });
    }
}

#[async_trait::async_trait]
impl LineHooks for YoaiLines {
    async fn start(&self, line: Line, _role: Role) -> Result<LineProcess, String> {
        if line != Line::V06 {
            return Err(format!("this build runs no {line} line"));
        }
        let entry = match resolve().await {
            Some(r) => {
                let start = r.for_start(PROBE_TIMEOUT).await;
                if start.without_auth {
                    log::warn!("[conductor] no rendezvous answers ({}): starting without its auth, offline - the next start uses it", start.rendezvous.bootstrap_url);
                }
                STARTED_OFFLINE.store(start.without_auth, Ordering::SeqCst);
                log::info!("[conductor] rendezvous {} (the person's own devices can meet)", start.rendezvous.bootstrap_url);
                supervisor().set_rendezvous(line, Some(&start.rendezvous.bootstrap_url));
                start.rendezvous
            }
            None => {
                log::info!("[conductor] no rendezvous - this conductor stays alone");
                supervisor().set_rendezvous(line, None);
                alone()
            }
        };
        let origin = supervisor().fresh_origin();
        let lair_bin = crate::resolve_sidecar_bin("yourowai-lair-keystore");
        let conductor_bin = crate::resolve_sidecar_bin("yourowai-holochain");
        log::info!("[conductor] key store {:?}, conductor {:?}", lair_bin, conductor_bin);
        let (lair_dir, conductor_dir) = (self.data_dir.join("lair"), self.data_dir.join("conductor"));
        supervisor::start_line(
            &LineStart {
                line,
                lair_bin: &lair_bin,
                conductor_bin: &conductor_bin,
                lair_dir: &lair_dir,
                conductor_dir: &conductor_dir,
                passphrase: PASSPHRASE_06,
                sockets: &socket_plan(),
                admin_ports: &admin_ports(),
                origin: &origin,
                rendezvous: &entry,
                tuning: &tuning(),
                agents: &[],
                timeouts: Timeouts::default(),
            },
            |step| self.say(step_message(step)),
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// The app interface, accepting only this start's origin. The
    /// coordinator sweep and the agents follow once the line has joined.
    async fn setup(&self, line: Line, _role: Role, process: &mut LineProcess) -> Result<Setup, String> {
        self.say("Setting up app interface...");
        let origin = supervisor().origin();
        let admin = AdminClient::connect(line, process.admin_port, &origin).await.map_err(|e| format!("Failed to connect to admin WebSocket: {e}"))?;
        let app_port = admin.attach_app_interface(&origin).await.map_err(|e| format!("Failed to attach app interface: {e}"))?;
        log::info!("App interface attached on port {}", app_port);
        Ok(app_port.into())
    }

    async fn joined(&self, _line: Line, _role: Role, _session: u64, admin_port: u16, app_port: u16) {
        self.emit(ConductorStatus::Ready { admin_port, app_port });
        log::info!("Holochain conductor ready (admin: {}, app: {})", admin_port, app_port);
    }

    fn wanted(&self) -> bool {
        !closing()
    }
}

/// Start the open profile's key store and conductor; returns once they are
/// up and the app interface is attached. The start gets one retry after a
/// failure (2 s later, so ports and processes are released).
pub async fn start_holochain(app: tauri::AppHandle, data_dir: PathBuf) -> Result<u64, String> {
    let hooks = Arc::new(YoaiLines { app, data_dir });
    match supervisor().start_lines(Line::V06, None, hooks).await {
        Err(e) if e == NOT_WANTED => Err("the app is closing".into()),
        other => other,
    }
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
            tuning: &super::tuning(),
        });
        assert!(c.contains("allowed_origins: 'yoai-00'"), "the admin interface accepts only this start's origin");
        assert!(c.contains("db_sync_strategy: Resilient"));
        assert!(c.contains("bootstrap_url: https://localhost/") && !c.contains("base64_auth_material"), "no material: the black hole, alone");
        assert_eq!(super::tuning().rust_log.as_deref(), Some("error,kitsune2_bootstrap_client=warn"));
    }

    #[test]
    fn admin_ports_start_at_the_usual_one() {
        let p = super::admin_ports();
        assert_eq!(p[0], crate::ports::admin_ws());
        assert_eq!(p, [p[0], p[0] + 1, p[0] + 2]);
    }
}
