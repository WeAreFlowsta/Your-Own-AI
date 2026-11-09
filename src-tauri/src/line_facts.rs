//! Where each of the person's computers is on the move to the 0.7 line, and
//! the facts the 0.6 line's stop decides from.
//!
//! - Each device writes a small `line` object about itself to its Vault: its
//!   generation, app version and move state. It is written outside the
//!   backup's gates, so a held backup (restore pending, an escrow conflict)
//!   cannot hide a move from the other devices.
//! - A device counts as moved only when its own `line` object or manifest
//!   says `holochain-0.7`. Anything else is on 0.6: it says so, it says
//!   nothing (every release before 0.9.0), or its word could not be read.
//! - A device whose key material is not this device's never shared the
//!   person's network (every release before 0.9.0 made its own key): it
//!   holds nothing open here.
//! - A device the person stopped waiting for counts as removed until one of
//!   its backups changes again. Nothing is ever counted as removed
//!   because it is missing from a list.
//! - A device list the Vault could not give is unknown, never "none": the
//!   facts last known are used, and with none the 0.6 line is wanted.

use crate::move_07::MARKER;
use crate::transcript_crypto::RecoveryMaterial;
use flowsta_hc_lines::marker::{Marker, MarkerRead};
use flowsta_hc_lines::policy::{self, Decision, RetirementPolicy, Sibling, Siblings, Why};
use flowsta_hc_lines::Line;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The label of the object each device writes about itself.
pub(crate) const LINE_LABEL: &str = "line";
const FACTS_FILE: &str = "line-facts.json";

/// The minute round, Settings' "sync now" and "Stop waiting" all change the
/// facts file: one at a time, so none of them loses another's change.
static FACTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Load, change and save the facts under the lock.
fn change_facts<T>(root: &Path, change: impl FnOnce(&mut LineFacts) -> Result<T, String>) -> Result<T, String> {
    let _held = FACTS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut facts = LineFacts::load(root).unwrap_or_default();
    let out = change(&mut facts)?;
    facts.save(root)?;
    Ok(out)
}

fn gen_06() -> &'static str {
    crate::conductor::network_generation(Line::V06)
}
fn gen_07() -> &'static str {
    crate::conductor::network_generation(Line::V07)
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoveState {
    /// The 0.7 line has not run on this profile yet.
    NotStarted,
    /// Both lines run and the records are being carried.
    Moving,
    /// The marker says writes go to 0.7.
    Moved,
}

/// What a device says about itself in its `line` object.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LineReport {
    pub v: u32,
    pub network_generation: String,
    pub app_version: String,
    pub move_state: MoveState,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SiblingLine {
    Moved,
    OnOld,
    /// Its key material is not this device's: never on the person's network.
    Separate,
}

/// The backup list's stamps for one device's objects (`created_at`, as text).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Stamps {
    pub manifest_at: Option<String>,
    pub ais_at: Option<String>,
    pub line_at: Option<String>,
    pub escrow_at: Option<String>,
}

/// A copy's stamp from the Vault's list, in ms (a number, quoted or not).
pub(crate) fn stamp_ms(s: &Option<String>) -> Option<i64> {
    s.as_deref().and_then(|v| v.trim_matches('"').parse::<i64>().ok())
}

impl Stamps {
    pub fn of(d: &crate::vault_escrow::SiblingDevice) -> Self {
        Stamps { manifest_at: d.manifest_at.clone(), ais_at: d.ais_at.clone(), line_at: d.line_at.clone(), escrow_at: d.escrow_at.clone() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KnownSibling {
    pub id: String,
    pub name: Option<String>,
    pub line: SiblingLine,
    /// Its `line` object, when it writes one (0.9.1 and later).
    pub report: Option<LineReport>,
    /// The generation its manifest or `ais` object last named.
    pub manifest_generation: Option<String>,
    /// Whether its key material is this device's (None: not read).
    pub material_same: Option<bool>,
    pub stamps: Stamps,
}

impl KnownSibling {
    /// On 0.6 and writing no `line` object: an older release. The one to
    /// tell the person about ("update it"); a 0.9.1 device still moving is
    /// not.
    pub fn needs_update(&self) -> bool {
        self.line == SiblingLine::OnOld && self.report.is_none()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StoppedWaiting {
    pub id: String,
    pub at_ms: u64,
    /// The device's stamps when the person stopped waiting: any change
    /// means it is back, and the wait resumes by itself.
    pub stamps: Stamps,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LineFacts {
    #[serde(default)]
    pub v: u32,
    /// When the Vault's device list was last read in full (0: never).
    #[serde(default)]
    pub checked_at_ms: u64,
    #[serde(default)]
    pub siblings: Vec<KnownSibling>,
    #[serde(default)]
    pub stopped_waiting: Vec<StoppedWaiting>,
    /// What this device last wrote about itself.
    #[serde(default)]
    pub reported: Option<LineReport>,
}

/// A sibling's line from what it said: other key material first,
/// then its own word, then its manifest's; on 0.6 otherwise. An unread
/// material counts as the same (unknown is never "separate").
pub fn classify(report: Option<&LineReport>, manifest_generation: Option<&str>, material_same: Option<bool>) -> SiblingLine {
    if material_same == Some(false) {
        return SiblingLine::Separate;
    }
    if report.map(|r| r.network_generation == gen_07()).unwrap_or(false) || manifest_generation == Some(gen_07()) {
        return SiblingLine::Moved;
    }
    SiblingLine::OnOld
}

/// A device's `line` object, unless an earlier release runs there again:
/// it said 0.7, and a newer manifest (or `ais` object) names the 0.6 line.
/// That happens only when a release before this one is installed by hand
/// after the move; it writes no `line` object, so the old one would keep
/// saying "moved" while it writes to the 0.6 line alone.
fn current_report(report: Option<LineReport>, manifest_generation: Option<&str>, stamps: &Stamps) -> Option<LineReport> {
    let generation_at = stamp_ms(&stamps.manifest_at).max(stamp_ms(&stamps.ais_at));
    let went_back = report.as_ref().is_some_and(|r| r.network_generation == gen_07())
        && manifest_generation == Some(gen_06())
        && matches!((generation_at, stamp_ms(&stamps.line_at)), (Some(g), Some(l)) if g > l);
    if went_back { None } else { report }
}

impl LineFacts {
    pub fn load(root: &Path) -> Option<LineFacts> {
        let bytes = std::fs::read(root.join(FACTS_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = root.join(format!("{FACTS_FILE}.part"));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, root.join(FACTS_FILE)).map_err(|e| e.to_string())
    }

    fn stopped(&self, id: &str) -> bool {
        self.stopped_waiting.iter().any(|s| s.id == id)
    }

    /// The siblings as the library reads them: a separate device is left
    /// out, one the person stopped waiting for is removed.
    pub fn siblings(&self) -> Siblings {
        Siblings::Heard(
            self.siblings
                .iter()
                .filter(|s| s.line != SiblingLine::Separate)
                .map(|s| Sibling {
                    device_id: s.id.clone(),
                    generation: if s.line == SiblingLine::Moved { gen_07() } else { gen_06() }.to_string(),
                    removed: self.stopped(&s.id),
                })
                .collect(),
        )
    }

    /// The devices keeping the 0.6 line wanted here: on 0.6, sharing the
    /// key, not stopped waiting for.
    pub fn holding(&self) -> Vec<&KnownSibling> {
        self.siblings.iter().filter(|s| s.line == SiblingLine::OnOld && !self.stopped(&s.id)).collect()
    }

    /// Take a round's view of the devices. A wait the person stopped ends
    /// when that device's stamps change (it is back); a device no longer
    /// listed keeps its stop.
    pub fn take_round(&mut self, seen: Vec<KnownSibling>, now_ms: u64) -> Vec<String> {
        let mut resumed = Vec::new();
        self.stopped_waiting.retain(|w| match seen.iter().find(|s| s.id == w.id) {
            Some(s) if s.stamps != w.stamps => {
                resumed.push(w.id.clone());
                false
            }
            _ => true,
        });
        self.siblings = seen;
        self.checked_at_ms = now_ms;
        self.v = 1;
        resumed
    }

    pub fn stop_waiting(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        let s = self.siblings.iter().find(|s| s.id == id).ok_or("unknown_device")?;
        let stamps = s.stamps.clone();
        self.stopped_waiting.retain(|w| w.id != id);
        self.stopped_waiting.push(StoppedWaiting { id: id.to_string(), at_ms: now_ms, stamps });
        Ok(())
    }

    pub fn wait_again(&mut self, id: &str) {
        self.stopped_waiting.retain(|w| w.id != id);
    }
}

/// What the facts are, as far as this device knows.
#[derive(Clone, Debug, PartialEq)]
pub enum Known {
    /// Never linked to a Vault and its material never escrowed: no other
    /// device can share its network.
    NeverShared,
    Facts(LineFacts),
    /// Linked, and the device list has never been read here.
    Unknown,
}

/// Whether this profile still wants the 0.6 line, and who holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct LineDecision {
    pub decision: Decision,
    /// Plain words for diagnostics.
    pub why: String,
    /// The devices that keep it wanted (ids), for the note that names them.
    pub holding: Vec<String>,
}

/// The decision, from local facts alone. Adds two rules the library's
/// order does not have, for this app:
/// - unknown facts want the 0.6 line (unknown is never "none");
/// - a profile with no marker and no 0.6 data (a fresh install) still wants
///   it while another device on 0.6 shares its key: the
///   library would answer `Fresh` before it looks at the siblings.
pub fn decide(known: &Known, marker: MarkerRead, old_line_data_present: bool, now_ms: u64) -> LineDecision {
    let (siblings, holding) = match known {
        Known::NeverShared => (Siblings::NeverShared, Vec::new()),
        Known::Unknown => {
            return LineDecision { decision: Decision::Wanted(Why::SiblingOnOld("unknown".into())), why: "the other devices are not known yet".into(), holding: Vec::new() };
        }
        Known::Facts(f) => (f.siblings(), f.holding().into_iter().map(|s| s.id.clone()).collect::<Vec<_>>()),
    };
    if matches!(marker, MarkerRead::Absent) && !old_line_data_present {
        if let Some(first) = holding.first() {
            return LineDecision { decision: Decision::Wanted(Why::SiblingOnOld(first.clone())), why: "a fresh profile beside a device still on 0.6".into(), holding };
        }
    }
    let facts = policy::Facts { marker, old_line_data_present, own_device_id: None, siblings, owed: Vec::new(), now_ms };
    let decision = policy::old_line_wanted(&facts, &RetirementPolicy::NEVER_ALONE, &MARKER);
    let why = match &decision {
        Decision::Wanted(Why::NotMoved) => "this profile has not moved yet".to_string(),
        Decision::Wanted(Why::SiblingOnOld(_)) => format!("{} other device(s) still on 0.6", holding.len()),
        Decision::Wanted(Why::NotCarried) => "no complete pass yet since every device left 0.6".to_string(),
        Decision::Wanted(w) => format!("{w:?}"),
        Decision::NotWanted(w) => format!("not needed ({w:?})"),
    };
    LineDecision { decision, why, holding }
}

/// The generation this profile's records are on: 0.7 once its marker says
/// writes go there.
pub fn own_generation(root: &Path) -> &'static str {
    let read = Marker::load(root, &MARKER);
    // An unreadable marker sends writes to 0.7, but tells the others 0.6:
    // they keep waiting rather than leave this device behind.
    if matches!(read, MarkerRead::Present(_)) && Marker::writes_to(&read, &MARKER) == Line::V07 {
        gen_07()
    } else {
        gen_06()
    }
}

fn move_state(root: &Path) -> MoveState {
    match Marker::load(root, &MARKER) {
        MarkerRead::Present(_) if own_generation(root) == gen_07() => MoveState::Moved,
        _ if crate::conductor::ports(Line::V07).is_some() => MoveState::Moving,
        _ => MoveState::NotStarted,
    }
}

/// What this device says about itself now.
pub fn own_report(app: &tauri::AppHandle, root: &Path) -> LineReport {
    LineReport { v: 1, network_generation: own_generation(root).to_string(), app_version: app.package_info().version.to_string(), move_state: move_state(root) }
}

/// The facts as this device knows them: last saved, or never shared, or
/// unknown.
pub fn known(app: &tauri::AppHandle, root: &Path) -> Known {
    match LineFacts::load(root) {
        Some(f) if f.checked_at_ms > 0 => Known::Facts(f),
        _ if never_shared(app) => Known::NeverShared,
        _ => Known::Unknown,
    }
}

/// How long a first start waits for the Vault to say where the other
/// devices are.
const FIRST_FACTS_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Before a start that knows nothing of the other devices (no facts kept,
/// and the profile is linked or escrowed): read them from the Vault, at most
/// [`FIRST_FACTS_WAIT`]. Without an answer the facts stay unknown, and
/// unknown runs the 0.6 line.
pub async fn learn_before_first_start(app: &tauri::AppHandle, root: &Path) {
    if LineFacts::load(root).is_some_and(|f| f.checked_at_ms > 0) || never_shared(app) {
        return;
    }
    match tokio::time::timeout(FIRST_FACTS_WAIT, crate::config_sync::facts_round(app)).await {
        Ok(Ok(true)) => {
            let n = LineFacts::load(root).map(|f| f.siblings.len()).unwrap_or(0);
            log::info!("[line] before the first start: {n} other device(s) read from the Vault");
        }
        Ok(Ok(false)) => log::info!("[line] before the first start: the Vault is still reading the identity's devices; they stay unknown"),
        Ok(Err(e)) => log::info!("[line] before the first start: the other devices could not be read ({e}); they stay unknown"),
        Err(_) => log::info!("[line] before the first start: no answer from the Vault within {} s; the other devices stay unknown", FIRST_FACTS_WAIT.as_secs()),
    }
}

/// Never linked, and no escrow owner recorded: the key was made here and
/// has never left this device.
fn never_shared(app: &tauri::AppHandle) -> bool {
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store(crate::profile::store_path(app, crate::flowsta::AUTH_STORE)) else { return false };
    let linked = store.get("link_done").and_then(|v| v.as_bool()).unwrap_or(false);
    let escrowed = store.get(crate::vault_escrow::ESCROW_OWNER_KEY).and_then(|v| v.as_str().map(String::from)).is_some();
    !linked && !escrowed
}

/// The 0.6 line's data is on this device (its key store exists).
pub fn old_line_data_present(root: &Path) -> bool {
    let (lair_06, _) = crate::conductor::line_dirs(root, Line::V06);
    flowsta_hc_lines::keystore::config_path(&lair_06).exists()
}

/// The newest change to the 0.6 conductor's databases when this device
/// stopped the line, in ms since the epoch.
pub(crate) const STOP_PRINT_FILE: &str = "line-06-at-stop";

/// The newest change under the 0.6 conductor's databases, in ms. They
/// change only while a 0.6 conductor runs.
fn newest_06_change_ms(root: &Path) -> Option<u64> {
    let (_, conductor_06) = crate::conductor::line_dirs(root, Line::V06);
    let mut newest: Option<u64> = None;
    let mut dirs = vec![(conductor_06.join("databases"), 0u8)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if depth < 3 {
                    dirs.push((e.path(), depth + 1));
                }
                continue;
            }
            let Some(ms) = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64) else { continue };
            newest = Some(newest.map_or(ms, |n| n.max(ms)));
        }
    }
    newest
}

/// Called once the 0.6 line has stopped: what its databases looked like.
pub fn note_stopped(root: &Path) {
    let Some(ms) = newest_06_change_ms(root) else { return };
    if let Err(e) = std::fs::write(root.join(STOP_PRINT_FILE), ms.to_string()) {
        log::warn!("[0.6] the stop's record did not save: {e}");
    }
}

/// Before the start's plan: the 0.6 line ran on this profile after this
/// device recorded that nothing was left on it (an earlier release
/// installed by hand). What it wrote is not carried yet, so the record is
/// cleared and the 0.6 line runs until a full pass carries it. Without a
/// stop record (the app quit between the full pass and the stop) it runs
/// once more too: a pass costs time, a record left behind is lost from view.
pub fn reopen_if_written_since_stop(root: &Path) -> bool {
    let MarkerRead::Present(m) = Marker::load(root, &MARKER) else { return false };
    if m.carried_all_ms.is_none() || !old_line_data_present(root) {
        return false;
    }
    let at_stop = std::fs::read_to_string(root.join(STOP_PRINT_FILE)).ok().and_then(|s| s.trim().parse::<u64>().ok());
    let written = match (at_stop, newest_06_change_ms(root)) {
        (Some(stop), Some(now)) => now > stop,
        (Some(_), None) => false,
        (None, _) => true,
    };
    if !written {
        return false;
    }
    if let Err(e) = flowsta_hc_lines::marker::update_marker(root, &MARKER, |m| m.carried_all_ms = None) {
        log::warn!("[0.6] the 0.6 line changed since the stop, but the marker did not take it: {e}");
        return false;
    }
    let _ = std::fs::remove_file(root.join(STOP_PRINT_FILE));
    log::info!("[0.6] {}: it runs again until a full pass carries what is on it", if at_stop.is_some() { "written since this device stopped it (an earlier release ran here)" } else { "no record of the stop" });
    true
}

/// Which line leads this start, and which joins beside it. `have_07`: the
/// 0.7 line can start (its key store is ready).
/// - no marker, 0.6 data here: 0.6 leads and 0.7 joins (the move runs);
/// - no marker, no 0.6 data (a fresh profile): the same while a device on
///   0.6 shares its key or the facts are unknown, else 0.7 alone;
/// - a marker (moved): 0.7 leads, and 0.6 joins only while it is wanted
///   (a device still on it, or something not yet carried).
pub fn start_plan(app: &tauri::AppHandle, root: &Path, have_07: bool) -> (Line, Option<Line>) {
    if !have_07 {
        return (Line::V06, None);
    }
    let marker = Marker::load(root, &MARKER);
    let old = old_line_data_present(root);
    let moved = !matches!(marker, MarkerRead::Absent);
    let wanted = decide(&known(app, root), marker, old, now_ms()).decision.wanted();
    plan(moved, old, wanted)
}

/// [`start_plan`]'s rule, from its three facts.
fn plan(moved: bool, old_line_data: bool, old_line_wanted: bool) -> (Line, Option<Line>) {
    match (moved, old_line_data) {
        (false, true) => (Line::V06, Some(Line::V07)),
        (false, false) if old_line_wanted => (Line::V06, Some(Line::V07)),
        (false, false) => (Line::V07, None),
        (true, _) => (Line::V07, old_line_wanted.then_some(Line::V06)),
    }
}

/// A fresh profile (no marker, no 0.6 data) that starts on the 0.7 line
/// alone: no device on 0.6 shares its key, and the facts are known.
pub fn fresh_starts_on_07(app: &tauri::AppHandle, root: &Path) -> bool {
    matches!(Marker::load(root, &MARKER), MarkerRead::Absent)
        && !old_line_data_present(root)
        && !decide(&known(app, root), MarkerRead::Absent, false, now_ms()).decision.wanted()
}

/// The decision for this profile now (diagnostics, the stop).
pub fn decision(app: &tauri::AppHandle, root: &Path) -> LineDecision {
    decide(&known(app, root), Marker::load(root, &MARKER), old_line_data_present(root), now_ms())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Write this device's `line` object when what it says changed. Called
/// every round while the Vault answers; the backup's own gates do not
/// apply (only the identity's).
pub async fn report(app: &tauri::AppHandle) -> Result<bool, String> {
    let port = crate::vault_escrow::escrow_port(app, true).await.map_err(|s| s.error.unwrap_or(s.state))?;
    let root = crate::profile::root(app)?;
    let now = own_report(app, &root);
    if LineFacts::load(&root).and_then(|f| f.reported).as_ref() == Some(&now) {
        return Ok(false);
    }
    let data = serde_json::to_value(&now).map_err(|e| e.to_string())?;
    crate::vault_escrow::post_backup_json(port, LINE_LABEL, &data).await?;
    log::info!("[line] told the other devices: {} ({:?}, {})", now.network_generation, now.move_state, now.app_version);
    change_facts(&root, |f| {
        f.reported = Some(now);
        Ok(())
    })?;
    Ok(true)
}

/// One device's facts this round. `generation`: what its manifest or `ais`
/// object named, when the round read one (else the last one known).
pub async fn observe(
    port: u16,
    device: &crate::vault_escrow::SiblingDevice,
    prior: Option<&KnownSibling>,
    generation: Option<String>,
    local: Option<&RecoveryMaterial>,
) -> KnownSibling {
    let stamps = Stamps::of(device);
    let short = &device.id[..device.id.len().min(8)];
    let report = match (prior, &stamps.line_at) {
        (Some(p), at) if p.stamps.line_at == *at => p.report.clone(),
        (_, None) => None,
        _ => match crate::vault_escrow::retrieve_from_device(port, LINE_LABEL, &device.id).await {
            Ok(Some(v)) => serde_json::from_value::<LineReport>(v["data"].clone()).ok(),
            Ok(None) => None,
            Err(e) => {
                log::warn!("[line] line object from {}: {}", short, e);
                prior.and_then(|p| p.report.clone())
            }
        },
    };
    let material_same = match (prior, local) {
        (Some(p), _) if p.stamps.escrow_at == stamps.escrow_at && p.material_same.is_some() => p.material_same,
        (_, None) => None,
        (_, Some(mine)) => match crate::vault_escrow::retrieve_from_device(port, crate::vault_escrow::ESCROW_LABEL, &device.id).await {
            Ok(Some(v)) => crate::vault_escrow::parse_escrow(&v["data"]).map(|theirs| theirs == *mine),
            Ok(None) => None,
            Err(e) => {
                log::warn!("[line] key material from {}: {}", short, e);
                prior.and_then(|p| p.material_same)
            }
        },
    };
    let manifest_generation = generation.or_else(|| prior.and_then(|p| p.manifest_generation.clone()));
    let report = current_report(report, manifest_generation.as_deref(), &stamps);
    let line = classify(report.as_ref(), manifest_generation.as_deref(), material_same);
    KnownSibling { id: device.id.clone(), name: device.name.clone(), line, report, manifest_generation, material_same, stamps }
}

/// The Vault says it is still reading the identity's records: the device
/// list may be short, so this round says nothing (the facts last known
/// stand).
pub async fn vault_catching_up(port: u16) -> bool {
    use crate::flowsta::{http, VAULT_ORIGIN};
    match http()
        .get(crate::flowsta::identity_query(format!("http://127.0.0.1:{}/devices", port)))
        .header("Origin", VAULT_ORIGIN)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok().and_then(|v| v["catching_up"].as_bool()).unwrap_or(false),
        // An older Vault has no `/devices`: its backup list is all there is.
        Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => false,
        _ => true,
    }
}

/// Save a round's view of the devices.
pub fn take_round(root: &Path, seen: Vec<KnownSibling>) -> Result<(), String> {
    change_facts(root, |facts| {
        let before: Vec<(String, SiblingLine)> = facts.siblings.iter().map(|s| (s.id.clone(), s.line)).collect();
        let resumed = facts.take_round(seen, now_ms());
        for id in resumed {
            log::info!("[line] {} is back: waiting for it again", &id[..id.len().min(8)]);
        }
        for s in &facts.siblings {
            if !before.iter().any(|(id, line)| *id == s.id && *line == s.line) {
                log::info!("[line] {} ({}): {:?}", s.name.as_deref().unwrap_or("another device"), &s.id[..s.id.len().min(8)], s.line);
            }
        }
        Ok(())
    })
}

/// What the app shows about the lines (Settings > Devices, diagnostics).
#[derive(Serialize)]
pub struct LineStatus {
    pub generation: String,
    pub move_state: MoveState,
    pub old_line_wanted: bool,
    /// The 0.6 line runs on this device right now (it stops at the watch's
    /// next round once it is no longer wanted).
    pub old_line_running: bool,
    pub why: String,
    pub holding: Vec<HoldingDevice>,
    pub stopped_waiting: Vec<String>,
    pub checked_at_ms: u64,
    /// The move's last pass (none yet this session), and the AIs it could
    /// not carry, with why: a move that does not finish says so.
    pub last_pass: Option<crate::move_07::LastPass>,
}

#[derive(Serialize)]
pub struct HoldingDevice {
    pub id: String,
    pub name: Option<String>,
    pub app_version: Option<String>,
    pub needs_update: bool,
}

#[tauri::command]
pub fn line_status(app: tauri::AppHandle) -> Result<LineStatus, String> {
    let root = crate::profile::root(&app)?;
    let d = decision(&app, &root);
    let facts = LineFacts::load(&root).unwrap_or_default();
    let holding = d
        .holding
        .iter()
        .filter_map(|id| facts.siblings.iter().find(|s| s.id == *id))
        .map(|s| HoldingDevice { id: s.id.clone(), name: s.name.clone(), app_version: s.report.as_ref().map(|r| r.app_version.clone()), needs_update: s.needs_update() })
        .collect();
    Ok(LineStatus {
        generation: own_generation(&root).to_string(),
        move_state: move_state(&root),
        old_line_wanted: d.decision.wanted(),
        old_line_running: crate::conductor::supervisor().has(Line::V06),
        why: d.why,
        holding,
        stopped_waiting: facts.stopped_waiting.iter().map(|w| w.id.clone()).collect(),
        checked_at_ms: facts.checked_at_ms,
        last_pass: crate::move_07::last_pass(),
    })
}

/// "Stop waiting for this computer": this app stops counting it
/// until one of its backups changes again. The Vault's device list is not
/// touched.
#[tauri::command]
pub fn line_stop_waiting(app: tauri::AppHandle, device: String) -> Result<(), String> {
    let root = crate::profile::root(&app)?;
    change_facts(&root, |f| f.stop_waiting(&device, now_ms()))?;
    log::info!("[line] stopped waiting for {}", &device[..device.len().min(8)]);
    Ok(())
}

#[tauri::command]
pub fn line_wait_again(app: tauri::AppHandle, device: String) -> Result<(), String> {
    let root = crate::profile::root(&app)?;
    change_facts(&root, |f| {
        f.wait_again(&device);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(gen: &str) -> LineReport {
        LineReport { v: 1, network_generation: gen.into(), app_version: "0.9.1".into(), move_state: MoveState::Moved }
    }

    fn sib(id: &str, line: SiblingLine, report: Option<LineReport>, stamp: &str) -> KnownSibling {
        KnownSibling { id: id.into(), name: None, line, report, manifest_generation: None, material_same: Some(true), stamps: Stamps { manifest_at: Some(stamp.into()), ..Default::default() } }
    }

    #[test]
    fn a_device_counts_as_moved_only_when_it_says_so() {
        assert_eq!(classify(Some(&report("holochain-0.7")), None, Some(true)), SiblingLine::Moved);
        assert_eq!(classify(None, Some("holochain-0.7"), None), SiblingLine::Moved, "a manifest that says 0.7");
        assert_eq!(classify(Some(&report("holochain-0.6")), Some("holochain-0.6"), Some(true)), SiblingLine::OnOld, "0.9.1 still moving");
        assert_eq!(classify(None, None, Some(true)), SiblingLine::OnOld, "a release before 0.9.0 says nothing");
        assert_eq!(classify(None, Some("holochain-0.6"), None), SiblingLine::OnOld, "0.9.0");
        assert_eq!(classify(None, Some("something else"), None), SiblingLine::OnOld, "an unknown word is not a move");
    }

    #[test]
    fn a_device_with_other_key_material_never_holds_the_old_line() {
        assert_eq!(classify(None, None, Some(false)), SiblingLine::Separate);
        assert_eq!(classify(None, Some("holochain-0.6"), Some(false)), SiblingLine::Separate);
        assert_eq!(classify(None, None, None), SiblingLine::OnOld, "an unread material is not separate");
    }

    #[test]
    fn only_an_older_release_is_told_to_update() {
        assert!(sib("a", SiblingLine::OnOld, None, "1").needs_update());
        assert!(!sib("a", SiblingLine::OnOld, Some(report("holochain-0.6")), "1").needs_update(), "0.9.1 still moving");
        assert!(!sib("a", SiblingLine::Moved, Some(report("holochain-0.7")), "1").needs_update());
    }

    #[test]
    fn stopping_the_wait_removes_a_device_until_its_backups_change() {
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "100")], 1);
        assert_eq!(f.holding().len(), 1);
        f.stop_waiting("aaaa", 2).unwrap();
        assert!(f.holding().is_empty());
        let Siblings::Heard(list) = f.siblings() else { panic!() };
        assert!(list[0].removed);
        // The same stamps next round: still stopped.
        assert!(f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "100")], 3).is_empty());
        assert!(f.holding().is_empty());
        // It wrote again: the wait resumes by itself.
        assert_eq!(f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "200")], 4), vec!["aaaa".to_string()]);
        assert_eq!(f.holding().len(), 1);
        assert!(f.stop_waiting("nobody", 5).is_err());
    }

    #[test]
    fn a_device_missing_from_a_round_is_never_removed_by_it() {
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "100")], 1);
        f.stop_waiting("aaaa", 2).unwrap();
        f.take_round(Vec::new(), 3);
        assert_eq!(f.stopped_waiting.len(), 1, "the stop stays while the device is not listed");
    }

    #[test]
    fn a_separate_device_is_left_out_of_the_siblings() {
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::Separate, None, "1"), sib("bbbb", SiblingLine::Moved, Some(report("holochain-0.7")), "1")], 1);
        let Siblings::Heard(list) = f.siblings() else { panic!() };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].generation, "holochain-0.7");
        assert!(f.holding().is_empty());
    }

    fn moved_marker() -> MarkerRead {
        MarkerRead::Present(Marker::new(&MARKER, 1))
    }

    #[test]
    fn unknown_facts_want_the_old_line() {
        let d = decide(&Known::Unknown, moved_marker(), true, 10);
        assert!(d.decision.wanted());
    }

    #[test]
    fn a_sibling_on_old_holds_the_old_line_and_is_named() {
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "1")], 1);
        let d = decide(&Known::Facts(f), moved_marker(), true, 10);
        assert_eq!(d.decision, Decision::Wanted(Why::SiblingOnOld("aaaa".into())));
        assert_eq!(d.holding, vec!["aaaa".to_string()]);
    }

    #[test]
    fn a_fresh_profile_beside_a_device_on_old_wants_the_old_line() {
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "1")], 1);
        let d = decide(&Known::Facts(f.clone()), MarkerRead::Absent, false, 10);
        assert!(d.decision.wanted(), "the library alone would answer Fresh");
        f.stop_waiting("aaaa", 2).unwrap();
        assert_eq!(decide(&Known::Facts(f), MarkerRead::Absent, false, 10).decision, Decision::NotWanted(Why::Fresh));
        assert_eq!(decide(&Known::NeverShared, MarkerRead::Absent, false, 10).decision, Decision::NotWanted(Why::Fresh));
    }

    #[test]
    fn everyone_moved_and_nothing_carried_yet_still_wants_it() {
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::Moved, Some(report("holochain-0.7")), "1")], 1);
        let d = decide(&Known::Facts(f.clone()), moved_marker(), true, 10);
        assert_eq!(d.decision, Decision::Wanted(Why::NotCarried));
        let mut m = Marker::new(&MARKER, 1);
        m.carried_all_ms = Some(5);
        assert_eq!(decide(&Known::Facts(f), MarkerRead::Present(m), true, 10).decision, Decision::NotWanted(Why::NothingLeft));
    }

    #[test]
    fn a_moved_profile_starts_with_0_7_leading_and_0_6_only_while_wanted() {
        assert_eq!(plan(false, true, true), (Line::V06, Some(Line::V07)), "not moved: 0.6 leads, the move runs");
        assert_eq!(plan(false, false, true), (Line::V06, Some(Line::V07)), "fresh, beside a device on 0.6");
        assert_eq!(plan(false, false, false), (Line::V07, None), "fresh, alone: 0.7 from the first start");
        assert_eq!(plan(true, true, true), (Line::V07, Some(Line::V06)), "moved, a device still on 0.6");
        assert_eq!(plan(true, true, false), (Line::V07, None), "moved, nothing needs 0.6: it stays stopped");
        // Whatever the facts, 0.6 is never the line a moved profile leads with:
        // a retired primary would send the watch into restarts.
        for (old, wanted) in [(true, true), (true, false), (false, true), (false, false)] {
            assert_eq!(plan(true, old, wanted).0, Line::V07);
        }
    }

    #[test]
    fn facts_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("line-facts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = LineFacts::default();
        f.take_round(vec![sib("aaaa", SiblingLine::OnOld, None, "1")], 7);
        f.save(&dir).unwrap();
        assert_eq!(LineFacts::load(&dir), Some(f));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn stamps(manifest: &str, line: &str) -> Stamps {
        Stamps { manifest_at: Some(manifest.into()), line_at: Some(line.into()), ..Default::default() }
    }

    #[test]
    fn a_device_back_on_an_earlier_release_counts_as_on_the_old_line() {
        // Moved, then an earlier release installed by hand: its manifest is
        // newer than the line object and names 0.6.
        let back = current_report(Some(report("holochain-0.7")), Some("holochain-0.6"), &stamps("200", "100"));
        assert_eq!(back, None);
        assert_eq!(classify(back.as_ref(), Some("holochain-0.6"), Some(true)), SiblingLine::OnOld);
        let s = KnownSibling { report: back, manifest_generation: Some("holochain-0.6".into()), ..sib("a", SiblingLine::OnOld, None, "200") };
        assert!(s.needs_update(), "named as on an older version, with no version of its own shown");
        // The line object newer than the manifest: moved, its manifest held back.
        assert!(current_report(Some(report("holochain-0.7")), Some("holochain-0.6"), &stamps("100", "200")).is_some());
        // Still moving on this release: it says 0.6 itself.
        assert!(current_report(Some(report("holochain-0.6")), Some("holochain-0.6"), &stamps("200", "100")).is_some());
        // Moved and its manifest says so.
        assert!(current_report(Some(report("holochain-0.7")), Some("holochain-0.7"), &stamps("200", "100")).is_some());
        // Stamps not read: the line object stands.
        assert!(current_report(Some(report("holochain-0.7")), Some("holochain-0.6"), &Stamps::default()).is_some());
        // A newer `ais` object counts as the manifest does.
        let ais = Stamps { manifest_at: Some("50".into()), ais_at: Some("300".into()), line_at: Some("100".into()), ..Default::default() };
        assert_eq!(current_report(Some(report("holochain-0.7")), Some("holochain-0.6"), &ais), None);
    }

    /// A moved profile whose 0.6 line holds data and whose marker records
    /// that nothing was left on it.
    fn stopped_profile() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let (lair, conductor) = crate::conductor::line_dirs(root, Line::V06);
        std::fs::create_dir_all(&lair).unwrap();
        std::fs::write(flowsta_hc_lines::keystore::config_path(&lair), "x").unwrap();
        std::fs::create_dir_all(conductor.join("databases").join("authored")).unwrap();
        std::fs::write(conductor.join("databases").join("authored").join("a.db"), "1").unwrap();
        let mut m = Marker::new(&MARKER, 1);
        m.carried_all_ms = Some(5);
        m.old_line_needed = Some(false);
        m.save(root, &MARKER).unwrap();
        tmp
    }

    fn carried_all(root: &Path) -> Option<u64> {
        match Marker::load(root, &MARKER) {
            MarkerRead::Present(m) => m.carried_all_ms,
            _ => panic!("no marker"),
        }
    }

    #[test]
    fn the_old_line_stays_stopped_while_nothing_wrote_to_it() {
        let tmp = stopped_profile();
        note_stopped(tmp.path());
        assert!(!reopen_if_written_since_stop(tmp.path()));
        assert_eq!(carried_all(tmp.path()), Some(5));
        assert!(!decide(&Known::NeverShared, Marker::load(tmp.path(), &MARKER), true, 10).decision.wanted());
    }

    #[test]
    fn an_earlier_release_writing_after_the_stop_opens_the_old_line_until_a_full_pass() {
        let tmp = stopped_profile();
        note_stopped(tmp.path());
        // An earlier release ran here by hand and wrote a conversation.
        let db = crate::conductor::line_dirs(tmp.path(), Line::V06).1.join("databases").join("authored").join("a.db");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&db).unwrap().set_modified(later).unwrap();
        assert!(reopen_if_written_since_stop(tmp.path()));
        assert_eq!(carried_all(tmp.path()), None);
        assert!(!tmp.path().join(STOP_PRINT_FILE).exists());
        let d = decide(&Known::NeverShared, Marker::load(tmp.path(), &MARKER), true, 10);
        assert_eq!(d.decision, Decision::Wanted(Why::NotCarried), "0.6 runs until a full pass carries what it wrote");
        // Once is enough: nothing more to clear.
        assert!(!reopen_if_written_since_stop(tmp.path()));
    }

    #[test]
    fn a_stop_with_no_record_opens_the_old_line_once_more() {
        let tmp = stopped_profile();
        assert!(reopen_if_written_since_stop(tmp.path()));
        assert_eq!(carried_all(tmp.path()), None);
    }

    #[test]
    fn the_old_line_running_again_clears_the_record_that_nothing_is_left() {
        let tmp = stopped_profile();
        crate::conductor::note_old_line_needed(tmp.path(), true);
        assert_eq!(carried_all(tmp.path()), None, "a device back on 0.6 may write what nobody else carries");
        let MarkerRead::Present(m) = Marker::load(tmp.path(), &MARKER) else { panic!() };
        assert_eq!(m.old_line_needed, Some(true));
        crate::conductor::note_old_line_needed(tmp.path(), false);
        assert_eq!(carried_all(tmp.path()), None, "stopping never records a full pass by itself");
    }
}
