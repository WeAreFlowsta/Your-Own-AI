//! The parts a release needs beside the app itself: the NVIDIA engine and
//! the Projects helper. A release's signed manifest names them (`parts`:
//! a name, a target, a version, a size and a SHA-256, never an address), so
//! the version doing the updating can fetch the ones this person has before
//! it offers the restart, and the new version starts complete.
//!
//! `parts.json` beside this crate is what the release build publishes for
//! the release tool to put in the manifest; a test here holds it to the
//! pins the app is compiled with.

use flowsta_update_check::Part;
use tauri::AppHandle;

/// Fetch, ahead of the update, each named part this install has and the
/// new version changes. Each is checked against the manifest's hash. A
/// failure is logged and left to the new version's first start. Returns
/// how many were fetched.
pub(crate) async fn fetch_ahead(app: &AppHandle, parts: &[Part]) -> usize {
    // An install with downloads switched off fetches nothing by itself.
    if crate::llm::downloads_blocked() {
        return 0;
    }
    let mut fetched = 0;
    for part in parts {
        let outcome = if part.name == crate::engine::PART_NAME && crate::engine::wants_ahead(app, &part.target, &part.version) {
            Some(crate::engine::fetch_ahead(app, &part.version, &part.sha256).await)
        } else if part.name == crate::build_install::PART_NAME && crate::build_install::wants_ahead(app, &part.target, &part.version) {
            Some(crate::build_install::fetch_ahead(app, &part.version, &part.sha256).await)
        } else {
            None
        };
        match outcome {
            Some(Ok(())) => fetched += 1,
            Some(Err(e)) => log::warn!("[update] {} {} was not fetched ahead (the new version fetches it at its first start): {}", part.name, part.version, e),
            None => {}
        }
    }
    fetched
}

/// Test builds only: fetch the parts named in `YOAI_TEST_PARTS_AHEAD` (a
/// JSON list, as a manifest's `parts`) a moment after start, as the updater
/// does once an update is downloaded (`e2e/parts-ahead-live.sh`). A release
/// build has no such door: its parts come from a signed manifest alone.
#[cfg(debug_assertions)]
pub(crate) fn fetch_named_by_the_test(app: &AppHandle) {
    let Ok(text) = std::env::var("YOAI_TEST_PARTS_AHEAD") else { return };
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        log::warn!("[update] TEST: YOAI_TEST_PARTS_AHEAD is not a list");
        return;
    };
    let word = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let parts: Vec<Part> = list
        .iter()
        .map(|v| Part { name: word(v, "name"), target: word(v, "target"), version: word(v, "version"), size: v.get("size").and_then(|x| x.as_u64()).unwrap_or(0), sha256: word(v, "sha256") })
        .collect();
    log::warn!("[update] TEST: fetching {} named part(s) ahead", parts.len());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let n = fetch_ahead(&app, &parts).await;
        log::info!("[update] TEST: {} part(s) fetched ahead", n);
    });
}

#[cfg(not(debug_assertions))]
pub(crate) fn fetch_named_by_the_test(_app: &AppHandle) {}

#[cfg(test)]
mod tests {
    #[derive(serde::Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Listed {
        name: String,
        target: String,
        version: String,
        size: u64,
        sha256: String,
    }

    #[test]
    fn parts_json_names_exactly_what_this_app_pins() {
        let listed: Vec<Listed> = serde_json::from_str(include_str!("../parts.json")).expect("parts.json is a list of parts");
        assert!(listed.iter().all(|p| p.size > 0 && p.sha256.len() == 64), "every part has a size and a SHA-256");
        let mut sorted: Vec<&Listed> = listed.iter().collect();
        sorted.sort();
        sorted.dedup_by(|a, b| a.name == b.name && a.target == b.target);
        assert_eq!(sorted.len(), listed.len(), "no part is named twice for a target");

        // The NVIDIA engine: every platform with a build, at the pinned tag,
        // size and hash.
        let engine: Vec<(String, String, u64, String)> = {
            let mut v: Vec<_> = listed.iter().filter(|p| p.name == crate::engine::PART_NAME).map(|p| (p.target.clone(), p.version.clone(), p.size, p.sha256.clone())).collect();
            v.sort();
            v
        };
        let mut pinned_engine = crate::engine::pinned_parts();
        pinned_engine.sort();
        assert_eq!(engine, pinned_engine, "parts.json and engine.rs disagree: move them together");

        // The Projects helper: every platform with a build, at the pinned
        // version and hash (its size is carried by parts.json alone).
        let helper: Vec<(String, String, String)> = {
            let mut v: Vec<_> = listed.iter().filter(|p| p.name == crate::build_install::PART_NAME).map(|p| (p.target.clone(), p.version.clone(), p.sha256.clone())).collect();
            v.sort();
            v
        };
        let mut pinned_helper = crate::build_install::pinned_parts();
        pinned_helper.sort();
        assert_eq!(helper, pinned_helper, "parts.json and build_install.rs disagree: move them together");

        assert_eq!(engine.len() + helper.len(), listed.len(), "parts.json names a part this app does not know");
    }
}
