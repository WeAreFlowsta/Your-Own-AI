//! Notices when the Flowsta identity in the Vault changes while the app runs.
//!
//! The data folder (identity profile) is chosen once, at launch, by the
//! identity unlocked in the Vault (`profile::init`). The Vault's identity
//! switcher (Vault 1.5.0) - and a Vault reset followed by another recovery
//! phrase, possible today - can change that identity under a running app.
//! The app must then stop acting for the old identity at once, and offer a
//! restart that opens the new identity's profile. It never swaps profiles in
//! place (every open store, the records engine and the key store would have
//! to reopen), and it never wipes the old identity's session: switching back
//! and restarting finds it where it was.
//!
//! What a switch does, in order:
//! - online requests stop being authorised (`flowsta::session_identity_ok`
//!   answers false without wiping; its positive cache is cleared);
//! - online replies already streaming stop, and the service is told to end
//!   them with the token each reply started with (after the switch the
//!   session check refuses to hand one out, so a fresh fetch would fail and
//!   the old identity would be metered for the rest of the reply);
//! - Vault backups for this profile stop quietly (`identity_switched`);
//! - the profile's record of who it belongs to is frozen (`profile::bind`
//!   refuses), so a sign-in or a backup cannot rebind the old identity's
//!   folder to the new identity;
//! - the window is told (`vault-identity-switched`) and offers the restart.
//!
//! Detection does not depend on any Vault push event (Vault 1.5.0 plans
//! one; correctness must never rely on receiving it): the online path checks
//! on every request, and this watcher polls the Vault's `/status` every
//! ten seconds. A locked Vault says nothing about who it is, so locked is
//! never a switch; only an UNLOCKED Vault under a different key is, seen on
//! two reads a few seconds apart (several Vault copies on different ports
//! must not flap the app into a switch).
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};

static SWITCHED: AtomicBool = AtomicBool::new(false);
/// The online chat replies in flight (request ids), so a switch can stop them.
static ONLINE_CHAT: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn notify() -> &'static tokio::sync::Notify {
    static N: OnceLock<tokio::sync::Notify> = OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

/// The Vault's identity changed since this process chose its profile.
pub(crate) fn switched() -> bool {
    SWITCHED.load(SeqCst)
}

/// Resolves when a switch happens (at once when it already has).
pub(crate) async fn wait_switched() {
    let fut = notify().notified();
    tokio::pin!(fut);
    fut.as_mut().enable();
    if switched() {
        return;
    }
    fut.await;
}

/// An online chat reply in flight; dropping it forgets the id.
pub(crate) struct OnlineChat(String);

impl OnlineChat {
    pub(crate) fn register(request_id: &str) -> Self {
        ONLINE_CHAT.lock().unwrap_or_else(|e| e.into_inner()).push(request_id.to_string());
        OnlineChat(request_id.to_string())
    }
}

impl Drop for OnlineChat {
    fn drop(&mut self) {
        let mut ids = ONLINE_CHAT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = ids.iter().position(|x| *x == self.0) {
            ids.remove(i);
        }
    }
}

/// Two agent keys name the same identity. Compared as the Vault's partition
/// key (the raw 39 bytes, hashed) so two spellings of one key agree; a key
/// that does not decode is compared as text.
pub(crate) fn same_identity(a: &str, b: &str) -> bool {
    match (crate::profile::partition_key(a), crate::profile::partition_key(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
}

/// Who this profile belongs to: the identity recorded for its folder, else
/// the identity of the Flowsta session stored in it. None = nobody yet (the
/// first identity to sign in adopts it; that is not a switch).
fn baseline(app: &tauri::AppHandle) -> Option<String> {
    crate::profile::bound_identity(app).or_else(|| crate::flowsta::stored_session_key(app))
}

/// Record a switch and act on it. `from` is who the profile belonged to.
pub(crate) fn note_switch(app: &tauri::AppHandle, from: Option<&str>, to: &str) {
    if switched() {
        return;
    }
    // An unbound profile holding `from`'s data is `from`'s: record that
    // before freezing, so the restart opens `to` in a folder of its own
    // rather than reusing this one.
    if crate::profile::bound_identity(app).is_none() {
        if let Some(f) = from {
            crate::profile::bind(app, f);
        }
    }
    if SWITCHED.swap(true, SeqCst) {
        return;
    }
    crate::flowsta::clear_identity_cache();
    notify().notify_waiters();
    let ids: Vec<String> = ONLINE_CHAT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let stopped = {
        let st = app.state::<crate::llm::LLMState>();
        ids.iter().map(|id| st.live_streams.stop(Some(id))).sum::<usize>()
    };
    log::warn!(
        "[identity] the Vault now holds another Flowsta identity ({} -> {}): online use and Vault backups stop for this profile, {} online reply(ies) stopped; a restart opens that identity's profile",
        from.map(short).unwrap_or_else(|| "unknown".into()),
        short(to),
        stopped
    );
    let _ = app.emit("vault-identity-switched", serde_json::json!({}));
}

fn short(key: &str) -> String {
    crate::profile::partition_key(key).unwrap_or_else(|| key.chars().take(12).collect())
}

/// Watch the Vault for the rest of the process.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            if switched() {
                return;
            }
            let Some(base) = baseline(&app) else { continue };
            let Some(live) = unlocked_key().await else { continue };
            if same_identity(&base, &live) {
                continue;
            }
            // Confirm on a second read: one answer from a stray Vault copy
            // on another port must not switch the app.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let Some(again) = unlocked_key().await else { continue };
            if !same_identity(&again, &live) || same_identity(&base, &again) {
                continue;
            }
            note_switch(&app, Some(&base), &again);
            return;
        }
    });
}

/// The key of an unlocked Vault, or None (absent, locked, no key).
async fn unlocked_key() -> Option<String> {
    let v = crate::flowsta::find_vault().await;
    if v.unlocked { v.agent_pub_key } else { None }
}

/// The window asks on mount (an event sent before it listened is not lost).
#[tauri::command]
pub fn identity_switched() -> bool {
    switched()
}

/// Relaunch into the identity the Vault holds now (`profile::init` picks it).
#[tauri::command]
pub fn restart_after_identity_switch(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("[identity] restarting to open the Vault's identity");
    #[cfg(not(debug_assertions))]
    {
        app.restart()
    }
    // Dev: the webview comes from the dev server, which a programmatic
    // restart does not survive (same split as reset.rs).
    #[cfg(debug_assertions)]
    {
        log::info!("[identity] dev build - exiting; re-run `npm run tauri dev`");
        app.exit(0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "uhCAk75qJ5oobyfm3Lh-akZIQSe2zpSTtG1Pcxs23qTFoQwY_GDWY";
    const B: &str = "uhCAk0O4EJ97RZ7eX2wf9x08PWjNj3Avt2K1SdU8tgPzWoQBwWk0s";

    #[test]
    fn same_identity_compares_keys_not_spelling() {
        assert!(same_identity(A, A));
        assert!(same_identity(A, &format!("  {A} ")));
        assert!(!same_identity(A, B));
        // Undecodable keys fall back to text.
        assert!(same_identity("not-a-key", "not-a-key"));
        assert!(!same_identity("not-a-key", A));
    }
}
